//! Guided setup: finds the game values the sight needs (held weapon, zero
//! level, camera pitch, FOV, rangefinder distance) in memory. The user only
//! does simple things in game and presses Alt+F12; scanning, narrowing,
//! verification, pointer scan and binding are automatic.

use crate::memory::binding::{Binding, Field, Loc};
use crate::memory::pointer::{self, PtrChain, PtrScanOptions};
use crate::memory::scan::{Cmp, Progress, Scanner, VType};
use crate::memory::Process;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    Weapon,
    Zero,
    Pitch,
    Fov,
    Distance,
}

impl Task {
    pub const ALL: [Task; 5] = [Task::Weapon, Task::Zero, Task::Pitch, Task::Fov, Task::Distance];
    pub fn field(self) -> Field {
        match self {
            Task::Weapon => Field::WeaponId,
            Task::Zero => Field::ZeroIndex,
            Task::Pitch => Field::CameraPitch,
            Task::Fov => Field::CameraFov,
            Task::Distance => Field::AimDistance,
        }
    }
    pub fn name(self) -> (&'static str, &'static str) {
        match self {
            Task::Weapon => ("手持武器", "Held weapon"),
            Task::Zero => ("归零档位", "Zero level"),
            Task::Pitch => ("镜头俯仰", "Camera pitch"),
            Task::Fov => ("镜头视角", "Camera FOV"),
            Task::Distance => ("测距距离", "Range distance"),
        }
    }
    /// type used for scanning
    fn scan_type(self) -> VType {
        match self {
            Task::Weapon | Task::Zero => VType::I32,
            _ => VType::F32,
        }
    }
    /// type of the bound value (the zero level is a single byte inside a scanned word)
    fn vtype(self) -> VType {
        match self {
            Task::Weapon => VType::I32,
            Task::Zero => VType::U8,
            _ => VType::F32,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Idle,
    Scanning,
    WaitUser,
    WaitStill(Instant),
    PointerScan,
    Done,
    Failed(String),
}

#[derive(Clone, Copy)]
struct Cand {
    addr: u64,
    scale: f64,
    base: f64,
}

const RAD: f64 = 57.295_779_513;
const CAND_LIMIT: u64 = 50_000;
const MAX_CONFIRMS: u32 = 9;

pub struct Guide {
    pub step: Step,
    pub task: Task,
    pub queue: VecDeque<Task>,
    pub total: usize,
    pub progress: Arc<Progress>,
    pub count: u64,
    /// number of zero levels of the held weapon (Zero task)
    pub zero_levels: usize,
    /// field of view configured by the user, to pick among FOV candidates
    pub fov_hint: f64,
    pub still: Duration,
    scanner: Arc<Mutex<Scanner>>,
    job: Option<JoinHandle<anyhow::Result<u64>>>,
    ptr_job: Option<JoinHandle<Vec<PtrChain>>>,
    after_scan: Step,
    confirms: u32,
    rounds: u32,
    cands: Option<Vec<Cand>>,
    verified: u32,
    pending: Option<(u64, f64)>,
}

impl Default for Guide {
    fn default() -> Self {
        Guide {
            step: Step::Idle,
            task: Task::Weapon,
            queue: VecDeque::new(),
            total: 0,
            progress: Arc::new(Progress::default()),
            count: 0,
            zero_levels: 2,
            fov_hint: 60.0,
            still: Duration::from_secs(3),
            scanner: Arc::new(Mutex::new(Scanner::new(VType::I32))),
            job: None,
            ptr_job: None,
            after_scan: Step::Idle,
            confirms: 0,
            rounds: 0,
            cands: None,
            verified: 0,
            pending: None,
        }
    }
}

fn read_f32(p: &dyn Process, a: u64) -> Option<f64> {
    let mut b = [0u8; 4];
    p.read(a, &mut b).then(|| f32::from_le_bytes(b) as f64).filter(|v| v.is_finite())
}

impl Guide {
    pub fn active(&self) -> bool {
        !matches!(self.step, Step::Idle | Step::Done | Step::Failed(_))
    }

    /// Run the given tasks one after another.
    pub fn start(&mut self, p: Arc<dyn Process>, tasks: &[Task]) {
        self.cancel();
        self.queue = tasks.iter().copied().collect();
        self.total = self.queue.len();
        self.next_task(p);
    }

    fn next_task(&mut self, p: Arc<dyn Process>) {
        let Some(t) = self.queue.pop_front() else {
            self.step = Step::Done;
            return;
        };
        self.task = t;
        self.confirms = 0;
        self.rounds = 0;
        self.cands = None;
        self.verified = 0;
        self.count = 0;
        self.pending = None;
        if t == Task::Distance {
            // exact values typed by the user; no snapshot needed
            *self.scanner.lock().unwrap() = Scanner::new(VType::F32);
            self.step = Step::WaitUser;
        } else {
            self.run(p, Cmp::Unknown, true, Step::WaitUser);
        }
    }

    fn run(&mut self, p: Arc<dyn Process>, cmp: Cmp, first: bool, then: Step) {
        let sc = self.scanner.clone();
        let prog = Arc::new(Progress::default());
        self.progress = prog.clone();
        self.after_scan = then;
        self.step = Step::Scanning;
        let (vt, tol) = (self.task.scan_type(), if self.task == Task::Distance { 0.6 } else { 0.01 });
        self.job = Some(std::thread::spawn(move || {
            let mut s = sc.lock().unwrap();
            if first {
                *s = Scanner::new(vt);
                s.tolerance = tol;
                s.first_scan(p.as_ref(), cmp, &prog)
            } else {
                s.next_scan(p.as_ref(), cmp, &prog)
            }
        }));
    }

    pub fn cancel(&mut self) {
        self.progress.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        self.queue.clear();
        self.step = Step::Idle;
    }

    /// The Distance task needs a number typed by the user instead of Alt+F12.
    pub fn needs_value(&self) -> bool {
        self.step == Step::WaitUser && self.task == Task::Distance
    }

    fn zero_expected(&self, confirm: u32) -> f64 {
        let n = self.zero_levels.clamp(2, 3) as u32;
        ((confirm - 1) % n) as f64
    }

    /// Alt+F12 / button (or a typed value for the Distance task).
    pub fn confirm(&mut self, p: Option<Arc<dyn Process>>, value: Option<f64>) {
        let Some(p) = p else { return };
        if self.step != Step::WaitUser {
            return;
        }
        match self.task {
            Task::Weapon => {
                self.confirms += 1;
                self.run(p, Cmp::Changed, false, Step::WaitStill(Instant::now()));
            }
            Task::Zero => {
                self.confirms += 1;
                if self.cands.is_some() {
                    self.verify_zero(p);
                } else {
                    self.run(p, Cmp::Changed, false, Step::WaitStill(Instant::now()));
                }
            }
            Task::Pitch | Task::Fov => {
                self.confirms += 1;
                if self.cands.is_some() {
                    self.verify(p);
                } else {
                    self.run(p, Cmp::Changed, false, Step::WaitUser);
                }
            }
            Task::Distance => {
                let Some(v) = value.filter(|v| *v > 0.0) else { return };
                self.confirms += 1;
                self.run(p, Cmp::Exact(v), self.confirms == 1, Step::WaitUser);
            }
        }
    }

    /// Posture the user is in after `confirms` confirmations: true = "high"
    /// (looking up / hip view), false = "low" (looking down / aiming).
    fn high(&self) -> bool {
        match self.task {
            Task::Pitch => self.confirms % 2 == 1,
            _ => self.confirms % 2 == 0,
        }
    }

    fn build_cands(&mut self, p: &dyn Process) {
        let hits = self.scanner.lock().unwrap().hits(CAND_LIMIT as usize);
        let mut c = vec![];
        for h in hits {
            let Some(v) = read_f32(p, h.addr) else { continue };
            let a = v.abs();
            match self.task {
                Task::Pitch => {
                    if (55.0..=90.5).contains(&a) {
                        c.push(Cand { addr: h.addr, scale: v.signum(), base: 0.0 });
                    } else if (0.96..=1.58).contains(&a) {
                        c.push(Cand { addr: h.addr, scale: v.signum() * RAD, base: 0.0 });
                    }
                }
                _ => {
                    if (30.0..=130.0).contains(&v) {
                        c.push(Cand { addr: h.addr, scale: 1.0, base: v });
                    } else if (0.52..=2.27).contains(&v) {
                        c.push(Cand { addr: h.addr, scale: RAD, base: v * RAD });
                    }
                }
            }
        }
        if !c.is_empty() {
            self.cands = Some(c);
            self.verified = 0;
        }
    }

    fn verify(&mut self, p: Arc<dyn Process>) {
        let high = self.high();
        let task = self.task;
        let mut c = self.cands.take().unwrap_or_default();
        c.retain(|k| {
            let Some(v) = read_f32(p.as_ref(), k.addr).map(|v| v * k.scale) else { return false };
            match (task, high) {
                (Task::Pitch, true) => (55.0..=90.5).contains(&v),
                (Task::Pitch, false) => (-90.5..=-55.0).contains(&v),
                (_, true) => (v - k.base).abs() < 0.5,
                (_, false) => v > 0.5 && v < k.base * 0.97,
            }
        });
        self.verified += 1;
        self.count = c.len() as u64;
        if c.is_empty() {
            self.step = Step::Failed("no value followed your movements".into());
            return;
        }
        if self.verified >= 2 {
            let hint = self.fov_hint;
            let best = if task == Task::Fov {
                *c.iter().min_by(|a, b| (a.base - hint).abs().total_cmp(&(b.base - hint).abs())).unwrap()
            } else {
                c[0]
            };
            self.finalize(p, best.addr, best.scale);
        } else {
            self.cands = Some(c);
            self.step = Step::WaitUser;
        }
    }

    /// Zero level: bytes inside the surviving words that equal the level just set.
    fn build_zero_cands(&mut self, p: &dyn Process) {
        let e = self.zero_expected(self.confirms) as u8;
        let hits = self.scanner.lock().unwrap().hits(CAND_LIMIT as usize);
        let mut c = vec![];
        for h in hits {
            let mut b = [0u8; 4];
            if p.read(h.addr, &mut b) {
                for (i, v) in b.iter().enumerate() {
                    if *v == e {
                        c.push(Cand { addr: h.addr + i as u64, scale: 1.0, base: 0.0 });
                    }
                }
            }
        }
        if !c.is_empty() {
            self.cands = Some(c);
            self.verified = 0;
        }
    }

    fn verify_zero(&mut self, p: Arc<dyn Process>) {
        let e = self.zero_expected(self.confirms) as u8;
        let mut c = self.cands.take().unwrap_or_default();
        c.retain(|k| {
            let mut b = [0u8; 1];
            p.read(k.addr, &mut b) && b[0] == e
        });
        self.verified += 1;
        self.count = c.len() as u64;
        if c.is_empty() {
            self.step = Step::Failed("no value followed the zero level".into());
        } else if (self.verified >= 3 && c.len() <= 4) || self.verified >= 6 {
            self.finalize(p, c[0].addr, 1.0);
        } else {
            self.cands = Some(c);
            self.step = Step::WaitUser;
        }
    }

    fn finalize(&mut self, p: Arc<dyn Process>, addr: u64, scale: f64) {
        self.pending = Some((addr, scale));
        self.step = Step::PointerScan;
        let prog = Arc::new(Progress::default());
        self.progress = prog.clone();
        self.ptr_job = Some(std::thread::spawn(move || pointer::scan(p.as_ref(), addr, &PtrScanOptions::default(), &prog)));
    }

    fn first_hit(&self) -> Option<u64> {
        self.scanner.lock().unwrap().hits(1).first().map(|h| h.addr)
    }

    fn weapon_hash_hit(&self, p: &dyn Process) -> Option<u64> {
        let hits = self.scanner.lock().unwrap().hits(100_000);
        hits.iter().map(|h| h.addr).find(|a| {
            let mut b = [0u8; 4];
            p.read(*a, &mut b) && crate::gamedata::by_hash(u32::from_le_bytes(b) as u64).is_some()
        })
    }

    fn scan_done(&mut self, p: Arc<dyn Process>, n: u64) {
        self.count = n;
        if n == 0 {
            self.step = Step::Failed("0 candidates left".into());
            return;
        }
        let next = std::mem::replace(&mut self.after_scan, Step::Idle);
        match self.task {
            Task::Weapon => {
                let hash = if self.rounds >= 2 && n <= 100_000 { self.weapon_hash_hit(p.as_ref()) } else { None };
                if next == Step::WaitUser && self.rounds > 0 && (n <= 8 || hash.is_some() || self.rounds >= 8) {
                    let target = hash.or_else(|| self.weapon_hash_hit(p.as_ref())).or_else(|| self.first_hit());
                    match target {
                        Some(t) => self.finalize(p, t, 1.0),
                        None => self.step = Step::Failed("0 candidates left".into()),
                    }
                } else {
                    self.step = next;
                }
            }
            Task::Zero => {
                if self.confirms > 0 && next == Step::WaitUser && n <= CAND_LIMIT {
                    self.build_zero_cands(p.as_ref());
                }
                if self.cands.is_none() && self.confirms >= MAX_CONFIRMS {
                    self.step = Step::Failed("too many candidates".into());
                } else {
                    self.step = next;
                }
            }
            Task::Pitch | Task::Fov => {
                if self.confirms > 0 && n <= CAND_LIMIT && self.high() {
                    self.build_cands(p.as_ref());
                }
                if self.cands.is_none() && self.confirms >= MAX_CONFIRMS {
                    self.step = Step::Failed("no plausible value found".into());
                } else {
                    self.step = next;
                }
            }
            Task::Distance => {
                let c = self.confirms;
                if (c >= 2 && n <= 20) || c >= 4 {
                    let t = self.first_hit().unwrap_or(0);
                    self.finalize(p, t, 1.0);
                } else {
                    self.step = next;
                }
            }
        }
    }

    /// Advance the state machine; returns a binding each time a task finishes.
    pub fn tick(&mut self, p: Option<Arc<dyn Process>>) -> Option<(Field, Binding)> {
        if self.job.as_ref().map(|j| j.is_finished()).unwrap_or(false) {
            let r = self.job.take().unwrap().join();
            if self.step == Step::Scanning {
                match (r, p.clone()) {
                    (Ok(Ok(n)), Some(p)) => self.scan_done(p, n),
                    (Ok(Err(e)), _) => self.step = Step::Failed(format!("{e:#}")),
                    _ => self.step = Step::Failed("scan failed".into()),
                }
            }
        }
        if let Step::WaitStill(t) = self.step {
            if t.elapsed() > self.still {
                if let Some(p) = p.clone() {
                    self.rounds += 1;
                    self.run(p, Cmp::Unchanged, false, Step::WaitUser);
                }
            }
        }
        if self.ptr_job.as_ref().map(|j| j.is_finished()).unwrap_or(false) {
            let chains = self.ptr_job.take().unwrap().join().unwrap_or_default();
            if self.step != Step::PointerScan {
                return None;
            }
            let (addr, scale) = self.pending.take()?;
            let loc = match chains.into_iter().next() {
                Some(c) => Loc::Pointer(c),
                None => Loc::Absolute(addr),
            };
            let out = (self.task.field(), Binding { vtype: self.task.vtype(), loc, scale, add: 0.0 });
            match p {
                Some(p) => self.next_task(p),
                None => self.step = Step::Done,
            }
            return Some(out);
        }
        None
    }

    /// What the user should do now (zh, en); None when idle.
    pub fn prompt(&self) -> Option<(String, String)> {
        if !self.active() {
            return None;
        }
        let idx = self.total - self.queue.len();
        let (nz, ne) = self.task.name();
        let head = (format!("[{idx}/{} {nz}] ", self.total), format!("[{idx}/{} {ne}] ", self.total));
        let (zh, en): (String, String) = match &self.step {
            Step::Scanning => {
                let pc = (self.progress.fraction() * 100.0) as u32;
                (format!("正在扫描内存 {pc}% … 请保持不动"), format!("Scanning memory {pc}% … hold still"))
            }
            Step::PointerScan => ("已找到，正在建立永久绑定…".into(), "Found; building a permanent binding…".into()),
            Step::WaitStill(_) => ("保持不动，什么都不要按…".into(), "Hold still, do not press anything…".into()),
            Step::WaitUser => match self.task {
                Task::Weapon => ("切换到另一把武器，然后按 Alt+F12".into(), "Switch to another weapon, then press Alt+F12".into()),
                Task::Zero => {
                    if self.zero_expected(self.confirms + 1) == 0.0 {
                        ("把归零调到最低一档，然后按 Alt+F12".into(), "Set zeroing to the LOWEST level, then press Alt+F12".into())
                    } else {
                        ("把归零调高一档，然后按 Alt+F12".into(), "Raise zeroing by ONE level, then press Alt+F12".into())
                    }
                }
                Task::Pitch => {
                    if self.confirms % 2 == 0 {
                        ("抬头看向正上方（到头），保持住按 Alt+F12".into(), "Look straight UP as far as possible, hold and press Alt+F12".into())
                    } else {
                        ("低头看向脚下（到底），保持住按 Alt+F12".into(), "Look straight DOWN as far as possible, hold and press Alt+F12".into())
                    }
                }
                Task::Fov => {
                    if self.confirms % 2 == 0 {
                        ("按住瞄准（开镜），保持住按 Alt+F12".into(), "Hold AIM (scope in), keep holding and press Alt+F12".into())
                    } else {
                        ("松开瞄准回到正常视角，然后按 Alt+F12".into(), "Release aim (normal view), then press Alt+F12".into())
                    }
                }
                Task::Distance => (
                    "用测距仪对准一个目标，在 CotwMod 窗口输入显示的距离并回车（换目标重复 2–4 次）".into(),
                    "Range a target, type the shown distance in the CotwMod window and press Enter (repeat 2–4 times on other targets)".into(),
                ),
            },
            _ => return None,
        };
        Some((head.0 + &zh, head.1 + &en))
    }
}
