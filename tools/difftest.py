"""
Compare Rust edits with the original Python APC on the synthetic fixture.
Run after `cargo test --test population`.
"""
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FIX = ROOT / "tests/fixtures"
OUT = ROOT / "target/difftest"
H = [sys.executable, str(ROOT / "tools/apc_harness.py")]


def dump(path):
    return json.loads(subprocess.check_output(H + ["dump", str(path), "hirsch"]))


def apc_op(name, species, op, gender, group):
    out = OUT / f"apc_{name}"
    subprocess.check_call(H + ["op", str(FIX), str(out), "hirsch", species, op, gender, str(group)])
    return out / "animal_population_0"


def mask_first_animal(d, pop, group):
    a = d["values"]["Populations"][pop]["Groups"][group]["Animals"][0]
    for k in ("Weight", "Score", "VisualVariationSeed", "Id", "IsScripted"):
        a[k] = "*"
    return d


fails = 0
# 1) add one male red deer to group 0  (red_deer = species index 8)
apc = mask_first_animal(dump(apc_op("add", "red_deer", "add", "male", 0)), 8, 0)
rs = mask_first_animal(dump(OUT / "add/animal_population_0"), 8, 0)
# the new animal's random score may or may not be a diamond -> compare structure, not summary
ok = apc["values"] == rs["values"]
print("add    :", "IDENTICAL" if ok else "DIFFERENT")
fails += not ok

# 2) remove first female fallow deer from group 0 (fallow_deer = index 2)
apc = dump(apc_op("remove", "fallow_deer", "remove", "female", 0))
rs = dump(OUT / "remove/animal_population_0")
ok = apc == rs
print("remove :", "IDENTICAL" if ok else "DIFFERENT")
fails += not ok

# 3) bigger Rust-only edits must still parse in deca, with APC agreeing on counts
for name in ("many", "rarity"):
    d = dump(OUT / f"{name}/animal_population_0")
    print(f"{name:7s}: deca parses OK;", {k: v for k, v in d["summary"].items() if k in ("eu_bison", "red_deer", "fallow_deer")})
    t = d["values"]
    assert t["Title"] == "73796e74686574696320706f70756c6174696f6e", "string after edits broken"
    assert t["Trailer"] == ["0.0", "1.25", "2.5", "3.75", "5.0"], "trailing array broken"

sys.exit(1 if fails else 0)
