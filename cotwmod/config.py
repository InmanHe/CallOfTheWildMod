"""Config file loading. Everything lives in <repo>/config unless COTWMOD_CONFIG is set."""
from __future__ import annotations

import json
import os
from pathlib import Path

from .ballistics import Projectile

ROOT = Path(__file__).resolve().parent.parent
CONFIG_DIR = Path(os.environ.get("COTWMOD_CONFIG", ROOT / "config"))


def load_json(name: str) -> dict:
    with open(CONFIG_DIR / name, encoding="utf-8") as f:
        return json.load(f)


def save_json(name: str, data: dict) -> None:
    path = CONFIG_DIR / name
    tmp = path.with_suffix(".tmp")
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=2, ensure_ascii=False)
    tmp.replace(path)


def load_calibrated() -> dict:
    p = CONFIG_DIR / "calibrated.json"
    if not p.exists():
        return {}
    with open(p, encoding="utf-8") as f:
        return json.load(f)


class WeaponDB:
    def __init__(self, weapons: dict | None = None, calibrated: dict | None = None):
        self.weapons = (weapons or load_json("weapons.json"))["weapons"]
        self.calibrated = calibrated if calibrated is not None else load_calibrated()

    def weapon_keys(self) -> list[str]:
        return list(self.weapons)

    def ammo_keys(self, weapon: str) -> list[str]:
        return list(self.weapons[weapon]["ammo"])

    def projectile(self, weapon: str, ammo: str) -> Projectile:
        w = self.weapons[weapon]
        a = dict(w["ammo"][ammo])
        cal = self.calibrated.get(f"{weapon}/{ammo}", {})
        params = {
            "muzzle_velocity": float(cal.get("muzzle_velocity", a["muzzle_velocity"])),
            "drag_k": float(cal.get("drag_k", a.get("drag_k", 0.0))),
            "wind_factor": float(cal.get("wind_factor", a.get("wind_factor", 1.0))),
            "sight_height": float(cal.get("sight_height", a.get("sight_height", w.get("sight_height", 0.08)))),
        }
        return Projectile(**params)

    def is_calibrated(self, weapon: str, ammo: str) -> bool:
        return f"{weapon}/{ammo}" in self.calibrated

    def save_calibration(self, weapon: str, ammo: str, params: dict, report: dict) -> None:
        self.calibrated[f"{weapon}/{ammo}"] = {**params, "_report": report}
        save_json("calibrated.json", self.calibrated)
