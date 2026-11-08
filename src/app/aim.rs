//! Aim computation, hotkey actions and live weapon resolution as pure
//! functions, shared by the UI thread and the native overlay thread.

use super::hotkeys::Action;
use super::live::LiveValues;
use super::settings::Settings;
use cotwmod::ballistics::{self, relative_dir, wind_components, CalShot, Camera, Shot};
use cotwmod::gamedata::{self, gd, Kind};
use cotwmod::memory::binding::Field;
use std::collections::HashMap;

#[allow(dead_code)]
#[derive(Clone, Default)]
pub struct AimOut {
    pub dx: f64,
    pub dy: f64,
    pub drop_mrad: f64,
    pub drift_mrad: f64,
    pub tof: f64,
    pub distance: f64,
    pub incline: f64,
    pub wind_speed: f64,
    pub wind_rel: f64,
    pub live_dist: bool,
    pub live_pitch: bool,
    pub live_wind: bool,
    pub live_dir: bool,
    pub ladder: Vec<(f64, f64, f64)>,
}

#[derive(Clone, Copy, Default, PartialEq)]
pub struct Calib {
    pub mode: bool,
    pub marker: [f64; 2],
}

/// Camera for the current state. Resolution follows the game window when known.
pub fn camera(s: &Settings, live: &LiveValues) -> Camera {
    let mut c = s.camera;
    c.magnification = s.magnification();
    if s.auto_resolution {
        if let Some(w) = live.window {
            c.screen_w = w[2] as f64;
            c.screen_h = w[3] as f64;
        }
    }
    if s.use_live {
        if let Some(f) = live.values.get(&Field::CameraFov) {
            if *f > 1.0 && *f < 170.0 {
                c.fov_deg = *f;
                c.magnification = 1.0;
            }
        }
    }
    c
}

pub fn resolve_id(raw: u64, learned: &HashMap<String, String>) -> Option<String> {
    gamedata::by_hash(raw).or_else(|| learned.get(&format!("{raw:X}")).cloned())
}

/// Apply the held weapon / ammo / zero read from memory. Returns true if anything changed.
pub fn apply_live_weapon(s: &mut Settings, live: &LiveValues) -> bool {
    if !(s.use_live && s.auto_weapon) {
        return false;
    }
    let g = gd();
    let mut changed = false;
    if let Some(raw) = live.raw.get(&Field::WeaponId).copied() {
        if let Some(k) = resolve_id(raw, &s.weapon_ids) {
            if g.weapons.contains_key(&k) && k != s.weapon {
                s.set_weapon(&k);
                changed = true;
            }
        }
    }
    if let Some(raw) = live.raw.get(&Field::AmmoId).copied() {
        if let Some(k) = resolve_id(raw, &s.ammo_ids) {
            if g.weapons.get(&s.weapon).map(|w| w.ammo.contains(&k)).unwrap_or(false) && k != s.ammo {
                s.ammo = k;
                changed = true;
            }
        }
    }
    if let Some(z) = live.values.get(&Field::ZeroIndex).copied() {
        let n = g.weapons.get(&s.weapon).and_then(|w| g.tunings.get(&w.tuning)).map(|t| t.zeros.len()).unwrap_or(0);
        if z >= 0.0 && (z as usize) < n && z as usize != s.zero_index {
            s.zero_index = z as usize;
            changed = true;
        }
    }
    changed
}

pub fn compute(s: &Settings, live: &LiveValues) -> Option<AimOut> {
    let lv = |f: Field| if s.use_live { live.values.get(&f).copied().filter(|v| v.is_finite()) } else { None };
    let live_d = lv(Field::AimDistance).filter(|d| *d > 0.5 && *d < 1000.0);
    let distance = live_d.unwrap_or(s.distance);
    let pitch = lv(Field::CameraPitch);
    let live_ws = lv(Field::WindSpeed);
    let wind_speed = live_ws.unwrap_or(s.wind_levels.get(s.wind_level).copied().unwrap_or(0.0));
    let (wind_rel, live_dir) = match (lv(Field::WindFrom), lv(Field::CameraYaw)) {
        (Some(w), Some(y)) => (relative_dir(w, y), true),
        _ => (s.wind_rel, false),
    };
    let (hw, cw) = wind_components(wind_speed, wind_rel);
    let st = gamedata::setup(&s.weapon, &s.ammo, s.zero_index, &s.engine)?;
    let incline = pitch.unwrap_or(s.incline);
    let shot = Shot { distance, incline_deg: incline, headwind: hw, crosswind: cw };
    let sol = if pitch.is_some() {
        ballistics::solve_bore(&st.proj, st.bore, st.bore_h, &shot)?
    } else {
        ballistics::solve_bore_hold(&st.proj, st.bore, st.bore_h, &shot)?
    };
    let cam = camera(s, live);
    let (dx, dy) = cam.impact_px(sol.drop_mrad, sol.drift_mrad);
    let mut ladder = vec![];
    if s.show_ladder {
        let step = if gamedata::kind(&s.weapon) == Kind::Bow { 10.0 } else { 50.0 };
        for i in 1..=8 {
            let d = i as f64 * step;
            if let Some(l) = ballistics::solve_bore(&st.proj, st.bore, st.bore_h, &Shot { distance: d, ..shot }) {
                let (lx, ly) = cam.impact_px(l.drop_mrad, l.drift_mrad);
                ladder.push((d, lx + s.aim_ref[0], ly + s.aim_ref[1]));
            }
        }
    }
    Some(AimOut {
        dx: dx + s.aim_ref[0],
        dy: dy + s.aim_ref[1],
        drop_mrad: sol.drop_mrad,
        drift_mrad: sol.drift_mrad,
        tof: sol.tof,
        distance,
        incline,
        wind_speed,
        wind_rel,
        live_dist: live_d.is_some(),
        live_pitch: pitch.is_some(),
        live_wind: live_ws.is_some(),
        live_dir,
        ladder,
    })
}

/// Apply a hotkey action. Returns true if a calibration shot should be recorded.
pub fn apply_action(s: &mut Settings, c: &mut Calib, a: Action) -> bool {
    match a {
        Action::CalibMode => {
            c.mode = !c.mode;
            return false;
        }
        Action::CalibRecord => return true,
        Action::WeaponSwitched => return false,
        _ => {}
    }
    if c.mode {
        let nudge = match a {
            Action::WindCw => Some((1.0, 0.0)),
            Action::WindCcw => Some((-1.0, 0.0)),
            Action::WindUp => Some((0.0, -1.0)),
            Action::WindDown => Some((0.0, 1.0)),
            _ => None,
        };
        if let Some((x, y)) = nudge {
            c.marker[0] += x;
            c.marker[1] += y;
            return false;
        }
    }
    let g = gd();
    match a {
        Action::ToggleOverlay => s.overlay_on = !s.overlay_on,
        Action::NextWeapon => {
            if s.recent.len() > 1 {
                let i = s.recent.iter().position(|k| *k == s.weapon).map(|i| (i + 1) % s.recent.len()).unwrap_or(0);
                let k = s.recent[i].clone();
                s.weapon = k;
                s.ammo.clear();
                s.zero_index = 0;
            }
        }
        Action::NextAmmo => {
            if let Some(w) = g.weapons.get(&s.weapon) {
                let i = w.ammo.iter().position(|k| *k == s.ammo).map(|i| (i + 1) % w.ammo.len()).unwrap_or(0);
                s.ammo = w.ammo[i].clone();
            }
        }
        Action::NextZero => {
            let n = g.weapons.get(&s.weapon).and_then(|w| g.tunings.get(&w.tuning)).map(|t| t.zeros.len()).unwrap_or(1).max(1);
            s.zero_index = (s.zero_index + 1) % n;
        }
        Action::NextMag => s.mag_index = (s.mag_index + 1) % s.mags.len().max(1),
        Action::DistPlus1 => s.distance += 1.0,
        Action::DistMinus1 => s.distance -= 1.0,
        Action::DistPlus5 => s.distance += 5.0,
        Action::DistMinus5 => s.distance -= 5.0,
        Action::WindCw => s.wind_rel = (s.wind_rel + 45.0).rem_euclid(360.0),
        Action::WindCcw => s.wind_rel = (s.wind_rel - 45.0).rem_euclid(360.0),
        Action::WindUp => s.wind_level += 1,
        Action::WindDown => s.wind_level = s.wind_level.saturating_sub(1),
        _ => {}
    }
    s.fix();
    false
}

/// Calibration shot: the red box marks where the projectile actually hit.
pub fn make_shot(s: &Settings, live: &LiveValues, c: &Calib) -> Option<CalShot> {
    let a = compute(s, live)?;
    let cam = camera(s, live);
    let (drop, drift) = cam.px_to_mrad(c.marker[0] - s.aim_ref[0], c.marker[1] - s.aim_ref[1]);
    let (hw, cw) = wind_components(a.wind_speed, a.wind_rel);
    Some(CalShot {
        weapon: s.weapon.clone(),
        ammo: s.ammo.clone(),
        distance: a.distance,
        zero: s.zero_distance(),
        zero_index: s.zero_index,
        incline_deg: a.incline,
        headwind: hw,
        crosswind: cw,
        drop_mrad: drop,
        drift_mrad: drift,
    })
}
