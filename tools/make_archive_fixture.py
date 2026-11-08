"""
Re-pack extracted game files into genuine Apex containers for testing the
Rust archive reader end to end:

    python3 tools/make_archive_fixture.py <org-dir> <out-dir> <deca-python-dir>

Produces <out-dir>/archives_win64/{game0.tab,arc, patch/game1.tab,arc} with:
  * TAB v3 indexes (hash32 of the virtual path, offset, size)
  * half of the SARC bundles and equipment_data.bin AAF-compressed
  * one bundle whose weapon tuning is a symlink (offset 0) resolved via the TAB
  * everything re-read and checked with deca's own TAB / AAF / SARC readers
"""
import io
import os
import struct
import sys
import zlib
from pathlib import Path

org, out, deca_dir = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3]
sys.path.insert(0, deca_dir)
from deca.hashes import hash32_func  # noqa: E402
from deca.ff_aaf import extract_aaf  # noqa: E402
from deca.ff_sarc import FileSarc  # noqa: E402
from deca.ff_arc_tab import tab_file_load  # noqa: E402
from deca.file import ArchiveFile  # noqa: E402


def aaf(data: bytes) -> bytes:
    sec = 32 * 1024 * 1024
    chunks = [data[i:i + sec] for i in range(0, len(data), sec)] or [b""]
    o = bytearray(b"AAF\0" + struct.pack("<I", 1) + b"AVALANCHEARCHIVEFORMATISCOOL")
    o += struct.pack("<III", len(data), min(sec, len(data)), len(chunks))
    for c in chunks:
        co = zlib.compressobj(9, zlib.DEFLATED, -15)
        z = co.compress(c) + co.flush()
        padded = (len(z) + 15) // 16 * 16
        o += struct.pack("<III", len(z), len(c), padded + 16) + b"EWAM" + z + b"\0" * (padded - len(z))
    return bytes(o)


def symlink_tuning(bundle: bytes) -> bytes:
    """Turn the first .wtunec entry of a v2 SARC into a symlink (offset 0)."""
    b = bytearray(bundle)
    dir_len = struct.unpack_from("<I", b, 12)[0]
    p = 16
    while p + 12 <= 16 + dir_len:
        n = struct.unpack_from("<I", b, p)[0]
        name = bytes(b[p + 4:p + 4 + n]).rstrip(b"\0")
        p += 4 + n
        if name.endswith(b".wtunec") and b"/tuning/" in name:
            struct.pack_into("<I", b, p, 0)
            return bytes(b)
        p += 8
    raise RuntimeError("no tuning entry")


files = []
for root, _, fs in os.walk(org / "editor/entities/hp_weapons"):
    for f in fs:
        if f.endswith((".ee", ".wtunec", ".ammotunec")):
            files.append(Path(root) / f)
files.append(org / "settings/hp_settings/equipment_data.bin")
files.sort()

symlinked = "editor/entities/hp_weapons/weapon_bows_01/equipment_weapon_crossbow_01.ee"
arc_dir = out / "archives_win64"
(arc_dir / "patch").mkdir(parents=True, exist_ok=True)
groups = {arc_dir / "game0": [], arc_dir / "patch" / "game1": []}
for i, f in enumerate(files):
    rel = f.relative_to(org).as_posix()
    data = f.read_bytes()
    if rel == symlinked:
        data = symlink_tuning(data)
    if (rel.endswith(".ee") and i % 2 == 0) or rel.endswith("equipment_data.bin"):
        data = aaf(data)
    # weapon bundles to the patch archive, the rest to the base archive
    key = arc_dir / "patch" / "game1" if "weapon_rifles_01" in rel else arc_dir / "game0"
    groups[key].append((rel, data))

for base, items in groups.items():
    tab = bytearray(b"TAB\0" + struct.pack("<HHI", 2, 1, 2048))
    with open(str(base) + ".arc", "wb") as arc:
        for rel, data in items:
            off = arc.tell()
            arc.write(data)
            arc.write(b"\0" * ((-arc.tell()) % 16))
            tab += struct.pack("<III", hash32_func(rel.encode()), off, len(data))
    Path(str(base) + ".tab").write_bytes(tab)

# ---- verify with deca
checked = 0
for base, items in groups.items():
    t = tab_file_load(str(base) + ".tab", 3)
    arc = open(str(base) + ".arc", "rb").read()
    for rel, data in items:
        e = t.file_hash_map[hash32_func(rel.encode())]
        raw = arc[e.offset:e.offset + e.size_c]
        assert raw == data
        plain = extract_aaf(ArchiveFile(io.BytesIO(raw))) if raw.startswith(b"AAF\0") else raw
        orig = (org / rel).read_bytes()
        if rel == symlinked:
            orig = symlink_tuning(orig)
        assert plain == orig, rel
        if rel.endswith(".ee"):
            s = FileSarc()
            s.header_deserialize(io.BytesIO(plain))
            assert len(s.entries) > 0
        checked += 1
print(f"wrote {sum(len(v) for v in groups.values())} files; {checked} verified with deca")
