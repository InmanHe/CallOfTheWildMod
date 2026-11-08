"""
Build data/ballistics.json from game files (read-only):

    python3 tools/gen_ballistics.py <org-dir>

<org-dir> is a directory of extracted game files with the game's layout, e.g.
the `modbuilder/org` folder of cpypasta's COTW Mod Builder (extracted from game
build 2777186), or your own extraction with deca. Uses deca (MIT) from
third_party/apc. Nothing in the game is modified.

Output, per weapon (equipment item): display name, tuning (kinetic energy,
gravity, zeroing table with per-ammo angle overrides, scatter) and compatible
ammo; per ammo: mass, diameter, drag coefficient, kinetic energy, max range,
velocity deviance, scatter multiplier.
"""
import contextlib
import json
import os
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "third_party/apc"))
from deca.ff_adf import Adf  # noqa: E402
from deca.ff_rtpc import rtpc_from_binary  # noqa: E402
from deca.file import ArchiveFile  # noqa: E402
from deca.hashes import hash32_func  # noqa: E402

P_NAME = 0xd31ab684      # equipment name
P_DISPLAY = 0x2314c9ea   # English display name
CAT_AMMO, CAT_WEAPON, CAT_SIGHT = 0x1f3f2b00, 0xc04b7961, 0x71b61a34


def s(d):
    return d.decode("utf-8", "replace") if isinstance(d, (bytes, bytearray)) else d


def adf(path):
    a = Adf()
    with ArchiveFile(open(path, "rb")) as fp:
        with contextlib.redirect_stdout(None):
            a.deserialize(fp)
    return a.table_instance_values[0]


def props(node):
    return {p.name_hash: p.data for p in node.prop_table}


def collect_names(node, out):
    for p in node.prop_table:
        if isinstance(p.data, (bytes, bytearray)) and p.data.startswith(b"equipment_"):
            out.append(s(p.data))
    for c in node.child_table:
        collect_names(c, out)


def main(org: Path):
    eq = rtpc_from_binary(open(org / "settings/hp_settings/equipment_data.bin", "rb")).root_node
    cats = {c.name_hash: c for c in eq.child_table}

    # ---- ammo items + compatibility
    ammo = {}
    for it in cats[CAT_AMMO].child_table:
        p = props(it)
        name = s(p.get(P_NAME, b""))
        comp = []
        for c in it.child_table:
            collect_names(c, comp)
        ammo[name] = {"display": s(p.get(P_DISPLAY, b"")), "compatible": sorted({x for x in comp if x.startswith("equipment_weapon_")})}

    # ---- ammo physics
    for f in (org / "editor/entities/hp_weapons/ammunition").glob("*/*.ammotunec"):
        key = f.name.split(".")[0]
        v = adf(f)
        d = ammo.setdefault(key, {"display": key, "compatible": []})
        d.update({
            "mass": float(v["mass"]), "diameter": float(v["diameter"]), "cd": float(v["drag_coefficient"]),
            "ke": float(v["kinetic_energy"]), "max_range": float(v["max_range"]),
            "velocity_deviance": float(v["velocity_deviance"]), "scatter_multiplier": float(v["scatter_multiplier"]),
            "projectiles": int(v["projectiles_per_shot"]), "hash": hash32_func(key.encode()),
        })
    ammo = {k: v for k, v in ammo.items() if "mass" in v}

    # ---- weapon tunings
    tunings = {}
    for f in (org / "editor/entities/hp_weapons").glob("*/tuning/*.wtunec"):
        try:
            v = adf(f)
        except Exception:
            continue
        if "ZeroingSettings" not in v:
            continue
        bt = v["bullet_weapon"][0]["tuning"]
        zeros = []
        by_hash = {a["hash"]: k for k, a in ammo.items()}
        for z in v["ZeroingSettings"]:
            ov = {}
            for o in z["BulletOverrides"]:
                k = by_hash.get(int(o["BulletName"]))
                if k:
                    ov[k] = float(o["Angle"])
            zeros.append({"distance": float(z["zero_distance"]), "angle": float(z["angle"]),
                          "h_angle": float(z["horizontal_angle"]), "overrides": ov})
        sc = bt["base_tuning"]["scatter"]
        tunings[f.relative_to(org).as_posix()] = {
            "ke": float(v["kinetic_energy"]),
            "gravity": float(bt["bullet_base_tuning"]["gravity_strength"]),
            "gravity_on": int(bt["bullet_base_tuning"]["gravity_on"]),
            "zeros": zeros,
            "scatter_min": float(sc["scatter_minimum"]), "scatter_max": float(sc["scatter_maximum"]),
            "vertical_offset": float(v.get("VerticalOffset", 0.0)),
        }

    # ---- weapon items -> tuning (via entity files that name both)
    weapons = {}
    for it in cats[CAT_WEAPON].child_table:
        p = props(it)
        name = s(p.get(P_NAME, b""))
        weapons[name] = {"display": s(p.get(P_DISPLAY, b"")), "tuning": None}
    for f in (org / "editor/entities/hp_weapons").glob("*/*.ee"):
        b = f.read_bytes()
        tun = [m.decode() for m in re.findall(rb"editor/entities/hp_weapons/[\w/\-]+/tuning/[\w\-]+\.wtunec", b)]
        tun = [t for t in tun if t in tunings]
        if not tun:
            continue
        for m in set(re.findall(rb"equipment_weapon_[a-z0-9_]+", b)):
            n = m.decode()
            if n in weapons and weapons[n]["tuning"] is None:
                weapons[n]["tuning"] = tun[0]
    for n, w in weapons.items():
        w["ammo"] = sorted(k for k, a in ammo.items() if n in a["compatible"])
        w["hash"] = hash32_func(n.encode())
    weapons = {k: v for k, v in weapons.items() if v["tuning"] and v["ammo"]}
    for a in ammo.values():
        a.pop("compatible", None)

    out = {"source": "theHunter: Call of the Wild game data (build 2777186)", "weapons": weapons, "ammo": ammo, "tunings": tunings}
    dst = ROOT / "data/ballistics.json"
    dst.write_text(json.dumps(out, separators=(",", ":"), sort_keys=True), "utf-8")
    print(f"weapons {len(weapons)}  ammo {len(ammo)}  tunings {len(tunings)}  -> {dst}")
    missing = [k for k, v in weapons.items() if not v["ammo"]]
    if missing:
        print("weapons without ammo:", missing)


if __name__ == "__main__":
    main(Path(sys.argv[1]))
