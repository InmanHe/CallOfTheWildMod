"""
Point-mass projectile solver for bows / crossbows.

Coordinate frame (world, shooter at origin):
    x : horizontal, along the shooter's facing direction
    y : up
    z : horizontal, to the shooter's right

Model
-----
    a = g + drag
    drag = -k * |v_rel| * v_rel,      v_rel = v - wind_factor * wind

``k`` (1/m) lumps 0.5*rho*Cd*A/m. With k = 0 the model is pure vacuum
(parabolic), which some games use. Both ``k`` and ``wind_factor`` can be
fitted from in-game test shots (see calibration.py), so the model can be
matched to whatever the game actually does.

Zeroing
-------
The sight (eye) sits ``sight_height`` metres above the arrow's launch point.
The sight is "zeroed" at ``zero_distance``: on flat ground, with no wind,
the arrow crosses the line of sight exactly at that distance. This fixes
the bore elevation relative to the line of sight (``zero_angle``). Every
prediction then uses that fixed bore angle — exactly how a real (and the
in-game) sight works — so the game's own zero setting is honoured.
"""
from __future__ import annotations

import math
from dataclasses import dataclass, field
from functools import lru_cache

G = 9.80665


@dataclass(frozen=True)
class Projectile:
    """Ballistic parameters of one weapon + ammo combination."""
    muzzle_velocity: float          # m/s
    drag_k: float = 0.0             # 1/m  (0.5*rho*Cd*A/m)
    wind_factor: float = 1.0        # scales wind effect (fit from calibration)
    sight_height: float = 0.08      # m, sight/eye above launch point
    gravity: float = G


@dataclass(frozen=True)
class ShotConditions:
    distance: float                 # m, slant range along line of sight to target
    incline_deg: float = 0.0        # + uphill / - downhill
    headwind: float = 0.0           # m/s, + blowing toward the shooter (from the front)
    crosswind: float = 0.0          # m/s, + blowing from left to right


@dataclass
class Solution:
    drop_m: float                   # + means impact BELOW the line of sight
    drift_m: float                  # + means impact RIGHT of the line of sight
    time_of_flight: float           # s
    impact_velocity: float          # m/s
    drop_mrad: float = 0.0
    drift_mrad: float = 0.0
    trajectory: list = field(default_factory=list)  # optional (x, y, z) samples

    @property
    def hold_up_mrad(self) -> float:
        """How far to hold above the target (= drop)."""
        return self.drop_mrad

    @property
    def hold_left_mrad(self) -> float:
        return self.drift_mrad


# --------------------------------------------------------------------------- #
# integration
# --------------------------------------------------------------------------- #
def _accel(v, wind, k, g):
    rx, ry, rz = v[0] - wind[0], v[1] - wind[1], v[2] - wind[2]
    s = math.sqrt(rx * rx + ry * ry + rz * rz)
    return (-k * s * rx, -g - k * s * ry, -k * s * rz)


def _rk4_step(p, v, dt, wind, k, g):
    a1 = _accel(v, wind, k, g)
    v2 = (v[0] + 0.5 * dt * a1[0], v[1] + 0.5 * dt * a1[1], v[2] + 0.5 * dt * a1[2])
    a2 = _accel(v2, wind, k, g)
    v3 = (v[0] + 0.5 * dt * a2[0], v[1] + 0.5 * dt * a2[1], v[2] + 0.5 * dt * a2[2])
    a3 = _accel(v3, wind, k, g)
    v4 = (v[0] + dt * a3[0], v[1] + dt * a3[1], v[2] + dt * a3[2])
    a4 = _accel(v4, wind, k, g)
    np_ = (
        p[0] + dt / 6.0 * (v[0] + 2 * v2[0] + 2 * v3[0] + v4[0]),
        p[1] + dt / 6.0 * (v[1] + 2 * v2[1] + 2 * v3[1] + v4[1]),
        p[2] + dt / 6.0 * (v[2] + 2 * v2[2] + 2 * v3[2] + v4[2]),
    )
    nv = (
        v[0] + dt / 6.0 * (a1[0] + 2 * a2[0] + 2 * a3[0] + a4[0]),
        v[1] + dt / 6.0 * (a1[1] + 2 * a2[1] + 2 * a3[1] + a4[1]),
        v[2] + dt / 6.0 * (a1[2] + 2 * a2[2] + 2 * a3[2] + a4[2]),
    )
    return np_, nv


def _fly(proj: Projectile, cond: ShotConditions, bore_angle: float,
         dt: float = 0.0005, keep_path: bool = False):
    """
    Fly the projectile with the line of sight (LOS) aimed at the target.

    Returns (offset_up, offset_right, tof, speed, path) measured in the plane
    perpendicular to the LOS at the target's slant distance.
    """
    th = math.radians(cond.incline_deg)
    u = (math.cos(th), math.sin(th), 0.0)          # LOS direction
    n = (-math.sin(th), math.cos(th), 0.0)         # LOS "up"
    # launch point: sight_height below the eye, perpendicular to the LOS
    p = (-proj.sight_height * n[0], -proj.sight_height * n[1], 0.0)
    elev = th + bore_angle
    v0 = proj.muzzle_velocity
    v = (v0 * math.cos(elev), v0 * math.sin(elev), 0.0)
    # wind vector: headwind blows toward shooter (-x), crosswind left->right (+z)
    wf = proj.wind_factor
    wind = (-cond.headwind * wf, 0.0, cond.crosswind * wf)

    def along(pt):
        return pt[0] * u[0] + pt[1] * u[1]

    t = 0.0
    path = [p] if keep_path else []
    target = cond.distance
    max_t = 20.0
    prev_p, prev_v = p, v
    while t < max_t:
        np_, nv = _rk4_step(p, v, dt, wind, proj.drag_k, proj.gravity)
        t += dt
        if keep_path:
            path.append(np_)
        if along(np_) >= target:
            prev_p, prev_v = p, v
            p, v = np_, nv
            break
        p, v = np_, nv
    else:
        raise ValueError("projectile never reached target distance")

    # linear interpolation to the exact crossing of the target plane
    a0, a1 = along(prev_p), along(p)
    f = 0.0 if a1 == a0 else (target - a0) / (a1 - a0)
    hit = tuple(prev_p[i] + f * (p[i] - prev_p[i]) for i in range(3))
    vel = tuple(prev_v[i] + f * (v[i] - prev_v[i]) for i in range(3))
    tof = t - dt + f * dt
    up = hit[0] * n[0] + hit[1] * n[1]
    right = hit[2]
    speed = math.sqrt(sum(c * c for c in vel))
    return up, right, tof, speed, path


# --------------------------------------------------------------------------- #
# zeroing
# --------------------------------------------------------------------------- #
@lru_cache(maxsize=512)
def zero_angle(proj: Projectile, zero_distance: float) -> float:
    """
    Bore elevation relative to the LOS (radians) so that, on flat ground and
    in still air, the projectile crosses the LOS at ``zero_distance``.
    Secant method on the vertical miss.
    """
    cond = ShotConditions(distance=zero_distance)
    # initial guesses: vacuum approximation
    tof = zero_distance / proj.muzzle_velocity
    a0 = (proj.sight_height + 0.5 * proj.gravity * tof * tof) / zero_distance
    a1 = a0 * 1.05 + 1e-4
    f0 = _fly(proj, cond, a0)[0]
    f1 = _fly(proj, cond, a1)[0]
    for _ in range(40):
        if abs(f1) < 1e-7:
            return a1
        if f1 == f0:
            break
        a2 = a1 - f1 * (a1 - a0) / (f1 - f0)
        a0, f0 = a1, f1
        a1 = a2
        f1 = _fly(proj, cond, a1)[0]
    if abs(f1) > 1e-4:
        raise ValueError(f"zero solve did not converge (miss {f1:.5f} m)")
    return a1


# --------------------------------------------------------------------------- #
# public API
# --------------------------------------------------------------------------- #
def solve(proj: Projectile, zero_distance: float, cond: ShotConditions,
          keep_path: bool = False) -> Solution:
    """Predict where the projectile hits relative to the point of aim."""
    if cond.distance <= 0.5:
        return Solution(0.0, 0.0, 0.0, proj.muzzle_velocity)
    bore = zero_angle(proj, zero_distance)
    up, right, tof, speed, path = _fly(proj, cond, bore, keep_path=keep_path)
    drop = -up
    sol = Solution(drop_m=drop, drift_m=right, time_of_flight=tof,
                   impact_velocity=speed, trajectory=path)
    sol.drop_mrad = math.atan2(drop, cond.distance) * 1000.0
    sol.drift_mrad = math.atan2(right, cond.distance) * 1000.0
    return sol


def solve_hold(proj: Projectile, zero_distance: float, cond: ShotConditions,
               iterations: int = 2) -> Solution:
    """
    Like :func:`solve`, but ``cond.incline_deg`` is the angle to the TARGET
    (e.g. typed in manually). When you put the predicted-impact marker on the
    target, the line of sight (screen centre) sits above the target by the
    hold angle, so the LOS pitch is target incline + hold. Fixed-point
    iteration on that.

    If the incline comes from the live camera pitch (memory mode), the LOS
    pitch is already known: use :func:`solve` directly instead.
    """
    sol = solve(proj, zero_distance, cond)
    base_incline = cond.incline_deg
    for _ in range(iterations):
        eff = ShotConditions(
            distance=cond.distance,
            incline_deg=base_incline + math.degrees(sol.drop_mrad / 1000.0),
            headwind=cond.headwind,
            crosswind=cond.crosswind,
        )
        sol = solve(proj, zero_distance, eff)
    return sol


def wind_components(wind_speed: float, wind_from_rel_deg: float) -> tuple[float, float]:
    """
    Split a wind into (headwind, crosswind) for the shooter.

    ``wind_from_rel_deg`` is the direction the wind is COMING FROM, measured
    clockwise from the shooter's facing direction:
        0   -> wind in your face (headwind)
        90  -> wind from your right (blows right->left, crosswind < 0)
        180 -> tailwind
        270 -> wind from your left (blows left->right, crosswind > 0)
    """
    r = math.radians(wind_from_rel_deg)
    headwind = wind_speed * math.cos(r)
    crosswind = -wind_speed * math.sin(r)
    return headwind, crosswind


def relative_wind_dir(wind_from_abs_deg: float, facing_abs_deg: float) -> float:
    """Absolute compass wind direction -> direction relative to the shooter."""
    return (wind_from_abs_deg - facing_abs_deg) % 360.0
