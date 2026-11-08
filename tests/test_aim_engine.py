import json

import pytest

from cotwmod.config import CONFIG_DIR, WeaponDB
from cotwmod.game.aim import AimEngine


class FakeMemory:
    def __init__(self, values):
        self.values = values
        self.configured = True

    def read_all(self):
        return self.values


def settings(mode="manual"):
    s = json.loads((CONFIG_DIR / "settings.json").read_text("utf-8"))
    s["input"]["mode"] = mode
    return s


def test_manual_defaults_and_cycling():
    eng = AimEngine(WeaponDB(calibrated={}), settings())
    r = eng.compute()
    assert r.sources["aim_distance_m"] == "manual"
    z0 = eng.state.zero
    eng.action("next_zero")
    assert eng.state.zero != z0
    eng.action("distance_plus_5")
    assert eng.state.distance == pytest.approx(35.0)
    eng.action("next_weapon")
    assert eng.state.ammo in eng.db.ammo_keys(eng.state.weapon)
    assert eng.state.zero in [float(z) for z in eng.db.weapons[eng.state.weapon]["zero_options"]]


def test_marker_below_center_beyond_zero():
    eng = AimEngine(WeaponDB(calibrated={}), settings())
    eng.state.distance = 70
    assert eng.compute().dy > 0
    eng.state.distance = eng.state.zero
    assert abs(eng.compute().dy) < 0.5


def test_live_values_override_manual():
    mem = FakeMemory({"aim_distance_m": 55.0, "camera_pitch_deg": 10.0,
                      "wind_from_deg": 0.0, "camera_yaw_deg": 90.0, "wind_speed_ms": 4.0})
    eng = AimEngine(WeaponDB(calibrated={}), settings("memory"), mem)
    r = eng.compute()
    assert r.distance == 55.0 and r.sources["incline"] == "live"
    # wind from north, facing east -> wind from the left (270) -> drifts right
    assert r.wind_rel_deg == pytest.approx(270.0)
    assert r.dx > 0


def test_calibration_marker_roundtrip():
    eng = AimEngine(WeaponDB(calibrated={}), settings())
    eng.action("calib_marker_mode")
    for _ in range(10):
        eng.action("wind_level_down")  # moves marker down in calib mode
    shot = eng.calibration_shot()
    assert shot["drop_mrad"] > 0 and shot["drift_mrad"] == 0
