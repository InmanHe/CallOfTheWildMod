"""
Apply a population profile to the game's population SAVE files.

Uses APC - Animal Population Changer Revived (MIT, third_party/apc) for the
ADF parsing/editing; this module adds the declarative profile, safe
backups, deploy/restore and a headless CLI. Game install files are never
touched - only the per-reserve population files in your save folder
(Documents/Avalanche Studios/COTW/Saves/<id>/animal_population_N).
"""
from __future__ import annotations

import logging
import os
import shutil
import sys
import time
from contextlib import contextmanager
from pathlib import Path

from .profile import Amount, SpeciesPlan

log = logging.getLogger(__name__)

ROOT = Path(__file__).resolve().parents[2]
APC_DIR = ROOT / "third_party" / "apc"
HOME = Path(os.environ.get("COTWMOD_HOME", Path.home() / ".cotwmod"))
WORK = HOME / "apc_work"
BACKUPS = HOME / "backups"


class _Null:
    """Stand-in for APC's GUI progress bar / message box."""
    def update(self, *a, **k):
        pass


@contextmanager
def _cwd(path: Path):
    old = Path.cwd()
    path.mkdir(parents=True, exist_ok=True)
    os.chdir(path)
    try:
        yield
    finally:
        os.chdir(old)


_apc = None


def apc(save_dir: Path | None = None):
    """Import APC lazily, pointing its mod folder at our work dir."""
    global _apc
    if _apc is None:
        if not (APC_DIR / "apc" / "__init__.py").exists():
            raise RuntimeError("third_party/apc missing - run: git submodule update --init")
        sys.path.insert(0, str(APC_DIR))
        try:
            import FreeSimpleGUI  # noqa: F401  (APC only uses it for type hints)
        except Exception:
            import types
            stub = types.ModuleType("FreeSimpleGUI")
            stub.ProgressBar = stub.Text = object
            sys.modules["FreeSimpleGUI"] = stub
        with _cwd(WORK):     # APC creates mods/ and backups/ in the cwd on import
            from apc import adf, config, populations
        config.MOD_DIR_PATH = WORK / "mods"
        config.MOD_DIR_PATH.mkdir(parents=True, exist_ok=True)
        _apc = (config, adf, populations)
    config = _apc[0]
    if save_dir is not None:
        config.get_save_path = lambda: Path(save_dir)
    return _apc


def find_save_dir() -> Path | None:
    config, _, _ = apc()
    return config.get_save_path()


def game_running() -> bool:
    try:
        import psutil
    except ImportError:
        return False
    return any((p.info.get("name") or "").lower() == "thehuntercotw_f.exe"
               for p in psutil.process_iter(["name"]))


# --------------------------------------------------------------------------- #
# inspection
# --------------------------------------------------------------------------- #
def valid_species() -> dict[str, list[str]]:
    config, _, _ = apc()
    return {k: [s for s in v["species"] if config.valid_species(s)] for k, v in config.RESERVES.items()}


def great_one_species() -> set[str]:
    config, _, _ = apc()
    return {s for s in config.ANIMALS if config.valid_great_one_species(s)}


def _animals(loaded, species: str):
    config, adf, populations = apc()
    groups = populations._get_species_groups(loaded.reserve_key, loaded.parsed_adf.adf, species)
    for g in groups:
        for a in g.value["Animals"].value:
            yield adf.AdfAnimal(a, species, loaded.reserve_key)


def species_stats(loaded, species: str) -> dict:
    config, _, _ = apc()
    st = {"total": 0, "males": 0, "females": 0, "diamonds": 0, "great_ones": 0, "rare_furs": 0}
    rare = {g: set(config.get_rare_furs(species, g) or []) for g in ("male", "female")}
    for a in _animals(loaded, species):
        st["total"] += 1
        st["males" if a.gender == "male" else "females"] += 1
        st["great_ones"] += bool(a.great_one)
        st["diamonds"] += (a.trophy == config.DIAMOND)
        st["rare_furs"] += a.fur_key in rare[a.gender]
    return st


def load_reserve(reserve: str, save_dir: Path, modded: bool = False):
    config, adf, _ = apc(save_dir)
    return adf.LoadedReserve(reserve, modded=modded, parse=True)


def describe(reserve: str, save_dir: Path) -> list[tuple[str, dict]]:
    config, _, populations = apc(save_dir)
    loaded = load_reserve(reserve, save_dir)
    out = []
    for sp in valid_species()[reserve]:
        try:
            out.append((sp, species_stats(loaded, sp)))
        except populations.NoAnimalsException:
            continue
    return out


# --------------------------------------------------------------------------- #
# applying
# --------------------------------------------------------------------------- #
def _run_strategy(loaded, species: str, strategy, amt: Amount, rares: bool = False):
    config, _, populations = apc()
    kwargs = dict(modifier=100 if amt.all else (amt.value if amt.percent else int(amt.value)),
                  percentage=amt.percent or amt.all, rares=rares,
                  progress_bar=_Null(), message_box=_Null())
    try:
        populations.mod(loaded, species, strategy, **kwargs)
    except populations.NoAnimalsException as e:
        # not enough eligible animals: convert as many as possible instead
        log.warning("%s: %s - converting all eligible animals instead", species, e)
        populations.mod(loaded, species, strategy, party=True, **kwargs)


def apply_plan(loaded, plan: SpeciesPlan) -> list[str]:
    config, _, populations = apc()
    S = config.Strategy
    msgs = []
    for key, amt in plan.ops:
        before = species_stats(loaded, plan.species)
        if key in ("males", "females"):
            gender = key[:-1]
            cur = before[key]
            if amt.mode == "=":
                delta = int(amt.value) - cur
            else:
                delta = int(amt.value) * (1 if amt.mode == "+" else -1)
            if delta:
                populations.mod_animal_cnt(loaded, plan.species, delta, gender,
                                           progress_bar=_Null(), message_box=_Null())
        elif key == "to_males":
            _run_strategy(loaded, plan.species, S.males, amt)
        elif key == "to_females":
            _run_strategy(loaded, plan.species, S.females, amt)
        elif key == "rare_furs":
            _run_strategy(loaded, plan.species, S.furs_some, amt, rares=True)
        elif key == "diamonds":
            if amt.all:
                populations.mod(loaded, plan.species, S.diamond_all, rares=plan.diamond_rare_furs,
                                progress_bar=_Null(), message_box=_Null())
            else:
                _run_strategy(loaded, plan.species, S.diamond_some, amt, rares=plan.diamond_rare_furs)
        elif key == "great_ones":
            if amt.all:
                populations.mod(loaded, plan.species, S.great_one_all,
                                progress_bar=_Null(), message_box=_Null())
            else:
                _run_strategy(loaded, plan.species, S.great_one_some, amt)
        after = species_stats(loaded, plan.species)
        diff = {k: after[k] - before[k] for k in after if after[k] != before[k]}
        msgs.append(f"{plan.reserve}/{plan.species} {key}: {diff or 'no change'}")
    return msgs


def _backup(save_dir: Path, filename: str) -> Path:
    src = save_dir / filename
    orig = BACKUPS / "original" / filename
    if not orig.exists():                      # very first backup is kept forever
        orig.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(src, orig)
    stamp = BACKUPS / time.strftime("%Y%m%d-%H%M%S") / filename
    stamp.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(src, stamp)
    return stamp


def apply_profile(plans: list[SpeciesPlan], save_dir: Path, dry_run: bool = False,
                  force: bool = False) -> list[str]:
    if game_running() and not dry_run and not force:
        raise RuntimeError("Close the game first - it rewrites population files on exit.")
    config, adf, _ = apc(save_dir)
    msgs = []
    by_reserve: dict[str, list[SpeciesPlan]] = {}
    for p in plans:
        by_reserve.setdefault(p.reserve, []).append(p)
    for reserve, rplans in by_reserve.items():
        fname = config.get_population_file_name(reserve)
        stale = config.MOD_DIR_PATH / fname
        if stale.exists():
            stale.unlink()
        loaded = load_reserve(reserve, save_dir)
        for plan in rplans:
            msgs += apply_plan(loaded, plan)
        loaded.save()
        if dry_run:
            msgs.append(f"[dry-run] {reserve}: result left in {config.MOD_DIR_PATH / fname}")
            continue
        b = _backup(save_dir, fname)
        shutil.copy2(config.MOD_DIR_PATH / fname, save_dir / fname)
        msgs.append(f"{reserve}: deployed {fname} (backup: {b})")
    return msgs


def restore(reserve: str, save_dir: Path, which: str = "original") -> Path:
    config, _, _ = apc(save_dir)
    fname = config.get_population_file_name(reserve)
    src = BACKUPS / which / fname
    if not src.exists():
        raise FileNotFoundError(f"no backup {src}")
    shutil.copy2(src, save_dir / fname)
    return src
