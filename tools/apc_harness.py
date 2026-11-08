"""
Differential-test harness: run the ORIGINAL Python APC/deca code on a
population file and dump a canonical JSON of its contents.

    python3 tools/apc_harness.py dump <file> <reserve> > out.json
    python3 tools/apc_harness.py op <in-dir> <out-dir> <reserve> <species> add|remove <male|female> <group>
"""
import contextlib
import json
import os
import sys
import types
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "third_party/apc"))
try:
    import FreeSimpleGUI  # noqa
except Exception:
    stub = types.ModuleType("FreeSimpleGUI")
    stub.ProgressBar = stub.Text = object
    sys.modules["FreeSimpleGUI"] = stub

ARGS_ABS = [str(Path(a).resolve()) if Path(a).exists() else a for a in sys.argv]
sys.argv = ARGS_ABS
WORK = Path(os.environ.get("APC_WORK", "/tmp/apc_work"))
WORK.mkdir(parents=True, exist_ok=True)
os.chdir(WORK)
import logging
logging.disable(logging.CRITICAL)
from apc import adf, config, populations  # noqa: E402
from deca.ff_adf import adf_value_extract  # noqa: E402


def dump(path: Path, reserve: str):
    loaded = adf.load_adf(path)
    root = loaded.adf.table_instance_full_values[0]
    vals = adf_value_extract(root)
    out = {"values": vals}
    # also expose what APC thinks of each animal
    species = config.RESERVES[reserve]["species"]
    summary = {}
    for si, pop in enumerate(root.value["Populations"].value):
        if si >= len(species):
            break
        sp = species[si]
        cnt = {"total": 0, "male": 0, "female": 0, "diamond": 0, "great_one": 0}
        for g in pop.value["Groups"].value:
            for a in g.value["Animals"].value:
                an = adf.AdfAnimal(a, sp, reserve)
                cnt["total"] += 1
                cnt[an.gender] += 1
                cnt["great_one"] += an.great_one
                cnt["diamond"] += an.trophy == config.DIAMOND
        summary[sp] = cnt
    out["summary"] = summary
    return out


def main():
    cmd = sys.argv[1]
    if cmd == "dump":
        with contextlib.redirect_stdout(sys.stderr):
            d = dump(Path(sys.argv[2]), sys.argv[3])
        json.dump(d, sys.stdout, default=lambda o: o.hex() if isinstance(o, (bytes, bytearray)) else str(o))
    elif cmd == "op":
        in_dir, out_dir, reserve, species, op, gender, group = sys.argv[2:9]
        config.get_save_path = lambda: Path(in_dir)
        config.MOD_DIR_PATH = Path(out_dir)
        Path(out_dir).mkdir(parents=True, exist_ok=True)
        with contextlib.redirect_stdout(sys.stderr):
            loaded = adf.LoadedReserve(reserve, parse=True)
            groups = populations._get_species_groups(reserve, loaded.parsed_adf.adf, species)
            g = groups[int(group)]
            if op == "add":
                adf.add_animal_to_group(loaded, g, species, gender)
            else:
                assert adf.remove_animal_from_group(loaded, g, species, gender)
            loaded.save()


if __name__ == "__main__":
    main()
