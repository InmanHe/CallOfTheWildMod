//! Guided setup against a fake process: every task must bind the right address.
use cotwmod::guide::{Guide, Step, Task};
use cotwmod::memory::binding::{Binding, Field, Loc};
use cotwmod::memory::{FakeProcess, Process};
use std::sync::Arc;
use std::time::Duration;

const MODULE: u64 = 0x1_4000_0000;
const HEAP: u64 = 0x2000_0000;
const OBJ: u64 = HEAP + 0x1000;
const WEAPON: u64 = OBJ + 0x44;
const ZERO: u64 = OBJ + 0x50;
const ZERO_FLAG: u64 = OBJ + 0x54; // decoy: "zero > 0"
const PITCH: u64 = OBJ + 0x60; // radians, negative = up
const PITCH_SIN: u64 = OBJ + 0x64; // decoy
const FOV: u64 = OBJ + 0x70; // vertical, degrees
const HFOV: u64 = OBJ + 0x74; // decoy: horizontal
const DIST: u64 = OBJ + 0x80;

struct World {
    p: Arc<FakeProcess>,
    seed: u32,
}

impl World {
    fn new() -> Self {
        let p = FakeProcess::new(vec![(MODULE, 0x2000, true, true), (HEAP, 0x10000, true, false)], (MODULE, 0x2000));
        let w = World { p: Arc::new(p), seed: 7 };
        w.p.poke(MODULE + 0x100, &OBJ.to_le_bytes());
        w.p.poke(WEAPON, &1294205923u32.to_le_bytes());
        w
    }
    fn f(&self, a: u64, v: f32) {
        self.p.poke(a, &v.to_le_bytes());
    }
    /// unrelated values that keep changing
    fn noise(&mut self) {
        for i in 0..200u64 {
            self.seed = self.seed.wrapping_mul(1664525).wrapping_add(1013904223);
            self.p.poke(HEAP + 0x4000 + i * 8, &self.seed.to_le_bytes());
            self.f(HEAP + 0x8000 + i * 8, (self.seed % 1000) as f32 / 7.0);
        }
    }
    fn pitch(&self, deg: f32) {
        self.f(PITCH, -deg.to_radians());
        self.f(PITCH_SIN, deg.to_radians().sin());
    }
    fn fov(&self, v: f32) {
        self.f(FOV, v);
        self.f(HFOV, v * 1.5);
    }
    fn zero(&self, z: u8) {
        self.p.poke(ZERO, &[z]);
        self.p.poke(ZERO_FLAG, &[(z > 0) as u8]);
    }
}

fn wait(g: &mut Guide, p: &Arc<dyn Process>, out: &mut Vec<(Field, Binding)>, w: &mut World) {
    for _ in 0..3000 {
        w.noise();
        if let Some(b) = g.tick(Some(p.clone())) {
            out.push(b);
        }
        match g.step {
            Step::WaitUser | Step::Done | Step::Failed(_) => return,
            _ => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    panic!("guide stuck in {:?}", g.step);
}

fn addr(p: &dyn Process, b: &Binding) -> u64 {
    assert!(matches!(b.loc, Loc::Pointer(_)), "expected a pointer chain, got {:?}", b.loc);
    b.address(p).unwrap()
}

#[test]
fn all_tasks_bind_correct_addresses() {
    let mut w = World::new();
    w.zero(1);
    w.pitch(3.0);
    w.fov(60.0);
    w.f(DIST, 10.0);
    let p: Arc<dyn Process> = w.p.clone();
    let mut g = Guide::default();
    g.still = Duration::from_millis(40);
    g.zero_levels = 3;
    let mut out = vec![];
    g.start(p.clone(), &Task::ALL);
    let weapons = [846411716u32, 1294205923];
    let mut steps = 0;
    loop {
        wait(&mut g, &p, &mut out, &mut w);
        if let Step::Failed(e) = &g.step {
            panic!("task {:?} failed: {e}", g.task);
        }
        if g.step == Step::Done {
            break;
        }
        steps += 1;
        assert!(steps < 60, "too many steps");
        w.noise();
        let (zh, _) = g.prompt().unwrap();
        match g.task {
            Task::Weapon => {
                w.p.poke(WEAPON, &weapons[(steps + 1) % 2].to_le_bytes());
                g.confirm(Some(p.clone()), None);
            }
            Task::Zero => {
                let cur = w.p.read_u64(ZERO).unwrap() as u8;
                w.zero(if zh.contains("最低") { 0 } else { cur + 1 });
                g.confirm(Some(p.clone()), None);
            }
            Task::Pitch => {
                w.pitch(if zh.contains("抬头") { 88.0 } else { -88.0 });
                g.confirm(Some(p.clone()), None);
            }
            Task::Fov => {
                w.fov(if zh.contains("按住瞄准") { 20.0 } else { 60.0 });
                g.confirm(Some(p.clone()), None);
            }
            Task::Distance => {
                let d = 50.0 + steps as f32 * 13.5;
                w.f(DIST, d);
                assert!(g.needs_value());
                g.confirm(Some(p.clone()), Some(d as f64 + 0.3));
            }
        }
    }
    for (f, b) in &out {
        eprintln!("{f:?}: {} -> {:X?}", b.describe(), b.address(p.as_ref()));
    }
    let get = |f: Field| out.iter().find(|(k, _)| *k == f).map(|(_, b)| b.clone()).unwrap_or_else(|| panic!("{f:?} not bound"));
    assert_eq!(addr(p.as_ref(), &get(Field::WeaponId)), WEAPON);
    assert_eq!(addr(p.as_ref(), &get(Field::ZeroIndex)), ZERO);
    let pb = get(Field::CameraPitch);
    assert_eq!(addr(p.as_ref(), &pb), PITCH);
    w.pitch(30.0);
    assert!((pb.read(p.as_ref()).unwrap() - 30.0).abs() < 1e-3, "pitch must read +30° looking up");
    let fb = get(Field::CameraFov);
    assert_eq!(addr(p.as_ref(), &fb), FOV);
    assert_eq!(addr(p.as_ref(), &get(Field::AimDistance)), DIST);
    w.zero(2);
    assert_eq!(get(Field::ZeroIndex).read(p.as_ref()).unwrap(), 2.0);
}
