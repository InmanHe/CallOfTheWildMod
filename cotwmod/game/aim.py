"""
Aim engine: merges manual + live inputs, runs the solver, returns what the
overlay should draw.
"""
from __future__ import annotations

import math
import threading
from dataclasses import dataclass, field

from ..ballistics import (Camera, ShotConditions, impact_offset_px, relative_wind_dir,
                          solve, solve_hold, wind_components)
from ..config import WeaponDB


_CALIB_NUDGE = {
    "wind_dir_cw": (1, 0), "wind_dir_ccw": (-1, 0),
    "wind_level_up": (0, -1), "wind_level_down": (0, 1),
}


@dataclass
class AimState:
    weapon: str
    ammo: str
    zero: float
    distance: float = 30.0
    incline_deg: float = 0.0
    wind_rel_deg: float = 0.0      # wind FROM, relative to facing (0 = in your face)
    wind_level: int = 0
    mag_index: int = 0
    visible: bool = True
    calib_marker: list = field(default_factory=lambda: [0.0, 0.0])  # px offset for calibration
    calib_mode: bool = False


@dataclass
class AimResult:
    dx: float
    dy: float
    drop_mrad: float
    drift_mrad: float
    tof: float
    distance: float
    incline_deg: float
    wind_speed: float
    wind_rel_deg: float
    headwind: float
    crosswind: float
    zero: float
    weapon_name: str
    ammo_name: str
    calibrated: bool
    sources: dict
    ladder: list                   # [(distance, dx, dy)]
    magnification: float


class AimEngine:
    def __init__(self, db: WeaponDB, settings: dict, memory=None):
        self.db = db
        self.settings = settings
        self.memory = memory
        st = settings["startup"]
        self.lock = threading.RLock()
        self.state = AimState(weapon=st["weapon"], ammo=st["ammo"], zero=float(st["zero"]),
                              distance=float(st.get("distance", 30.0)),
                              mag_index=int(st.get("magnification_index", 0)))
        self._fix_state()
        self.use_memory = settings["input"].get("mode", "memory") == "memory" and memory is not None

    # ---------------- state mutation (hotkeys) ----------------
    def _fix_state(self):
        s = self.state
        if s.weapon not in self.db.weapons:
            s.weapon = self.db.weapon_keys()[0]
        if s.ammo not in self.db.ammo_keys(s.weapon):
            s.ammo = self.db.ammo_keys(s.weapon)[0]
        w = self.db.weapons[s.weapon]
        if s.zero not in w["zero_options"]:
            s.zero = float(w.get("default_zero", w["zero_options"][0]))
        s.mag_index %= len(w.get("magnification", [1.0]))
        levels = self.settings["input"]["wind_levels_ms"]
        s.wind_level = max(0, min(len(levels) - 1, s.wind_level))
        s.distance = max(1.0, min(300.0, s.distance))

    def _cycle(self, items, cur):
        items = list(items)
        i = items.index(cur) if cur in items else -1
        return items[(i + 1) % len(items)]

    def action(self, name: str):
        with self.lock:
            s = self.state
            w = self.db.weapons[s.weapon]
            if s.calib_mode and name in _CALIB_NUDGE:
                # in calibration mode the arrow keys move the "where it hit" marker
                ddx, ddy = _CALIB_NUDGE[name]
                s.calib_marker[0] += ddx
                s.calib_marker[1] += ddy
                return
            if name == "toggle_overlay":
                s.visible = not s.visible
            elif name == "next_weapon":
                s.weapon = self._cycle(self.db.weapon_keys(), s.weapon)
                s.ammo = ""
                s.zero = -1
            elif name == "next_ammo":
                s.ammo = self._cycle(self.db.ammo_keys(s.weapon), s.ammo)
            elif name == "next_zero":
                s.zero = float(self._cycle([float(z) for z in w["zero_options"]], s.zero))
            elif name == "next_magnification":
                s.mag_index += 1
            elif name.startswith("distance_"):
                sign = 1 if "plus" in name else -1
                s.distance += sign * float(name.rsplit("_", 1)[1])
            elif name == "wind_dir_cw":
                s.wind_rel_deg = (s.wind_rel_deg + 45) % 360
            elif name == "wind_dir_ccw":
                s.wind_rel_deg = (s.wind_rel_deg - 45) % 360
            elif name == "wind_level_up":
                s.wind_level += 1
            elif name == "wind_level_down":
                s.wind_level -= 1
            elif name == "calib_marker_mode":
                s.calib_mode = not s.calib_mode
            self._fix_state()

    # ---------------- solving ----------------
    def camera(self, live_fov=None) -> Camera:
        sc = self.settings["screen"]
        w = self.db.weapons[self.state.weapon]
        mags = w.get("magnification", [1.0])
        mag = float(mags[self.state.mag_index % len(mags)])
        fov = float(sc["fov_deg"])
        if live_fov:            # live camera FOV already includes zoom
            fov, mag = float(live_fov), 1.0
        return Camera(int(sc["width"]), int(sc["height"]), fov,
                      bool(sc.get("fov_is_horizontal", False)), mag,
                      sc.get("px_per_mrad_override"))

    def compute(self) -> AimResult:
        with self.lock:
            s = AimState(**{**self.state.__dict__})
        live = self.memory.read_all() if self.use_memory else {}
        sources = {}

        def pick(key, manual):
            v = live.get(key)
            if v is not None and math.isfinite(v):
                sources[key] = "live"
                return v
            sources[key] = "manual"
            return manual

        distance = pick("aim_distance_m", s.distance)
        if not (0.5 < distance < 1000):
            distance, sources["aim_distance_m"] = s.distance, "manual"
        pitch = live.get("camera_pitch_deg")
        levels = self.settings["input"]["wind_levels_ms"]
        wind_speed = pick("wind_speed_ms", float(levels[s.wind_level]))
        wfrom, yaw = live.get("wind_from_deg"), live.get("camera_yaw_deg")
        if wfrom is not None and yaw is not None:
            wind_rel = relative_wind_dir(wfrom, yaw)
            sources["wind_dir"] = "live"
        else:
            wind_rel = s.wind_rel_deg
            sources["wind_dir"] = "manual"
        hw, cw = wind_components(wind_speed, wind_rel)

        proj = self.db.projectile(s.weapon, s.ammo)
        cam = self.camera(live.get("camera_fov_deg"))
        if pitch is not None:
            # live LOS pitch: already includes the hold, solve directly
            sources["incline"] = "live"
            incline = pitch
            sol = solve(proj, s.zero, ShotConditions(distance, pitch, hw, cw))
        else:
            sources["incline"] = "manual"
            incline = s.incline_deg
            sol = solve_hold(proj, s.zero, ShotConditions(distance, incline, hw, cw))

        ref = self.settings["screen"].get("aim_reference_px", [0, 0])
        dx, dy = impact_offset_px(cam, sol.drop_mrad, sol.drift_mrad)
        ladder = []
        if self.settings["overlay"].get("show_trajectory_ladder"):
            for d in self.settings["overlay"].get("ladder_distances", []):
                ls = solve(proj, s.zero, ShotConditions(float(d), incline, hw, cw))
                lx, ly = impact_offset_px(cam, ls.drop_mrad, ls.drift_mrad)
                ladder.append((d, lx + ref[0], ly + ref[1]))

        w = self.db.weapons[s.weapon]
        return AimResult(
            dx=dx + ref[0], dy=dy + ref[1], drop_mrad=sol.drop_mrad, drift_mrad=sol.drift_mrad,
            tof=sol.time_of_flight, distance=distance, incline_deg=incline,
            wind_speed=wind_speed, wind_rel_deg=wind_rel, headwind=hw, crosswind=cw,
            zero=s.zero, weapon_name=w["name"], ammo_name=w["ammo"][s.ammo]["name"],
            calibrated=self.db.is_calibrated(s.weapon, s.ammo), sources=sources,
            ladder=ladder, magnification=cam.magnification,
        )

    # ---------------- calibration helpers ----------------
    def calibration_shot(self) -> dict:
        """Record a shot: the calibration marker sits where the arrow actually hit."""
        r = self.compute()
        f = self.camera().focal_px()
        ref = self.settings["screen"].get("aim_reference_px", [0, 0])
        mx, my = self.state.calib_marker
        return {
            "weapon": self.state.weapon, "ammo": self.state.ammo,
            "distance": r.distance, "zero_distance": r.zero, "incline_deg": r.incline_deg,
            "headwind": r.headwind, "crosswind": r.crosswind,
            # screen y grows downward: marker below reference = positive drop
            "drop_mrad": math.atan((my - ref[1]) / f) * 1000.0,
            "drift_mrad": math.atan((mx - ref[0]) / f) * 1000.0,
        }
