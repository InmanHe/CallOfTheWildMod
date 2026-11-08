"""
Build a synthetic but format-faithful population file for tests:
    tests/fixtures/animal_population_0   (Hirschfelden layout: 9 species)

Layout mirrors what deca/APC expect: ADF v4, one instance, data first, then
instance table / typedefs / name table. Includes strings and trailing arrays
so offset fix-ups after insert/remove are exercised.
"""
import random
import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
T_U8, T_U32, T_F32, T_STR = 0x0ca2821d, 0x075e4e4f, 0x7515a207, 0x8955583e

names = []
def nm(s):
    if s not in names:
        names.append(s)
    return names.index(s)

types = []  # (metatype, size, align, hash, name, elem, elen, members)
def struct_t(h, name, size, members):
    types.append((1, size, 4, h, name, 0, 0, members))
def array_t(h, name, elem):
    types.append((3, 16, 8, h, name, elem, 0, []))

H_POS, H_ANIMAL, H_ANIMALS, H_GROUP, H_GROUPS, H_POP, H_POPS, H_ROOT, H_FARR = (
    0x1001, 0x1002, 0x1003, 0x1004, 0x1005, 0x1006, 0x1007, 0x1008, 0x1009)
struct_t(H_POS, "Vec2", 8, [("X", T_F32, 4, 0), ("Y", T_F32, 4, 4)])
struct_t(H_ANIMAL, "Animal", 32, [
    ("Gender", T_U8, 1, 0), ("Weight", T_F32, 4, 4), ("Score", T_F32, 4, 8),
    ("IsGreatOne", T_U8, 1, 12), ("IsScripted", T_U8, 1, 13),
    ("VisualVariationSeed", T_U32, 4, 16), ("Id", T_U32, 4, 20), ("MapPosition", H_POS, 8, 24)])
array_t(H_ANIMALS, "A[Animal]", H_ANIMAL)
struct_t(H_GROUP, "Group", 40, [("GroupId", T_U32, 4, 0), ("Animals", H_ANIMALS, 16, 8),
                                ("Name", T_STR, 8, 24), ("Radius", T_F32, 4, 32)])
array_t(H_GROUPS, "A[Group]", H_GROUP)
array_t(H_FARR, "A[float]", T_F32)
struct_t(H_POP, "Population", 40, [("SpeciesId", T_U32, 4, 0), ("Groups", H_GROUPS, 16, 8),
                                   ("Densities", H_FARR, 16, 24)])
array_t(H_POPS, "A[Population]", H_POP)
struct_t(H_ROOT, "AnimalPopulationData", 48, [("Version", T_U32, 4, 0), ("Populations", H_POPS, 16, 8),
                                              ("Trailer", H_FARR, 16, 24), ("Title", T_STR, 8, 40)])


def build(seed=7, n_species=9):
    rnd = random.Random(seed)
    buf = bytearray()

    def alloc(n, align=8):
        while len(buf) % align:
            buf.append(0)
        p = len(buf)
        buf.extend(b"\0" * n)
        return p

    def put(fmt, p, *v):
        struct.pack_into("<" + fmt, buf, p, *v)

    def set_array(at, off, length):
        put("IIII", at, off, 0, length, 0)

    def put_str(at, s):
        p = alloc(len(s) + 1, 1)
        buf[p:p + len(s)] = s.encode()
        put("II", at, p, 0)

    root = alloc(48)
    put("I", root, 1)
    pops = alloc(40 * n_species)
    set_array(root + 8, pops, n_species)
    for si in range(n_species):
        pp = pops + 40 * si
        put("I", pp, si)
        ng = rnd.randint(3, 6)
        groups = alloc(40 * ng)
        set_array(pp + 8, groups, ng)
        for gi in range(ng):
            gp = groups + 40 * gi
            put("I", gp, gi)
            put("f", gp + 32, rnd.uniform(50, 200))
            na = rnd.randint(2, 8)
            animals = alloc(32 * na)
            set_array(gp + 8, animals, na)
            for ai in range(na):
                ap = animals + 32 * ai
                male = ai % 2 == 0
                put("B", ap, 1 if male else 2)
                put("f", ap + 4, rnd.uniform(100, 200) if male else rnd.uniform(80, 150))
                put("f", ap + 8, rnd.uniform(50, 150) if male else 0.0)
                put("B", ap + 12, 0)
                put("I", ap + 16, rnd.getrandbits(32))
                put("I", ap + 20, rnd.getrandbits(16))
                put("ff", ap + 24, rnd.uniform(0, 8000), rnd.uniform(0, 8000))
            put_str(gp + 24, f"group_{si}_{gi}")
        dens = alloc(4 * 3, 4)
        for k in range(3):
            put("f", dens + 4 * k, 0.5 * k)
        set_array(pp + 24, dens, 3)
    trailer = alloc(4 * 5, 4)
    for k in range(5):
        put("f", trailer + 4 * k, 1.25 * k)
    set_array(root + 24, trailer, 5)
    put_str(root + 40, "synthetic population")
    return bytes(buf)


def adf_file(instance: bytes) -> bytes:
    out = bytearray(b"\0" * 0x40)
    out += b"\0"  # empty comment
    while len(out) % 16:
        out.append(0)
    inst_off = len(out)
    out += instance
    while len(out) % 8:
        out.append(0)
    root_name = nm("AnimalPopulationData")
    # instance table
    instance_table = len(out)
    out += struct.pack("<IIIIQ", 0xABCDEF01, H_ROOT, inst_off, len(instance), root_name)
    # typedefs
    td_off = len(out)
    for (meta, size, align, h, name, elem, elen, members) in types:
        out += struct.pack("<IIIIQIII", meta, size, align, h, nm(name), 0, elem, elen)
        if meta == 1:
            out += struct.pack("<I", len(members))
            for (mname, mt, msize, moff) in members:
                out += struct.pack("<QIIIIQ", nm(mname), mt, msize, moff, 0, 0)
        else:
            out += struct.pack("<I", 0)
    # name table
    nt_off = len(out)
    out += bytes(len(n) for n in names)
    for n in names:
        out += n.encode() + b"\0"
    struct.pack_into("<4sIIIIIIIIII", out, 0, b" FDA", 4, 1, instance_table, len(types), td_off,
                     0, 0, len(names), nt_off, len(out))
    return bytes(out)


def pop_file(adf: bytes) -> bytes:
    dec = b"\x00\x01\x02\x03\x04" + adf
    hdr = bytearray(32)
    hdr[0:4] = b"SARC"
    struct.pack_into("<I", hdr, 8, len(dec))
    struct.pack_into("<I", hdr, 24, len(dec))
    return bytes(hdr) + zlib.compress(dec)


if __name__ == "__main__":
    # names used in members must be registered before the table is written
    for t in types:
        nm(t[4])
        for m in t[7]:
            nm(m[0])
    d = ROOT / "tests/fixtures"
    d.mkdir(parents=True, exist_ok=True)
    (d / "animal_population_0").write_bytes(pop_file(adf_file(build())))
    print("wrote", d / "animal_population_0")
