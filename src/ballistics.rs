//! Point-mass projectile model for bows / crossbows (RK4), zeroing, wind,
//! incline, screen projection and Levenberg-Marquardt calibration.
//!
//! Frame: x forward (horizontal), y up, z right. Drag a = -k|v_rel|v_rel,
//! v_rel = v - wind_factor*wind. The sight sits `sight_height` above the launch
//! point; the bore is angled so that on flat ground in still air the arrow
//! crosses the line of sight (LOS) at the zero distance — exactly how the
//! in-game sight zero works, so the game's own zero setting is honoured.

use serde::{Deserialize, Serialize};

pub const G: f64 = 9.80665;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Projectile {
    pub muzzle_velocity: f64,
    pub drag_k: f64,
    pub wind_factor: f64,
    pub sight_height: f64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Shot {
    pub distance: f64,
    pub incline_deg: f64,
    /// + = blowing toward the shooter
    pub headwind: f64,
    /// + = blowing left -> right
    pub crosswind: f64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Solution {
    pub drop_m: f64,
    pub drift_m: f64,
    pub tof: f64,
    pub speed: f64,
    pub drop_mrad: f64,
    pub drift_mrad: f64,
}

type V3 = [f64; 3];

#[inline]
fn accel(v: V3, w: V3, k: f64) -> V3 {
    let r = [v[0] - w[0], v[1] - w[1], v[2] - w[2]];
    let s = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt();
    [-k * s * r[0], -G - k * s * r[1], -k * s * r[2]]
}

#[inline]
fn add(a: V3, b: V3, s: f64) -> V3 {
    [a[0] + b[0] * s, a[1] + b[1] * s, a[2] + b[2] * s]
}

/// Returns (up offset, right offset, tof, speed) in the plane perpendicular
/// to the LOS at the target's slant distance.
fn fly(p: &Projectile, s: &Shot, bore: f64) -> Option<(f64, f64, f64, f64)> {
    let dt = 0.0005;
    let th = s.incline_deg.to_radians();
    let u = [th.cos(), th.sin(), 0.0];
    let n = [-th.sin(), th.cos(), 0.0];
    let mut pos = [-p.sight_height * n[0], -p.sight_height * n[1], 0.0];
    let e = th + bore;
    let mut vel = [p.muzzle_velocity * e.cos(), p.muzzle_velocity * e.sin(), 0.0];
    let w = [-s.headwind * p.wind_factor, 0.0, s.crosswind * p.wind_factor];
    let along = |q: V3| q[0] * u[0] + q[1] * u[1];
    let mut t = 0.0;
    while t < 20.0 {
        let a1 = accel(vel, w, p.drag_k);
        let v2 = add(vel, a1, dt / 2.0);
        let a2 = accel(v2, w, p.drag_k);
        let v3 = add(vel, a2, dt / 2.0);
        let a3 = accel(v3, w, p.drag_k);
        let v4 = add(vel, a3, dt);
        let a4 = accel(v4, w, p.drag_k);
        let mut np = pos;
        let mut nv = vel;
        for i in 0..3 {
            np[i] += dt / 6.0 * (vel[i] + 2.0 * v2[i] + 2.0 * v3[i] + v4[i]);
            nv[i] += dt / 6.0 * (a1[i] + 2.0 * a2[i] + 2.0 * a3[i] + a4[i]);
        }
        t += dt;
        if along(np) >= s.distance {
            let a0 = along(pos);
            let a1 = along(np);
            let f = if a1 == a0 { 0.0 } else { (s.distance - a0) / (a1 - a0) };
            let hit: V3 = std::array::from_fn(|i| pos[i] + f * (np[i] - pos[i]));
            let hv: V3 = std::array::from_fn(|i| vel[i] + f * (nv[i] - vel[i]));
            let up = hit[0] * n[0] + hit[1] * n[1];
            let speed = (hv[0] * hv[0] + hv[1] * hv[1] + hv[2] * hv[2]).sqrt();
            return Some((up, hit[2], t - dt + f * dt, speed));
        }
        pos = np;
        vel = nv;
    }
    None
}

/// Bore elevation (rad, relative to LOS) for the given zero distance.
pub fn zero_angle(p: &Projectile, zero: f64) -> Option<f64> {
    let s = Shot { distance: zero, ..Default::default() };
    let tof = zero / p.muzzle_velocity;
    let mut a0 = (p.sight_height + 0.5 * G * tof * tof) / zero;
    let mut a1 = a0 * 1.05 + 1e-4;
    let mut f0 = fly(p, &s, a0)?.0;
    let mut f1 = fly(p, &s, a1)?.0;
    for _ in 0..40 {
        if f1.abs() < 1e-7 {
            return Some(a1);
        }
        if f1 == f0 {
            break;
        }
        let a2 = a1 - f1 * (a1 - a0) / (f1 - f0);
        a0 = a1;
        f0 = f1;
        a1 = a2;
        f1 = fly(p, &s, a1)?.0;
    }
    (f1.abs() < 1e-4).then_some(a1)
}

pub fn solve(p: &Projectile, zero: f64, s: &Shot) -> Option<Solution> {
    if s.distance <= 0.5 {
        return Some(Solution { speed: p.muzzle_velocity, ..Default::default() });
    }
    let bore = zero_angle(p, zero)?;
    let (up, right, tof, speed) = fly(p, s, bore)?;
    let drop = -up;
    Some(Solution {
        drop_m: drop,
        drift_m: right,
        tof,
        speed,
        drop_mrad: drop.atan2(s.distance) * 1000.0,
        drift_mrad: right.atan2(s.distance) * 1000.0,
    })
}

/// `s.incline_deg` is the angle to the TARGET (manual input). With the impact
/// marker on the target, the LOS sits above it by the hold, so iterate.
/// If the incline is the live camera pitch, use [`solve`] directly.
pub fn solve_hold(p: &Projectile, zero: f64, s: &Shot) -> Option<Solution> {
    let mut sol = solve(p, zero, s)?;
    for _ in 0..2 {
        let eff = Shot { incline_deg: s.incline_deg + (sol.drop_mrad / 1000.0).to_degrees(), ..*s };
        sol = solve(p, zero, &eff)?;
    }
    Some(sol)
}

/// Wind FROM direction relative to facing (0 = in your face, 90 = from the right).
pub fn wind_components(speed: f64, from_rel_deg: f64) -> (f64, f64) {
    let r = from_rel_deg.to_radians();
    (speed * r.cos(), -speed * r.sin())
}

pub fn relative_dir(from_abs: f64, facing_abs: f64) -> f64 {
    (from_abs - facing_abs).rem_euclid(360.0)
}

// ------------------------------------------------------------------ camera

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Camera {
    pub screen_w: f64,
    pub screen_h: f64,
    pub fov_deg: f64,
    pub fov_horizontal: bool,
    pub magnification: f64,
    /// from calibration; overrides FOV math when > 0
    pub px_per_mrad: f64,
}

impl Camera {
    pub fn focal_px(&self) -> f64 {
        if self.px_per_mrad > 0.0 {
            return self.px_per_mrad * 1000.0;
        }
        let half = self.fov_deg.to_radians() / 2.0;
        let span = if self.fov_horizontal { self.screen_w } else { self.screen_h };
        (span / 2.0) / half.tan() * self.magnification
    }
    /// Screen offset (dx, dy) of the impact point; y grows downward.
    pub fn impact_px(&self, drop_mrad: f64, drift_mrad: f64) -> (f64, f64) {
        let f = self.focal_px();
        (f * (drift_mrad / 1000.0).tan(), f * (drop_mrad / 1000.0).tan())
    }
    pub fn px_to_mrad(&self, dx: f64, dy: f64) -> (f64, f64) {
        let f = self.focal_px();
        ((dy / f).atan() * 1000.0, (dx / f).atan() * 1000.0)
    }
}

// ------------------------------------------------------------- calibration

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalShot {
    pub weapon: String,
    pub ammo: String,
    pub distance: f64,
    pub zero: f64,
    pub incline_deg: f64,
    pub headwind: f64,
    pub crosswind: f64,
    pub drop_mrad: f64,
    pub drift_mrad: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FitReport {
    pub rms_mrad: f64,
    pub max_mrad: f64,
    pub shots: usize,
    pub params: Vec<String>,
}

fn residuals(p: &Projectile, shots: &[CalShot]) -> Option<Vec<f64>> {
    let mut r = Vec::with_capacity(shots.len() * 2);
    for s in shots {
        let sol = solve(p, s.zero, &Shot { distance: s.distance, incline_deg: s.incline_deg, headwind: s.headwind, crosswind: s.crosswind })?;
        r.push(sol.drop_mrad - s.drop_mrad);
        r.push(sol.drift_mrad - s.drift_mrad);
    }
    Some(r)
}

const LO: [f64; 3] = [10.0, 0.0, 0.0];
const HI: [f64; 3] = [400.0, 0.05, 10.0];

fn get(p: &Projectile, i: usize) -> f64 {
    [p.muzzle_velocity, p.drag_k, p.wind_factor][i]
}
fn set(p: &mut Projectile, i: usize, v: f64) {
    let v = v.clamp(LO[i], HI[i]);
    match i {
        0 => p.muzzle_velocity = v,
        1 => p.drag_k = v,
        _ => p.wind_factor = v,
    }
}

/// Solve a small dense linear system (Gaussian elimination, partial pivoting).
fn lin_solve(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    for c in 0..n {
        let piv = (c..n).max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()))?;
        if a[piv][c].abs() < 1e-300 {
            return None;
        }
        a.swap(c, piv);
        b.swap(c, piv);
        for r in c + 1..n {
            let f = a[r][c] / a[c][c];
            for k in c..n {
                a[r][k] -= f * a[c][k];
            }
            b[r] -= f * b[c];
        }
    }
    let mut x = vec![0.0; n];
    for r in (0..n).rev() {
        let s: f64 = (r + 1..n).map(|k| a[r][k] * x[k]).sum();
        x[r] = (b[r] - s) / a[r][r];
    }
    Some(x)
}

/// Fit velocity (+ drag with >= 3 distances, + wind factor if any shot had wind).
pub fn fit(initial: &Projectile, shots: &[CalShot]) -> Option<(Projectile, FitReport)> {
    if shots.is_empty() {
        return None;
    }
    let mut idx = vec![0usize];
    let mut dists: Vec<i64> = shots.iter().map(|s| s.distance.round() as i64).collect();
    dists.sort();
    dists.dedup();
    if dists.len() >= 3 {
        idx.push(1);
    }
    if shots.iter().any(|s| s.crosswind.abs() > 0.1 || s.headwind.abs() > 0.1) {
        idx.push(2);
    }
    let names = ["muzzle_velocity", "drag_k", "wind_factor"];
    let mut p = *initial;
    let cost = |p: &Projectile| residuals(p, shots).map(|r| { let c: f64 = r.iter().map(|x| x * x).sum(); (r, c) });
    let (mut r, mut c) = cost(&p)?;
    let mut lam = 1e-2;
    for _ in 0..60 {
        let m = idx.len();
        let mut jac = vec![vec![0.0; m]; r.len()];
        for (j, &pi) in idx.iter().enumerate() {
            let h = get(&p, pi).abs().max(1e-3) * 1e-4;
            let mut q = p;
            set(&mut q, pi, get(&p, pi) + h);
            let (rq, _) = cost(&q)?;
            for (row, (a, b)) in jac.iter_mut().zip(rq.iter().zip(&r)) {
                row[j] = (a - b) / h;
            }
        }
        let mut a = vec![vec![0.0; m]; m];
        let mut g = vec![0.0; m];
        for (row, ri) in jac.iter().zip(&r) {
            for i in 0..m {
                g[i] += row[i] * ri;
                for k in 0..m {
                    a[i][k] += row[i] * row[k];
                }
            }
        }
        let mut improved = false;
        let mut step_norm = 0.0;
        for _ in 0..12 {
            let mut aa = a.clone();
            for i in 0..m {
                aa[i][i] += lam * (a[i][i] + 1e-12);
            }
            let step = lin_solve(aa, g.iter().map(|x| -x).collect())?;
            let mut q = p;
            for (j, &pi) in idx.iter().enumerate() {
                set(&mut q, pi, get(&p, pi) + step[j]);
            }
            if let Some((rq, cq)) = cost(&q) {
                if cq < c {
                    p = q;
                    r = rq;
                    c = cq;
                    lam = (lam / 3.0).max(1e-9);
                    improved = true;
                    step_norm = step.iter().map(|x| x * x).sum::<f64>().sqrt();
                    break;
                }
            }
            lam *= 4.0;
        }
        if !improved || step_norm < 1e-10 {
            break;
        }
    }
    let rms = (c / r.len().max(1) as f64).sqrt();
    let max = r.iter().fold(0.0f64, |m, x| m.max(x.abs()));
    Some((p, FitReport { rms_mrad: rms, max_mrad: max, shots: shots.len(), params: idx.iter().map(|&i| names[i].to_string()).collect() }))
}

#[cfg(test)]
mod tests {
    use super::*;
    const ARROW: Projectile = Projectile { muzzle_velocity: 80.0, drag_k: 0.0008, wind_factor: 1.0, sight_height: 0.08 };

    #[test]
    fn zero_has_no_drop() {
        for z in [10.0, 20.0, 30.0, 40.0, 50.0, 60.0] {
            let s = solve(&ARROW, z, &Shot { distance: z, ..Default::default() }).unwrap();
            assert!(s.drop_m.abs() < 1e-5, "{z} {}", s.drop_m);
        }
    }

    #[test]
    fn matches_python_reference() {
        // values printed by the Python version: `cotwmod table` hawkedge 420gr zero 40, wind 4 from left
        let p = Projectile { muzzle_velocity: 81.0, drag_k: 0.0008, wind_factor: 1.0, sight_height: 0.08 };
        let (hw, cw) = wind_components(4.0, 270.0);
        let s = solve(&p, 40.0, &Shot { distance: 70.0, incline_deg: 0.0, headwind: hw, crosswind: cw }).unwrap();
        assert!((s.drop_m * 100.0 - 160.7).abs() < 0.1, "{}", s.drop_m);
        assert!((s.drift_m * 100.0 - 9.9).abs() < 0.1, "{}", s.drift_m);
        let s = solve(&p, 40.0, &Shot { distance: 10.0, incline_deg: 0.0, headwind: hw, crosswind: cw }).unwrap();
        assert!((s.drop_m * 100.0 + 17.1).abs() < 0.1);
    }

    #[test]
    fn wind_and_incline_directions() {
        let (hw, cw) = wind_components(5.0, 270.0);
        assert!(solve(&ARROW, 20.0, &Shot { distance: 50.0, headwind: hw, crosswind: cw, ..Default::default() }).unwrap().drift_m > 0.0);
        let flat = solve(&ARROW, 20.0, &Shot { distance: 60.0, ..Default::default() }).unwrap().drop_m;
        let up = solve(&ARROW, 20.0, &Shot { distance: 60.0, incline_deg: 30.0, ..Default::default() }).unwrap().drop_m;
        let head = solve(&ARROW, 20.0, &Shot { distance: 60.0, headwind: 6.0, ..Default::default() }).unwrap().drop_m;
        assert!(up < flat && head > flat);
        assert!(solve_hold(&ARROW, 20.0, &Shot { distance: 70.0, incline_deg: 25.0, ..Default::default() }).is_some());
    }

    #[test]
    fn camera() {
        let c = Camera { screen_w: 1920.0, screen_h: 1080.0, fov_deg: 90.0, fov_horizontal: true, magnification: 1.0, px_per_mrad: 0.0 };
        assert!((c.focal_px() - 960.0).abs() < 1e-9);
        let (dx, dy) = c.impact_px(10.0, 5.0);
        assert!(dx > 0.0 && dy > 0.0);
        let (d, w) = c.px_to_mrad(dx, dy);
        assert!((d - 10.0).abs() < 1e-9 && (w - 5.0).abs() < 1e-9);
    }

    #[test]
    fn calibration_recovers_truth() {
        let truth = Projectile { muzzle_velocity: 73.0, drag_k: 0.0015, wind_factor: 0.7, sight_height: 0.08 };
        let mut shots = vec![];
        for d in [10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0] {
            for (hw, cw) in [(0.0, 0.0), (0.0, 4.0), (3.0, -2.0)] {
                let s = solve(&truth, 30.0, &Shot { distance: d, incline_deg: 0.0, headwind: hw, crosswind: cw }).unwrap();
                shots.push(CalShot { weapon: "w".into(), ammo: "a".into(), distance: d, zero: 30.0, incline_deg: 0.0,
                    headwind: hw, crosswind: cw, drop_mrad: s.drop_mrad, drift_mrad: s.drift_mrad });
            }
        }
        let guess = Projectile { muzzle_velocity: 90.0, drag_k: 0.0005, wind_factor: 1.0, sight_height: 0.08 };
        let (f, rep) = fit(&guess, &shots).unwrap();
        assert!((f.muzzle_velocity - 73.0).abs() / 73.0 < 0.01, "{f:?}");
        assert!((f.drag_k - 0.0015).abs() / 0.0015 < 0.05, "{f:?}");
        assert!((f.wind_factor - 0.7).abs() < 0.02, "{f:?}");
        assert!(rep.rms_mrad < 0.01);
    }
}
