"""cotwmod command line.

    cotwmod aim                          start the aiming overlay
    cotwmod table  [-w W -a A -z Z]      print a trajectory / hold-over table
    cotwmod calibrate [-w W -a A]        fit ballistics from recorded shots
    cotwmod offsets                      live-test config/offsets.json
    cotwmod pop reserves | species R | show R | apply PROFILE [--dry-run] | restore R
"""
from __future__ import annotations

import argparse
import json
import logging
import sys
import time
from pathlib import Path

from .config import CONFIG_DIR, WeaponDB, load_json


def _settings():
    return load_json("settings.json")


def _memory(settings):
    from .game.memory import MemoryReader
    if settings["input"].get("mode") != "memory":
        return None
    return MemoryReader(load_json("offsets.json"))


def cmd_aim(args):
    from .game.aim import AimEngine
    from .overlay.overlay import run_overlay
    settings = _settings()
    engine = AimEngine(WeaponDB(), settings, _memory(settings))
    mem = engine.memory
    if mem is not None and not mem.configured:
        print("offsets.json 尚未配置 -> 全部使用手動輸入 (見 docs/OFFSETS_GUIDE.md)")
    return run_overlay(engine, settings)


def cmd_table(args):
    from .ballistics import ShotConditions, solve, wind_components
    s = _settings()["startup"]
    db = WeaponDB()
    w = args.weapon or s["weapon"]
    a = args.ammo or s["ammo"]
    z = float(args.zero or db.weapons[w].get("default_zero"))
    proj = db.projectile(w, a)
    hw, cw = wind_components(args.wind, args.wind_dir)
    print(f"{db.weapons[w]['name']} / {db.weapons[w]['ammo'][a]['name']}  zero {z:g} m  "
          f"v0 {proj.muzzle_velocity:.1f} m/s  k {proj.drag_k:.5f}  "
          f"{'calibrated' if db.is_calibrated(w, a) else 'UNCALIBRATED'}")
    print(f"{'dist':>5} {'drop cm':>8} {'drop mrad':>9} {'drift cm':>8} {'drift mrad':>10} {'tof s':>6} {'v m/s':>6}")
    for d in range(args.step, args.max + 1, args.step):
        r = solve(proj, z, ShotConditions(float(d), args.incline, hw, cw))
        print(f"{d:5d} {r.drop_m * 100:8.1f} {r.drop_mrad:9.2f} {r.drift_m * 100:8.1f} "
              f"{r.drift_mrad:10.2f} {r.time_of_flight:6.3f} {r.impact_velocity:6.1f}")


def cmd_calibrate(args):
    from .ballistics.calibration import fit
    db = WeaponDB()
    path = Path(args.shots or CONFIG_DIR / "calibration_shots.json")
    shots = json.loads(path.read_text("utf-8"))
    combos = sorted({(s["weapon"], s["ammo"]) for s in shots})
    if args.weapon:
        combos = [c for c in combos if c[0] == args.weapon and (not args.ammo or c[1] == args.ammo)]
    if not combos:
        print("沒有匹配的校準射擊記錄")
        return 1
    for w, a in combos:
        sub = [s for s in shots if s["weapon"] == w and s["ammo"] == a]
        fitted, rep = fit(db.projectile(w, a), sub)
        print(f"{w}/{a}: {len(sub)} shots -> {rep['params']}  rms {rep['rms_mrad']:.3f} mrad, "
              f"max {rep['max_abs_mrad']:.3f} mrad")
        if rep["rms_mrad"] > 0.5:
            print("  ! 殘差偏大: 檢查距離/歸零/風記錄是否準確, 或增加射擊次數")
        if not args.dry_run:
            db.save_calibration(w, a, rep["params"], rep)
    if not args.dry_run:
        print(f"已寫入 {CONFIG_DIR / 'calibrated.json'}")


def cmd_offsets(args):
    from .game.memory import MemoryReader
    m = MemoryReader(load_json("offsets.json"))
    if not m.configured:
        print("offsets.json 中沒有已配置的欄位")
        return 1
    for _ in range(args.count):
        print({k: (round(v, 3) if isinstance(v, float) else v) for k, v in m.read_all().items()})
        time.sleep(0.5)


def cmd_pop(args):
    from .population import apply as A
    from .population.profile import load_profile
    save_dir = Path(args.save_dir) if args.save_dir else A.find_save_dir()
    if args.pop_cmd == "reserves":
        cfg = A.apc()[0]
        for k in cfg.RESERVES:
            print(f"{k:14s} {cfg.get_reserve_name(k)}")
        return
    if args.pop_cmd == "species":
        cfg = A.apc()[0]
        go = A.great_one_species()
        for sp in A.valid_species()[args.reserve]:
            print(f"{sp:22s} {cfg.get_species_name(sp)}{'  [Great One]' if sp in go else ''}")
        return
    if save_dir is None:
        print("找不到存檔資料夾, 請用 --save-dir 指定 (Documents/Avalanche Studios/COTW/Saves/<數字>)")
        return 1
    if args.pop_cmd == "show":
        print(f"{'species':22s} {'total':>6} {'male':>5} {'female':>6} {'diamond':>7} {'GO':>3} {'rare':>5}")
        for sp, st in A.describe(args.reserve, save_dir):
            print(f"{sp:22s} {st['total']:6d} {st['males']:5d} {st['females']:6d} "
                  f"{st['diamonds']:7d} {st['great_ones']:3d} {st['rare_furs']:5d}")
    elif args.pop_cmd == "apply":
        plans = load_profile(Path(args.profile), valid_species=A.valid_species(),
                             great_one_species=A.great_one_species())
        for m in A.apply_profile(plans, save_dir, dry_run=args.dry_run, force=args.force):
            print(m)
    elif args.pop_cmd == "restore":
        src = A.restore(args.reserve, save_dir, args.backup)
        print(f"已從 {src} 還原")


def main(argv=None):
    logging.basicConfig(level=logging.INFO, format="%(levelname)s %(message)s")
    ap = argparse.ArgumentParser(prog="cotwmod")
    sub = ap.add_subparsers(dest="cmd", required=True)

    sub.add_parser("aim", help="啟動瞄準疊加層").set_defaults(fn=cmd_aim)

    t = sub.add_parser("table", help="輸出彈道表")
    t.add_argument("-w", "--weapon"); t.add_argument("-a", "--ammo"); t.add_argument("-z", "--zero")
    t.add_argument("--max", type=int, default=80); t.add_argument("--step", type=int, default=5)
    t.add_argument("--incline", type=float, default=0.0)
    t.add_argument("--wind", type=float, default=0.0, help="m/s")
    t.add_argument("--wind-dir", type=float, default=270.0, help="來向, 相對朝向的角度 (270=從左吹來)")
    t.set_defaults(fn=cmd_table)

    c = sub.add_parser("calibrate", help="由射擊記錄擬合彈道參數")
    c.add_argument("-w", "--weapon"); c.add_argument("-a", "--ammo"); c.add_argument("--shots")
    c.add_argument("--dry-run", action="store_true")
    c.set_defaults(fn=cmd_calibrate)

    o = sub.add_parser("offsets", help="測試記憶體位址")
    o.add_argument("-n", "--count", type=int, default=10)
    o.set_defaults(fn=cmd_offsets)

    p = sub.add_parser("pop", help="種群數量/珍稀度控制")
    p.add_argument("--save-dir")
    psub = p.add_subparsers(dest="pop_cmd", required=True)
    psub.add_parser("reserves")
    sp = psub.add_parser("species"); sp.add_argument("reserve")
    sh = psub.add_parser("show"); sh.add_argument("reserve")
    pa = psub.add_parser("apply"); pa.add_argument("profile")
    pa.add_argument("--dry-run", action="store_true"); pa.add_argument("--force", action="store_true")
    pr = psub.add_parser("restore"); pr.add_argument("reserve"); pr.add_argument("--backup", default="original")
    p.set_defaults(fn=cmd_pop)

    args = ap.parse_args(argv)
    return args.fn(args) or 0


if __name__ == "__main__":
    sys.exit(main())
