//! Guided automatic detection of the held-weapon ID in game memory.
//! The user only switches weapons when asked (and presses Alt+F12);
//! scanning, narrowing, pointer scan and binding are automatic.

use super::{tr, App};
use cotwmod::memory::binding::{Binding, Field, Loc};
use cotwmod::memory::pointer::{self, PtrChain, PtrScanOptions};
use cotwmod::memory::scan::{Cmp, Progress, Scanner, VType};
use cotwmod::memory::Process;
use eframe::egui::{self, Color32, RichText};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Clone, PartialEq)]
pub enum Step {
    Idle,
    Scanning,
    WaitSwitch,
    WaitStill(Instant),
    PointerScan,
    Done(String),
    Failed(String),
}

pub struct Wizard {
    pub step: Step,
    scanner: Arc<Mutex<Scanner>>,
    job: Option<JoinHandle<anyhow::Result<u64>>>,
    ptr_job: Option<JoinHandle<Vec<PtrChain>>>,
    pub progress: Arc<Progress>,
    pub count: u64,
    pub rounds: u32,
    target: u64,
    after_scan: Step,
}

impl Default for Wizard {
    fn default() -> Self {
        Wizard {
            step: Step::Idle,
            scanner: Arc::new(Mutex::new(Scanner::new(VType::I32))),
            job: None,
            ptr_job: None,
            progress: Arc::new(Progress::default()),
            count: 0,
            rounds: 0,
            target: 0,
            after_scan: Step::Idle,
        }
    }
}

const MAX_ROUNDS: u32 = 8;

impl Wizard {
    fn run(&mut self, p: Arc<dyn Process>, cmp: Cmp, first: bool, then: Step) {
        let sc = self.scanner.clone();
        let prog = Arc::new(Progress::default());
        self.progress = prog.clone();
        self.after_scan = then;
        self.step = Step::Scanning;
        self.job = Some(std::thread::spawn(move || {
            let mut s = sc.lock().unwrap();
            if first {
                *s = Scanner::new(VType::I32);
                s.first_scan(p.as_ref(), cmp, &prog)
            } else {
                s.next_scan(p.as_ref(), cmp, &prog)
            }
        }));
    }

    pub fn start(&mut self, p: Arc<dyn Process>) {
        self.rounds = 0;
        self.run(p, Cmp::Unknown, true, Step::WaitSwitch);
    }

    /// Alt+F12 / button: the user has just switched weapons.
    pub fn on_switch(&mut self, p: Option<Arc<dyn Process>>) {
        if self.step == Step::WaitSwitch {
            if let Some(p) = p {
                self.run(p, Cmp::Changed, false, Step::WaitStill(Instant::now()));
            }
        }
    }

    pub fn cancel(&mut self) {
        self.progress.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        self.step = Step::Idle;
    }

    /// Advance the state machine; returns a binding when finished.
    pub fn tick(&mut self, p: Option<Arc<dyn Process>>) -> Option<Binding> {
        if self.job.as_ref().map(|j| j.is_finished()).unwrap_or(false) {
            match self.job.take().unwrap().join() {
                Ok(Ok(n)) => {
                    self.count = n;
                    if n == 0 {
                        self.step = Step::Failed("0".into());
                        return None;
                    }
                    let next = std::mem::replace(&mut self.after_scan, Step::Idle);
                    // after an "unchanged" pass decide whether we are done
                    let hash_hit = self.rounds >= 2 && n <= 100_000 && p.as_ref().map(|p| self.has_known_hash(p.as_ref())).unwrap_or(false);
                    if next == Step::WaitSwitch && self.rounds > 0 && (n <= 8 || hash_hit || self.rounds >= MAX_ROUNDS) {
                        self.finalize(p.clone());
                    } else {
                        self.step = next;
                    }
                }
                Ok(Err(e)) => self.step = Step::Failed(format!("{e:#}")),
                Err(_) => self.step = Step::Failed("scan thread crashed".into()),
            }
        }
        if let Step::WaitStill(t) = self.step {
            if t.elapsed() > Duration::from_secs(3) {
                if let Some(p) = p.clone() {
                    self.rounds += 1;
                    self.run(p, Cmp::Unchanged, false, Step::WaitSwitch);
                }
            }
        }
        if self.ptr_job.as_ref().map(|j| j.is_finished()).unwrap_or(false) {
            let chains = self.ptr_job.take().unwrap().join().unwrap_or_default();
            let loc = match chains.into_iter().next() {
                Some(c) => Loc::Pointer(c),
                None => Loc::Absolute(self.target),
            };
            let permanent = matches!(loc, Loc::Pointer(_));
            self.step = Step::Done(if permanent { "pointer".into() } else { "absolute".into() });
            return Some(Binding { vtype: VType::I32, loc, scale: 1.0, add: 0.0 });
        }
        None
    }

    /// Does any remaining candidate currently hold a known equipment hash?
    fn has_known_hash(&self, p: &dyn Process) -> bool {
        let hits = self.scanner.lock().unwrap().hits(100_000);
        hits.iter().any(|h| {
            let mut b = [0u8; 4];
            p.read(h.addr, &mut b) && cotwmod::gamedata::by_hash(u32::from_le_bytes(b) as u64).is_some()
        })
    }

    fn finalize(&mut self, p: Option<Arc<dyn Process>>) {
        let Some(p) = p else { return };
        let hits = self.scanner.lock().unwrap().hits(100_000);
        if hits.is_empty() {
            self.step = Step::Failed("0".into());
            return;
        }
        // prefer a candidate whose value is a known equipment hash
        let mut target = hits[0].addr;
        for h in &hits {
            let mut b = [0u8; 4];
            if p.read(h.addr, &mut b) && cotwmod::gamedata::by_hash(u32::from_le_bytes(b) as u64).is_some() {
                target = h.addr;
                break;
            }
        }
        self.target = target;
        self.step = Step::PointerScan;
        let prog = Arc::new(Progress::default());
        self.progress = prog.clone();
        self.ptr_job = Some(std::thread::spawn(move || pointer::scan(p.as_ref(), target, &PtrScanOptions::default(), &prog)));
    }
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let lang = app.s.lang;
    let t = |zh, en| tr(lang, zh, en);
    let process = app.shared.lock().unwrap().process.clone();
    ui.heading(t("自动识别手持武器", "Auto-detect held weapon"));
    let bound = app.s.bindings.contains_key(&Field::WeaponId);
    if bound {
        ui.colored_label(Color32::from_rgb(80, 200, 120), t("● 已绑定手持武器，切换武器会实时更新", "● Held weapon bound: switching updates in real time"));
    }
    match app.wizard.step.clone() {
        Step::Idle | Step::Done(_) | Step::Failed(_) => {
            if let Step::Done(kind) = &app.wizard.step {
                ui.label(if kind == "pointer" {
                    t("完成：已用指针链绑定，游戏重启后仍有效。", "Done: bound with a pointer chain, survives game restarts.")
                } else {
                    t("完成：已绑定（绝对地址，游戏重启后需重新识别）。", "Done: bound (absolute address; redo after a game restart).")
                });
            }
            if let Step::Failed(e) = &app.wizard.step {
                ui.colored_label(ui.visuals().warn_fg_color, format!("{} {e}", t("识别失败，请重试：", "Detection failed, try again:")));
            }
            ui.label(RichText::new(t("步骤：进入游戏并手持一把武器 → 点“开始” → 按提示切换武器后按 Alt+F12，其余全自动。",
                "Steps: hold a weapon in game → Start → when asked, switch weapon and press Alt+F12; the rest is automatic.")).small());
            if ui.add_enabled(process.is_some(), egui::Button::new(t("开始自动识别", "Start detection"))).clicked() {
                app.wizard.start(process.clone().unwrap());
            }
            if process.is_none() {
                ui.label(RichText::new(t("（需要先启动游戏）", "(start the game first)")).small());
            }
        }
        Step::Scanning | Step::PointerScan => {
            ui.label(if app.wizard.step == Step::PointerScan { t("正在做指针扫描…", "Pointer scanning…") } else { t("正在扫描内存…", "Scanning memory…") });
            ui.add(egui::ProgressBar::new(app.wizard.progress.fraction()).show_percentage());
            if ui.button(t("取消", "Cancel")).clicked() {
                app.wizard.cancel();
            }
        }
        Step::WaitSwitch => {
            ui.colored_label(Color32::YELLOW, t("➜ 现在在游戏里切换到另一把武器，然后按 Alt+F12（或点下面按钮）",
                "➜ Switch to a different weapon in game now, then press Alt+F12 (or the button below)"));
            ui.label(format!("{} {}  ·  {} {}", t("剩余候选", "Candidates"), app.wizard.count, t("轮次", "Round"), app.wizard.rounds + 1));
            ui.horizontal(|ui| {
                if ui.button(t("我已切换武器", "I switched weapons")).clicked() {
                    app.wizard.on_switch(process.clone());
                }
                if ui.button(t("取消", "Cancel")).clicked() {
                    app.wizard.cancel();
                }
            });
        }
        Step::WaitStill(_) => {
            ui.colored_label(Color32::YELLOW, t("➜ 保持不动，不要切换武器（3 秒）…", "➜ Hold still, don't switch weapons (3 s)…"));
        }
    }
    ui.label(RichText::new(t("识别后请在上方武器列表选中你当前手持的武器；遇到新武器 ID 时会提示你“记住”一次。",
        "After detection, select the weapon you are holding in the list above; each new weapon ID asks to be remembered once.")).small());
}
