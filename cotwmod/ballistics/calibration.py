"""
Fit projectile parameters to in-game test shots.

The game's internal arrow/bolt numbers aren't published, so instead of
guessing we measure: shoot at known distances (ideally a few with crosswind),
record where each arrow actually landed relative to the aim point, and fit
muzzle velocity, drag and wind factor with Levenberg-Marquardt.

A "shot" record (JSON-friendly dict):
    {
      "distance": 40.0,          # m (rangefinder)
      "zero_distance": 20.0,     # the in-game zero used for that shot
      "incline_deg": 0.0,
      "headwind": 0.0, "crosswind": 0.0,   # m/s
      "drop_mrad": 6.1,          # measured: + = below aim point
      "drift_mrad": 0.0          # measured: + = right of aim point
    }
The overlay's calibration mode records these for you (you drag the marker
onto where the arrow stuck and press the record key).
"""
from __future__ import annotations

from dataclasses import replace

import numpy as np

from .solver import Projectile, ShotConditions, solve, zero_angle

FITTABLE = ("muzzle_velocity", "drag_k", "wind_factor", "sight_height")
_LOWER = {"muzzle_velocity": 10.0, "drag_k": 0.0, "wind_factor": 0.0, "sight_height": 0.0}
_UPPER = {"muzzle_velocity": 400.0, "drag_k": 0.05, "wind_factor": 10.0, "sight_height": 0.5}


def _clamp(name, v):
    return float(min(max(v, _LOWER[name]), _UPPER[name]))


def _predict(proj: Projectile, shot: dict) -> tuple[float, float]:
    cond = ShotConditions(
        distance=float(shot["distance"]),
        incline_deg=float(shot.get("incline_deg", 0.0)),
        headwind=float(shot.get("headwind", 0.0)),
        crosswind=float(shot.get("crosswind", 0.0)),
    )
    s = solve(proj, float(shot["zero_distance"]), cond)
    return s.drop_mrad, s.drift_mrad


def residuals(proj: Projectile, shots: list[dict]) -> np.ndarray:
    r = []
    for sh in shots:
        d, w = _predict(proj, sh)
        r.append(d - float(sh["drop_mrad"]))
        if "drift_mrad" in sh and sh["drift_mrad"] is not None:
            r.append(w - float(sh["drift_mrad"]))
    return np.asarray(r, dtype=float)


def fit(initial: Projectile, shots: list[dict], params: tuple[str, ...] | None = None,
        max_iter: int = 60, verbose: bool = False) -> tuple[Projectile, dict]:
    """
    Levenberg-Marquardt fit. ``params`` defaults to whatever the data can
    support: velocity + drag always, wind_factor only if any shot had wind.
    Returns (fitted projectile, report).
    """
    if not shots:
        raise ValueError("no calibration shots")
    if params is None:
        params = ["muzzle_velocity"]
        dists = {round(float(s["distance"])) for s in shots}
        if len(dists) >= 3:
            params.append("drag_k")
        if any(abs(float(s.get("crosswind", 0))) > 0.1 or abs(float(s.get("headwind", 0))) > 0.1
               for s in shots):
            params.append("wind_factor")
        params = tuple(params)

    proj = initial
    x = np.array([getattr(proj, p) for p in params], dtype=float)
    scale = np.array([max(abs(v), 1e-3) for v in x])

    def make(xv):
        kw = {p: _clamp(p, xv[i]) for i, p in enumerate(params)}
        return replace(initial, **kw)

    def cost(xv):
        r = residuals(make(xv), shots)
        return r, float(r @ r)

    r, c = cost(x)
    lam = 1e-2
    for it in range(max_iter):
        # numerical Jacobian (forward differences, relative step)
        J = np.zeros((len(r), len(params)))
        for j in range(len(params)):
            h = scale[j] * 1e-4
            xp = x.copy()
            xp[j] += h
            J[:, j] = (cost(xp)[0] - r) / h
        A = J.T @ J
        g = J.T @ r
        improved = False
        for _ in range(12):
            step = np.linalg.solve(A + lam * np.diag(np.diag(A) + 1e-12), -g)
            xn = np.array([_clamp(p, x[i] + step[i]) for i, p in enumerate(params)])
            rn, cn = cost(xn)
            if cn < c:
                x, r, c = xn, rn, cn
                lam = max(lam / 3.0, 1e-9)
                improved = True
                break
            lam *= 4.0
        if verbose:
            print(f"iter {it}: cost={c:.6g} x={dict(zip(params, x))}")
        if not improved or np.linalg.norm(step) < 1e-9 * (1 + np.linalg.norm(x)):
            break

    fitted = make(x)
    zero_angle.cache_clear()
    rms = float(np.sqrt(c / max(len(r), 1)))
    report = {
        "params": {p: getattr(fitted, p) for p in params},
        "rms_mrad": rms,
        "max_abs_mrad": float(np.max(np.abs(r))) if len(r) else 0.0,
        "n_shots": len(shots),
    }
    return fitted, report
