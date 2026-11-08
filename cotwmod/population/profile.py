"""
Population profile: a YAML file that declares, per reserve and species,
how many animals there should be and how rare they should be.

Example (see config/population_profile.example.yaml):

    reserves:
      hirsch:                     # reserve key (cotwmod pop reserves)
        red_deer:                 # species key  (cotwmod pop species hirsch)
          males: "+20"            # +N add, -N remove, =N set total males to N
          females: "=60"
          to_males: 10%           # convert 10% of females into males
          rare_furs: 15%          # 15% of animals get a random rare fur
          diamonds: 8             # 8 diamonds (count) - or "5%" - or "all"
          diamond_rare_furs: true # diamonds also get rare furs
          great_ones: 2           # 2 Great Ones (species that have them)

Operations always run in this order so later steps don't undo earlier ones:
    counts -> gender conversion -> rare furs -> diamonds -> great ones
"""
from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path

import yaml

ORDER = ("males", "females", "to_males", "to_females", "rare_furs", "diamonds", "great_ones")
KNOWN = set(ORDER) | {"diamond_rare_furs"}


class ProfileError(ValueError):
    pass


@dataclass
class Amount:
    value: float
    percent: bool = False
    all: bool = False
    mode: str = "+"            # for counts: '+', '-', '='

    def resolve(self, population: int) -> int:
        if self.all:
            return population
        if self.percent:
            return int(round(population * self.value / 100.0))
        return int(self.value)


_COUNT_RE = re.compile(r"^\s*([+\-=]?)\s*(\d+)\s*$")
_AMOUNT_RE = re.compile(r"^\s*(\d+(?:\.\d+)?)\s*(%?)\s*$")


def parse_count(v) -> Amount:
    if isinstance(v, bool):
        raise ProfileError(f"bad count {v!r}")
    if isinstance(v, int):
        return Amount(abs(v), mode="+" if v >= 0 else "-")
    m = _COUNT_RE.match(str(v))
    if not m:
        raise ProfileError(f"bad count {v!r} - use +N, -N or =N")
    return Amount(int(m.group(2)), mode=m.group(1) or "+")


def parse_amount(v) -> Amount:
    if isinstance(v, bool):
        raise ProfileError(f"bad amount {v!r}")
    if isinstance(v, (int, float)):
        if v < 0:
            raise ProfileError(f"amount must be >= 0, got {v}")
        return Amount(float(v))
    s = str(v).strip().lower()
    if s == "all":
        return Amount(100, percent=True, all=True)
    m = _AMOUNT_RE.match(s)
    if not m:
        raise ProfileError(f"bad amount {v!r} - use N, N% or all")
    pct = m.group(2) == "%"
    val = float(m.group(1))
    if pct and val > 100:
        raise ProfileError(f"percentage over 100: {v!r}")
    return Amount(val, percent=pct)


@dataclass
class SpeciesPlan:
    reserve: str
    species: str
    ops: list[tuple[str, Amount]]
    diamond_rare_furs: bool = False


def parse_profile(data: dict, valid_species: dict[str, list[str]] | None = None,
                  great_one_species: set[str] | None = None) -> list[SpeciesPlan]:
    """
    Validate a profile dict and turn it into ordered plans.
    ``valid_species`` maps reserve key -> species keys (from APC config) and,
    if given, is used to reject typos before touching any file.
    """
    if not isinstance(data, dict) or "reserves" not in data:
        raise ProfileError("profile must have a top-level 'reserves:' mapping")
    plans: list[SpeciesPlan] = []
    for reserve, species_map in (data["reserves"] or {}).items():
        if valid_species is not None and reserve not in valid_species:
            raise ProfileError(f"unknown reserve '{reserve}'. valid: {', '.join(valid_species)}")
        for species, spec in (species_map or {}).items():
            if valid_species is not None and species not in valid_species[reserve]:
                raise ProfileError(f"'{species}' does not live in '{reserve}'. "
                                   f"valid: {', '.join(valid_species[reserve])}")
            spec = spec or {}
            unknown = set(spec) - KNOWN
            if unknown:
                raise ProfileError(f"{reserve}/{species}: unknown keys {sorted(unknown)}; "
                                   f"allowed: {sorted(KNOWN)}")
            ops = []
            for key in ORDER:
                if key not in spec:
                    continue
                amt = parse_count(spec[key]) if key in ("males", "females") else parse_amount(spec[key])
                ops.append((key, amt))
            if great_one_species is not None and "great_ones" in spec and species not in great_one_species:
                raise ProfileError(f"{species} has no Great One in the game")
            plans.append(SpeciesPlan(reserve, species, ops, bool(spec.get("diamond_rare_furs", False))))
    return plans


def load_profile(path: Path, **kw) -> list[SpeciesPlan]:
    with open(path, encoding="utf-8") as f:
        return parse_profile(yaml.safe_load(f), **kw)
