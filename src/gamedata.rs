//! The game's own weapon & ammunition ballistics (extracted from game files
//! by tools/gen_ballistics.py) and the model that turns them into a projectile.

use crate::ballistics::Projectile;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

#[derive(Deserialize, Debug, Clone)]
pub struct Zero {
    pub distance: f64,
    /// launch angle above the line of sight, degrees
    pub angle: f64,
    pub h_angle: f64,
    /// per-ammo angle overrides (degrees)
    pub overrides: BTreeMap<String, f64>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Tuning {
    pub ke: f64,
    pub gravity: f64,
    pub gravity_on: i64,
    pub zeros: Vec<Zero>,
    pub scatter_min: f64,
    pub scatter_max: f64,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Weapon {
    pub display: String,
    pub tuning: String,
    pub ammo: Vec<String>,
    pub hash: u32,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Ammo {
    pub display: String,
    pub mass: f64,
    pub diameter: f64,
    pub cd: f64,
    pub ke: f64,
    pub max_range: f64,
    pub velocity_deviance: f64,
    pub scatter_multiplier: f64,
    pub projectiles: i64,
    pub hash: u32,
}

#[derive(Deserialize, Debug)]
pub struct GameData {
    pub source: String,
    pub weapons: BTreeMap<String, Weapon>,
    pub ammo: BTreeMap<String, Ammo>,
    pub tunings: BTreeMap<String, Tuning>,
    #[serde(skip)]
    pub by_hash: HashMap<u32, String>,
}

static GD: OnceLock<GameData> = OnceLock::new();

pub fn gd() -> &'static GameData {
    GD.get_or_init(|| {
        let mut g: GameData = serde_json::from_str(include_str!("../data/ballistics.json")).expect("embedded ballistics");
        for (k, w) in &g.weapons {
            g.by_hash.insert(w.hash, k.clone());
        }
        for (k, a) in &g.ammo {
            g.by_hash.insert(a.hash, k.clone());
        }
        g
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Bow,
    Gun,
}

pub fn kind(weapon: &str) -> Kind {
    let w = weapon.to_ascii_lowercase();
    if w.contains("bow") { Kind::Bow } else { Kind::Gun }
}

/// Engine-wide constants not stored in the data files. Defaults come from
/// fitting the model to every zero angle in the game's weapon tunings
/// (bows: 1.3 mrad RMS; guns: 0.35 mrad median). One calibration refines them
/// for all weapons at once.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Engine {
    /// launch point below the eye (m)
    pub launch_height: f64,
    /// multiplier on 0.5*rho*Cd*A/m
    pub drag_mul: f64,
    /// v0 = eff * sqrt(2 KE / m)
    pub vel_eff_bow: f64,
    pub vel_eff_gun: f64,
    pub wind_factor: f64,
    pub air_density: f64,
}

impl Default for Engine {
    fn default() -> Self {
        Engine { launch_height: 0.065, drag_mul: 1.9, vel_eff_bow: 0.972, vel_eff_gun: 1.10, wind_factor: 1.0, air_density: 1.225 }
    }
}

pub struct Setup {
    pub proj: Projectile,
    /// bore angle relative to the line of sight (radians)
    pub bore: f64,
    pub bore_h: f64,
    pub zero_distance: f64,
    pub v0: f64,
    pub ke: f64,
}

/// Build the projectile + bore angle the game uses for weapon/ammo/zero index.
pub fn setup(weapon: &str, ammo: &str, zero_index: usize, e: &Engine) -> Option<Setup> {
    let g = gd();
    let w = g.weapons.get(weapon)?;
    let a = g.ammo.get(ammo)?;
    let t = g.tunings.get(&w.tuning)?;
    let z = t.zeros.get(zero_index).or_else(|| t.zeros.first())?;
    let ke = if t.ke > 0.0 { t.ke } else { a.ke };
    if ke <= 0.0 || a.mass <= 0.0 {
        return None;
    }
    let eff = if kind(weapon) == Kind::Bow { e.vel_eff_bow } else { e.vel_eff_gun };
    let v0 = eff * (2.0 * ke / a.mass).sqrt();
    let area = std::f64::consts::PI * (a.diameter / 2.0).powi(2);
    let k = e.drag_mul * 0.5 * e.air_density * a.cd * area / a.mass;
    let angle = z.overrides.get(ammo).copied().unwrap_or(z.angle);
    Some(Setup {
        proj: Projectile {
            muzzle_velocity: v0,
            drag_k: k,
            wind_factor: e.wind_factor,
            sight_height: e.launch_height,
            gravity: if t.gravity_on != 0 { t.gravity } else { 0.0 },
        },
        bore: angle.to_radians(),
        bore_h: z.h_angle.to_radians(),
        zero_distance: z.distance,
        v0,
        ke,
    })
}

/// Calibrate the engine constants from shots taken with ANY weapons.
/// Fits launch height + drag always; velocity efficiency per weapon kind
/// present; wind factor if any shot had wind. Returns (engine, rms mrad, max mrad).
pub fn fit_engine(e0: &Engine, shots: &[crate::ballistics::CalShot]) -> Option<(Engine, f64, f64, Vec<&'static str>)> {
    use crate::ballistics::{solve_bore, Shot};
    if shots.is_empty() {
        return None;
    }
    let has_bow = shots.iter().any(|s| kind(&s.weapon) == Kind::Bow);
    let has_gun = shots.iter().any(|s| kind(&s.weapon) == Kind::Gun);
    let has_wind = shots.iter().any(|s| s.crosswind.abs() > 0.1 || s.headwind.abs() > 0.1);
    let mut names: Vec<&'static str> = vec!["launch_height", "drag_mul"];
    if has_bow { names.push("vel_eff_bow") }
    if has_gun { names.push("vel_eff_gun") }
    if has_wind { names.push("wind_factor") }
    let get = |e: &Engine, n: &str| match n {
        "launch_height" => e.launch_height, "drag_mul" => e.drag_mul, "vel_eff_bow" => e.vel_eff_bow,
        "vel_eff_gun" => e.vel_eff_gun, _ => e.wind_factor,
    };
    let make = |x: &[f64]| {
        let mut e = *e0;
        for (n, v) in names.iter().zip(x) {
            match *n {
                "launch_height" => e.launch_height = *v, "drag_mul" => e.drag_mul = *v, "vel_eff_bow" => e.vel_eff_bow = *v,
                "vel_eff_gun" => e.vel_eff_gun = *v, _ => e.wind_factor = *v,
            }
        }
        e
    };
    let bounds = |n: &str| match n {
        "launch_height" => (-0.3, 0.3), "drag_mul" => (0.0, 10.0), "wind_factor" => (0.0, 10.0), _ => (0.5, 1.5),
    };
    let x0: Vec<f64> = names.iter().map(|n| get(e0, n)).collect();
    let lo: Vec<f64> = names.iter().map(|n| bounds(n).0).collect();
    let hi: Vec<f64> = names.iter().map(|n| bounds(n).1).collect();
    let f = |x: &[f64]| -> Option<Vec<f64>> {
        let e = make(x);
        let mut r = Vec::with_capacity(shots.len() * 2);
        for s in shots {
            let st = setup(&s.weapon, &s.ammo, s.zero_index, &e)?;
            let sol = solve_bore(&st.proj, st.bore, st.bore_h, &Shot { distance: s.distance, incline_deg: s.incline_deg, headwind: s.headwind, crosswind: s.crosswind })?;
            r.push(sol.drop_mrad - s.drop_mrad);
            r.push(sol.drift_mrad - s.drift_mrad);
        }
        Some(r)
    };
    let (x, rms, max) = crate::ballistics::lm(&x0, &lo, &hi, &f)?;
    Some((make(&x), rms, max, names))
}

/// Look up a raw in-memory id: game equipment-name hash of a weapon or ammo.
pub fn by_hash(raw: u64) -> Option<&'static str> {
    gd().by_hash.get(&(raw as u32)).map(|s| s.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ballistics::{solve_bore, Shot};

    #[test]
    fn data_loads_and_bows_zero_close_to_their_distance() {
        let g = gd();
        assert!(g.weapons.len() > 100 && g.ammo.len() > 80);
        let e = Engine::default();
        let w = "equipment_weapon_compound_bow_70lb_01";
        for (zi, z) in g.tunings[&g.weapons[w].tuning].zeros.iter().enumerate() {
            let s = setup(w, "equipment_ammo_bh_bow_arrow_420gr_01", zi, &e).unwrap();
            let sol = solve_bore(&s.proj, s.bore, 0.0, &Shot { distance: z.distance, ..Default::default() }).unwrap();
            // the game's own zero angle lands within ~3 mrad of the reticle at the zero distance
            assert!(sol.drop_mrad.abs() < 3.5, "zero {} -> {} mrad", z.distance, sol.drop_mrad);
        }
    }

    #[test]
    fn every_weapon_has_a_working_setup() {
        let g = gd();
        let e = Engine::default();
        let mut ok = 0;
        for (wk, w) in &g.weapons {
            for a in &w.ammo {
                if let Some(s) = setup(wk, a, 0, &e) {
                    assert!(s.v0 > 20.0 && s.v0 < 2000.0, "{wk} {a} v0 {}", s.v0);
                    ok += 1;
                }
            }
        }
        assert!(ok > 200);
    }

    #[test]
    fn engine_calibration_recovers_constants() {
        use crate::ballistics::{solve_bore, wind_components, CalShot, Shot};
        let truth = Engine { launch_height: 0.09, drag_mul: 2.4, vel_eff_bow: 0.93, vel_eff_gun: 1.05, wind_factor: 0.6, air_density: 1.225 };
        let mut shots = vec![];
        for (w, a) in [("equipment_weapon_compound_bow_70lb_01", "equipment_ammo_bh_bow_arrow_420gr_01"),
                       ("equipment_weapon_crossbow_165lb_01", "equipment_ammo_bh_crossbow_arrow_600gr_01")] {
            for d in [10.0, 20.0, 30.0, 45.0, 60.0, 75.0] {
                for (sp, dir) in [(0.0, 0.0), (5.0, 270.0)] {
                    let (hw, cw) = wind_components(sp, dir);
                    let s = setup(w, a, 0, &truth).unwrap();
                    let sol = solve_bore(&s.proj, s.bore, s.bore_h, &Shot { distance: d, incline_deg: 0.0, headwind: hw, crosswind: cw }).unwrap();
                    shots.push(CalShot { weapon: w.into(), ammo: a.into(), distance: d, zero: 0.0, zero_index: 0, incline_deg: 0.0,
                        headwind: hw, crosswind: cw, drop_mrad: sol.drop_mrad, drift_mrad: sol.drift_mrad });
                }
            }
        }
        let (e, rms, _, names) = fit_engine(&Engine::default(), &shots).unwrap();
        assert!(names.contains(&"vel_eff_bow") && !names.contains(&"vel_eff_gun"));
        assert!(rms < 0.02, "rms {rms} {e:?}");
        assert!((e.launch_height - 0.09).abs() < 0.005 && (e.vel_eff_bow - 0.93).abs() < 0.01 && (e.wind_factor - 0.6).abs() < 0.03, "{e:?}");
    }

    #[test]
    fn hash_lookup() {
        let g = gd();
        let (k, w) = g.weapons.iter().next().unwrap();
        assert_eq!(by_hash(w.hash as u64), Some(k.as_str()));
    }
}
