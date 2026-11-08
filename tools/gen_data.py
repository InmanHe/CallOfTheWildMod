"""Build data/apc_data.json from third_party/apc (MIT). Run: python3 tools/gen_data.py"""
import json, re, sys
from pathlib import Path
root = Path(__file__).resolve().parents[1]
apc = root / "third_party/apc/apc"
cfg = apc / "config"
def po(path):
    out, msgid = {}, None
    txt = path.read_text("utf-8")
    for m in re.finditer(r'msgid "((?:[^"\\]|\\.)*)"\s*\nmsgstr "((?:[^"\\]|\\.)*)"', txt):
        if m.group(1) and m.group(2):
            out[m.group(1)] = m.group(2)
    return out
zh = po(apc / "locale/zh_CN/LC_MESSAGES/apc.po")
reserves = json.load(open(cfg / "reserve_details.json", encoding="utf-8"))
animals = json.load(open(cfg / "animal_details.json", encoding="utf-8"))
names = json.load(open(cfg / "animal_names.json", encoding="utf-8"))
furs = json.load(open(cfg / "fur_names.json", encoding="utf-8"))
out = {"reserves": [], "species": {}, "furs": {}}
for k, r in reserves.items():
    out["reserves"].append({"key": k, "index": r["index"], "species": r["species"],
        "renames": r.get("renames", {}), "en": r["reserve_name"], "zh": zh.get(r["reserve_name"], r["reserve_name"])})
for k, d in animals.items():
    en = names.get(k, {}).get("animal_name", k)
    out["species"][k] = {"en": en, "zh": zh.get(en, en), **d}
for k, f in furs.items():
    en = f["fur_name"]
    out["furs"][k] = {"en": en, "zh": zh.get(en, en)}
missing = [s for r in out["reserves"] for s in r["species"] if s not in out["species"]]
print("reserves", len(out["reserves"]), "species", len(out["species"]), "furs", len(out["furs"]),
      "zh species", sum(v["zh"] != v["en"] for v in out["species"].values()), "unknown species refs", len(missing))
(root / "data/apc_data.json").write_text(json.dumps(out, ensure_ascii=False, separators=(",", ":")), "utf-8")
