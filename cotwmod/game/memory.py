"""
Read-only access to live game values (camera angles, wind, aim distance).

Nothing is written to the game process or its files: we only open the
process with read rights and follow pointer chains / AOB signatures listed
in config/offsets.json. Windows only (uses pymem); on other platforms or if
the game isn't running, every field simply reports "unavailable" and the
manual values are used.
"""
from __future__ import annotations

import logging
import re
import struct
import sys
import time

log = logging.getLogger(__name__)

_TYPES = {
    "float": ("<f", 4),
    "double": ("<d", 8),
    "int32": ("<i", 4),
    "uint32": ("<I", 4),
    "int64": ("<q", 8),
}


def _int(v) -> int:
    if isinstance(v, int):
        return v
    return int(str(v), 0)


def parse_aob(pattern: str) -> bytes:
    """'48 8B 05 ?? ?? ?? ??' -> regex bytes for pymem.pattern_scan_module."""
    out = b""
    for tok in pattern.split():
        if tok in ("?", "??"):
            out += b"."
        else:
            out += re.escape(bytes([int(tok, 16)]))
    return out


class MemoryReader:
    def __init__(self, offsets_cfg: dict):
        self.cfg = offsets_cfg
        self.process_name = offsets_cfg.get("process", "theHunterCotW_F.exe")
        self.fields = {k: v for k, v in offsets_cfg.get("fields", {}).items() if v}
        self.pm = None
        self.module = None
        self._aob_cache: dict[str, int] = {}
        self._last_attach = 0.0

    @property
    def configured(self) -> bool:
        return bool(self.fields)

    def attach(self) -> bool:
        if self.pm is not None:
            return True
        if not sys.platform.startswith("win") or not self.configured:
            return False
        now = time.monotonic()
        if now - self._last_attach < 3.0:      # don't hammer while game is closed
            return False
        self._last_attach = now
        try:
            import pymem
            import pymem.process
            pm = pymem.Pymem(self.process_name)
            self.module = pymem.process.module_from_name(pm.process_handle, self.process_name)
            self.pm = pm
            self._aob_cache.clear()
            log.info("attached to %s (base 0x%X)", self.process_name, self.module.lpBaseOfDll)
            return True
        except Exception as e:  # game not running, no permission, pymem missing...
            log.debug("attach failed: %s", e)
            self.pm = None
            return False

    def _base_address(self, name: str, spec: dict) -> int:
        if "aob" in spec:
            if name not in self._aob_cache:
                import pymem.pattern
                hit = pymem.pattern.pattern_scan_module(self.pm.process_handle, self.module,
                                                        parse_aob(spec["aob"]))
                if not hit:
                    raise LookupError(f"AOB for {name} not found (game updated?)")
                rip_off = _int(spec.get("rip_offset", 3))
                instr_len = _int(spec.get("instr_len", 7))
                rel = struct.unpack("<i", self.pm.read_bytes(hit + rip_off, 4))[0]
                self._aob_cache[name] = hit + instr_len + rel
            return self._aob_cache[name]
        return self.module.lpBaseOfDll + _int(spec["base"])

    def read_field(self, name: str):
        spec = self.fields.get(name)
        if not spec or not self.attach():
            return None
        try:
            addr = self._base_address(name, spec)
            offsets = [_int(o) for o in spec.get("offsets", [])]
            # Cheat Engine semantics: [[base]+o1]+o2 ... last offset is added, not dereferenced
            if offsets:
                addr = self.pm.read_longlong(addr)
                for o in offsets[:-1]:
                    addr = self.pm.read_longlong(addr + o)
                addr += offsets[-1]
            fmt, size = _TYPES[spec.get("type", "float")]
            val = struct.unpack(fmt, self.pm.read_bytes(addr, size))[0]
            return val * float(spec.get("scale", 1.0)) + float(spec.get("add", 0.0))
        except Exception as e:
            log.debug("read %s failed: %s", name, e)
            if "process" in str(e).lower() or "handle" in str(e).lower():
                self.pm = None      # game closed - reattach later
            return None

    def read_all(self) -> dict:
        return {k: self.read_field(k) for k in self.fields}
