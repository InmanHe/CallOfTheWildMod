//! Background thread: keeps the game attached, polls bound fields, and
//! (optionally) keeps writing the animal view distance.

use cotwmod::memory::binding::{Binding, Field};
use cotwmod::memory::{attach_game, Process};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Default, Clone)]
pub struct LiveValues {
    pub attached: bool,
    /// game client area on screen (x, y, w, h) and whether the game is in front
    pub window: Option<[i32; 4]>,
    pub foreground: bool,
    pub values: HashMap<Field, f64>,
    pub raw: HashMap<Field, u64>,
    pub view_values: Vec<Option<f64>>,
}

#[derive(Default)]
pub struct Shared {
    pub bindings: HashMap<Field, Vec<Binding>>,
    pub view_lock: Option<f64>,
    pub out: LiveValues,
    pub process: Option<Arc<dyn Process>>,
    pub stop: bool,
}

pub fn spawn(shared: Arc<Mutex<Shared>>) {
    std::thread::spawn(move || {
        let mut last_try = Instant::now() - Duration::from_secs(10);
        let mut last_write = Instant::now();
        loop {
            let (proc_, bindings, lock) = {
                let s = shared.lock().unwrap();
                if s.stop {
                    return;
                }
                (s.process.clone(), s.bindings.clone(), s.view_lock)
            };
            let proc_ = match proc_ {
                Some(p) if p.alive() => Some(p),
                _ => {
                    let mut got = None;
                    if last_try.elapsed() > Duration::from_secs(2) {
                        last_try = Instant::now();
                        got = attach_game();
                    }
                    let mut s = shared.lock().unwrap();
                    s.process = got.clone();
                    s.out.attached = got.is_some();
                    got
                }
            };
            if let Some(p) = proc_ {
                let mut out = LiveValues { attached: true, ..Default::default() };
                #[cfg(windows)]
                if let Some((_, rect, fg)) = cotwmod::memory::win::game_window(p.pid()) {
                    if rect[2] > 0 && rect[3] > 0 {
                        out.window = Some(rect);
                    }
                    out.foreground = fg;
                }
                for (f, bs) in &bindings {
                    if *f == Field::ViewDistance {
                        out.view_values = bs.iter().map(|b| b.read(p.as_ref())).collect();
                        continue;
                    }
                    if let Some(b) = bs.first() {
                        if let Some(v) = b.read(p.as_ref()) {
                            out.values.insert(*f, v);
                        }
                        if let Some(r) = b.read_raw(p.as_ref()) {
                            out.raw.insert(*f, r);
                        }
                    }
                }
                if let (Some(v), Some(bs)) = (lock, bindings.get(&Field::ViewDistance)) {
                    if last_write.elapsed() > Duration::from_millis(250) {
                        last_write = Instant::now();
                        for b in bs {
                            if b.read(p.as_ref()).map(|cur| (cur - v).abs() > 0.01).unwrap_or(false) {
                                b.write(p.as_ref(), v);
                            }
                        }
                    }
                }
                shared.lock().unwrap().out = out;
            }
            std::thread::sleep(Duration::from_millis(33));
        }
    });
}
