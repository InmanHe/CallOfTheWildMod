//! Avalanche Data Format (ADF) reader + structural editor.
//!
//! Port of the parts of `deca/ff_adf.py` (Krzysztof Kamieniecki, MIT) that the
//! population files need, plus the insert/remove fix-up logic of APC
//! (Ryan Maxim, MIT). Differences from APC: element bytes are cloned using the
//! element type's real size from the file's own type table, primitive writes use
//! the field's real width, and the tree is re-parsed after every structural edit
//! so offsets can never drift.

use anyhow::{anyhow, bail, Context, Result};
use indexmap::IndexMap;
use std::collections::HashMap;

pub const T_S8: u32 = 0x580D0A62;
pub const T_U8: u32 = 0x0ca2821d;
pub const T_S16: u32 = 0xD13FCF93;
pub const T_U16: u32 = 0x86d152bd;
pub const T_S32: u32 = 0x192fe633;
pub const T_U32: u32 = 0x075e4e4f;
pub const T_S64: u32 = 0xAF41354F;
pub const T_U64: u32 = 0xA139E01F;
pub const T_F32: u32 = 0x7515a207;
pub const T_F64: u32 = 0xC609F663;
pub const T_STRING: u32 = 0x8955583e;
pub const T_DEFERRED: u32 = 0xdefe88ed;

fn prim_size(t: u32) -> Option<usize> {
    Some(match t {
        T_S8 | T_U8 => 1,
        T_S16 | T_U16 => 2,
        T_S32 | T_U32 | T_F32 => 4,
        T_S64 | T_U64 | T_F64 => 8,
        _ => return None,
    })
}

#[derive(Debug, Clone)]
pub struct Member {
    pub name: String,
    pub type_hash: u32,
    pub size: u32,
    pub offset: u32,
    pub bit_offset: u32,
}

#[derive(Debug, Clone)]
pub struct TypeDef {
    pub metatype: u32,
    pub size: u32,
    pub alignment: u32,
    pub type_hash: u32,
    pub name: String,
    pub element_type: u32,
    pub element_length: u32,
    pub members: Vec<Member>,
}

#[derive(Debug, Clone)]
pub struct Instance {
    pub name_hash: u32,
    pub type_hash: u32,
    pub offset: u32,
    pub size: u32,
    pub name: String,
    /// byte position of this entry in the file (for patching size)
    pub entry_pos: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Prim {
    I(i64),
    U(u64),
    F(f64),
}

impl Prim {
    pub fn as_f64(&self) -> f64 {
        match *self {
            Prim::I(v) => v as f64,
            Prim::U(v) => v as f64,
            Prim::F(v) => v,
        }
    }
    pub fn as_u64(&self) -> u64 {
        match *self {
            Prim::I(v) => v as u64,
            Prim::U(v) => v,
            Prim::F(v) => v as u64,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Val {
    Prim(Prim),
    Str(String),
    Struct(IndexMap<String, Node>),
    Array(Vec<Node>),
    Bytes(Vec<u8>),
    Other,
}

/// A parsed value. `info` = where the value (or its pointer header) sits;
/// `data` = where the referenced data starts (differs for arrays/strings/deferred).
#[derive(Debug, Clone)]
pub struct Node {
    pub type_id: u32,
    pub info: usize,
    pub data: usize,
    pub val: Val,
}

impl Node {
    pub fn field(&self, name: &str) -> Option<&Node> {
        match &self.val {
            Val::Struct(m) => m.get(name),
            _ => None,
        }
    }
    pub fn get(&self, name: &str) -> Result<&Node> {
        self.field(name).ok_or_else(|| anyhow!("missing field {name}"))
    }
    pub fn items(&self) -> &[Node] {
        match &self.val {
            Val::Array(v) => v,
            _ => &[],
        }
    }
    pub fn num(&self) -> f64 {
        match &self.val {
            Val::Prim(p) => p.as_f64(),
            _ => f64::NAN,
        }
    }
    pub fn uint(&self) -> u64 {
        match &self.val {
            Val::Prim(p) => p.as_u64(),
            _ => 0,
        }
    }
    pub fn is_pointerish(&self) -> bool {
        self.info != self.data
    }
}

pub struct Header {
    pub version: u32,
    pub instance_count: u32,
    pub instance_offset: u32,
    pub typedef_count: u32,
    pub typedef_offset: u32,
    pub stringhash_count: u32,
    pub stringhash_offset: u32,
    pub nametable_count: u32,
    pub nametable_offset: u32,
    pub total_size: u32,
}

pub struct Adf {
    pub header: Header,
    pub types: HashMap<u32, TypeDef>,
    pub instances: Vec<Instance>,
    pub roots: Vec<Node>,
}

pub const H_VERSION: usize = 4;
pub const H_INSTANCE_OFFSET: usize = 12;
pub const H_TYPEDEF_OFFSET: usize = 20;
pub const H_STRINGHASH_OFFSET: usize = 28;
pub const H_NAMETABLE_OFFSET: usize = 36;
pub const H_TOTAL_SIZE: usize = 40;

struct Rd<'a> {
    b: &'a [u8],
}

impl<'a> Rd<'a> {
    fn bytes(&self, p: usize, n: usize) -> Result<&'a [u8]> {
        self.b.get(p..p + n).ok_or_else(|| anyhow!("read past end at {p}+{n}"))
    }
    fn u8(&self, p: usize) -> Result<u8> {
        Ok(self.bytes(p, 1)?[0])
    }
    fn u16(&self, p: usize) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(p, 2)?.try_into()?))
    }
    fn u32(&self, p: usize) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(p, 4)?.try_into()?))
    }
    fn u64(&self, p: usize) -> Result<u64> {
        Ok(u64::from_le_bytes(self.bytes(p, 8)?.try_into()?))
    }
    fn strz(&self, p: usize) -> Result<String> {
        let end = self.b[p..].iter().position(|&c| c == 0).ok_or_else(|| anyhow!("unterminated string"))?;
        Ok(String::from_utf8_lossy(&self.b[p..p + end]).into_owned())
    }
}

pub fn read_u32(b: &[u8], p: usize) -> u32 {
    u32::from_le_bytes(b[p..p + 4].try_into().unwrap())
}
pub fn write_u32(b: &mut [u8], p: usize, v: u32) {
    b[p..p + 4].copy_from_slice(&v.to_le_bytes());
}

impl Adf {
    pub fn parse(b: &[u8]) -> Result<Adf> {
        let r = Rd { b };
        if b.len() < 0x40 {
            bail!("file too short");
        }
        if r.bytes(0, 4)? != b" FDA" {
            bail!("not an ADF file (magic mismatch)");
        }
        let h = Header {
            version: r.u32(4)?,
            instance_count: r.u32(8)?,
            instance_offset: r.u32(12)?,
            typedef_count: r.u32(16)?,
            typedef_offset: r.u32(20)?,
            stringhash_count: r.u32(24)?,
            stringhash_offset: r.u32(28)?,
            nametable_count: r.u32(32)?,
            nametable_offset: r.u32(36)?,
            total_size: r.u32(40)?,
        };
        // name table: N length bytes, then N zero-terminated strings
        let mut names = Vec::with_capacity(h.nametable_count as usize);
        let mut p = h.nametable_offset as usize;
        let lens: Vec<usize> = (0..h.nametable_count as usize)
            .map(|i| r.u8(p + i).map(|v| v as usize))
            .collect::<Result<_>>()?;
        p += lens.len();
        for l in lens {
            names.push(String::from_utf8_lossy(r.bytes(p, l)?).into_owned());
            p += l + 1;
        }
        let name = |i: u64| names.get(i as usize).cloned().unwrap_or_default();

        // type definitions
        let mut types = HashMap::new();
        let mut p = h.typedef_offset as usize;
        for _ in 0..h.typedef_count {
            let metatype = r.u32(p)?;
            let mut td = TypeDef {
                metatype,
                size: r.u32(p + 4)?,
                alignment: r.u32(p + 8)?,
                type_hash: r.u32(p + 12)?,
                name: name(r.u64(p + 16)?),
                element_type: r.u32(p + 28)?,
                element_length: r.u32(p + 32)?,
                members: vec![],
            };
            p += 36;
            match metatype {
                0 => {}
                1 => {
                    let n = r.u32(p)?;
                    p += 4;
                    for _ in 0..n {
                        let off = r.u32(p + 16)?;
                        td.members.push(Member {
                            name: name(r.u64(p)?),
                            type_hash: r.u32(p + 8)?,
                            size: r.u32(p + 12)?,
                            offset: off & 0x00ff_ffff,
                            bit_offset: (off >> 24) & 0xff,
                        });
                        p += 32;
                    }
                }
                2 | 3 | 4 | 7 | 9 => {
                    p += 4;
                }
                8 => {
                    let n = r.u32(p)?;
                    p += 4 + 12 * n as usize;
                }
                m => bail!("unknown ADF metatype {m}"),
            }
            types.insert(td.type_hash, td);
        }

        // instances
        let mut instances = vec![];
        let mut p = h.instance_offset as usize;
        for _ in 0..h.instance_count {
            instances.push(Instance {
                name_hash: r.u32(p)?,
                type_hash: r.u32(p + 4)?,
                offset: r.u32(p + 8)?,
                size: r.u32(p + 12)?,
                name: name(r.u64(p + 16)?),
                entry_pos: p,
            });
            p += 24;
        }

        let mut adf = Adf { header: h, types, instances, roots: vec![] };
        let mut roots = vec![];
        for ins in &adf.instances {
            let base = ins.offset as usize;
            let buf = r.bytes(base, ins.size as usize).context("instance out of range")?;
            let (n, _) = adf.read(&Rd { b: buf }, 0, ins.type_hash, base, 0)?;
            roots.push(n);
        }
        adf.roots = roots;
        Ok(adf)
    }

    /// Read a value at `pos` (relative to the instance buffer). `abs` = instance offset in file.
    fn read(&self, r: &Rd, pos: usize, t: u32, abs: usize, bit: u32) -> Result<(Node, usize)> {
        let node = |val, data: usize| Node { type_id: t, info: pos + abs, data: data + abs, val };
        if let Some(sz) = prim_size(t) {
            let raw = r.bytes(pos, sz)?;
            let v = match t {
                T_S8 => Prim::I(raw[0] as i8 as i64),
                T_U8 => Prim::U(raw[0] as u64),
                T_S16 => Prim::I(i16::from_le_bytes(raw.try_into()?) as i64),
                T_U16 => Prim::U(u16::from_le_bytes(raw.try_into()?) as u64),
                T_S32 => Prim::I(i32::from_le_bytes(raw.try_into()?) as i64),
                T_U32 => Prim::U(u32::from_le_bytes(raw.try_into()?) as u64),
                T_S64 => Prim::I(i64::from_le_bytes(raw.try_into()?)),
                T_U64 => Prim::U(u64::from_le_bytes(raw.try_into()?)),
                T_F32 => Prim::F(f32::from_le_bytes(raw.try_into()?) as f64),
                _ => Prim::F(f64::from_le_bytes(raw.try_into()?)),
            };
            return Ok((node(Val::Prim(v), pos), pos + sz));
        }
        if t == T_STRING {
            let off = r.u32(pos)? as usize;
            let s = r.strz(off)?;
            return Ok((node(Val::Str(s), off), pos + 8));
        }
        if t == T_DEFERRED {
            let off = r.u32(pos)? as usize;
            let ty = r.u32(pos + 8)?;
            if off == 0 || ty == 0 {
                return Ok((node(Val::Other, pos), pos + 16));
            }
            let inner = self.read(r, off, ty, abs, 0).map(|(n, _)| Val::Struct(IndexMap::from([("value".into(), n)])))
                .unwrap_or(Val::Other);
            return Ok((node(inner, off), pos + 16));
        }
        let td = self.types.get(&t).ok_or_else(|| anyhow!("missing ADF type {t:08x}"))?;
        match td.metatype {
            1 => {
                let mut m = IndexMap::new();
                for mem in &td.members {
                    let (v, _) = self.read(r, pos + mem.offset as usize, mem.type_hash, abs, mem.bit_offset)?;
                    m.insert(mem.name.clone(), v);
                }
                Ok((node(Val::Struct(m), pos), pos + td.size as usize))
            }
            3 | 4 => {
                let (off, len, next) = if td.metatype == 3 {
                    (r.u32(pos)? as usize, r.u32(pos + 8)? as usize, Some(pos + 12))
                } else {
                    (pos, td.element_length as usize, None)
                };
                let et = td.element_type;
                let (val, end) = if let Some(sz) = prim_size(et) {
                    (Val::Bytes(r.bytes(off, sz * len)?.to_vec()), off + sz * len)
                } else {
                    let mut items = Vec::with_capacity(len);
                    let mut p = off;
                    for _ in 0..len {
                        let (v, np) = self.read(r, p, et, abs, 0)?;
                        items.push(v);
                        p = np;
                    }
                    (Val::Array(items), p)
                };
                let after = next.unwrap_or(end);
                Ok((Node { type_id: t, info: pos + abs, data: off + abs, val }, after))
            }
            7 => {
                let v = match td.size {
                    1 => r.u8(pos)? as u64,
                    2 => r.u16(pos)? as u64,
                    4 => r.u32(pos)? as u64,
                    8 => r.u64(pos)?,
                    _ => bail!("bad bitfield size"),
                };
                Ok((node(Val::Prim(Prim::U((v >> bit) & 1)), pos), pos + td.size as usize))
            }
            8 => Ok((node(Val::Prim(Prim::U(r.u32(pos)? as u64)), pos), pos + 4)),
            9 => {
                let v = match td.size {
                    4 => r.u32(pos)? as u64,
                    8 => r.u64(pos)?,
                    _ => 0,
                };
                Ok((node(Val::Prim(Prim::U(v)), pos), pos + td.size as usize))
            }
            2 => Ok((node(Val::Prim(Prim::U(r.u64(pos)?)), pos), pos + 8)),
            _ => Ok((node(Val::Other, pos), pos + td.size as usize)),
        }
    }

    pub fn type_of(&self, t: u32) -> Option<&TypeDef> {
        self.types.get(&t)
    }

    /// Byte size of one element of an array node.
    pub fn element_size(&self, array: &Node) -> Result<usize> {
        let td = self.type_of(array.type_id).ok_or_else(|| anyhow!("array type missing"))?;
        let et = td.element_type;
        if let Some(s) = prim_size(et) {
            return Ok(s);
        }
        Ok(self.type_of(et).ok_or_else(|| anyhow!("element type missing"))?.size as usize)
    }
}

/// Walk every node, calling `f` for each.
pub fn walk<'a>(n: &'a Node, f: &mut impl FnMut(&'a Node)) {
    f(n);
    match &n.val {
        Val::Struct(m) => m.values().for_each(|c| walk(c, f)),
        Val::Array(v) => v.iter().for_each(|c| walk(c, f)),
        _ => {}
    }
}

/// Apply a size change of `delta` bytes at file position `at` (bytes not yet
/// inserted/removed). Mirrors APC's update of header + pointer offsets:
/// every pointer-like value whose target starts after `at` gets its stored
/// offset adjusted.
fn fixup(adf: &Adf, b: &mut [u8], at: usize, delta: i64) {
    let adj = |v: u32| (v as i64 + delta).max(0) as u32;
    let h = &adf.header;
    for (pos, val) in [
        (H_INSTANCE_OFFSET, h.instance_offset),
        (H_TYPEDEF_OFFSET, h.typedef_offset),
        (H_STRINGHASH_OFFSET, h.stringhash_offset),
        (H_NAMETABLE_OFFSET, h.nametable_offset),
        (H_TOTAL_SIZE, h.total_size),
    ] {
        if val > 0 && val as usize > at {
            write_u32(b, pos, adj(val));
        }
    }
    for (i, ins) in adf.instances.iter().enumerate() {
        let start = ins.offset as usize;
        let end = start + ins.size as usize;
        if start > at {
            write_u32(b, ins.entry_pos + 8, adj(ins.offset));
        } else if at <= end {
            write_u32(b, ins.entry_pos + 12, adj(ins.size));
        }
        // stored pointers are relative to the instance start; they only change
        // when the edit happens inside this instance
        if at < start || at > end {
            continue;
        }
        let mut fixes = vec![];
        walk(&adf.roots[i], &mut |n: &Node| {
            if n.is_pointerish() && n.data > at {
                fixes.push(n.info);
            }
        });
        for p in fixes {
            let cur = read_u32(b, p);
            write_u32(b, p, adj(cur));
        }
    }
}

/// Insert a copy of element `src_index` of `array` at the front of the array.
/// Returns the file offset of the new element (= array data start).
pub fn insert_clone_front(adf: &Adf, b: &mut Vec<u8>, array: &Node, src_index: usize) -> Result<usize> {
    let es = adf.element_size(array)?;
    let at = array.data;
    let src = array.items().get(src_index).ok_or_else(|| anyhow!("no element {src_index}"))?.data;
    fixup(adf, b, at, es as i64);
    // copy after fix-up so any pointers inside the element are already adjusted
    let bytes = b[src..src + es].to_vec();
    let len_pos = array.info + 8;
    let len = read_u32(b, len_pos);
    write_u32(b, len_pos, len + 1);
    b.splice(at..at, bytes);
    write_u32(b, H_VERSION, 3); // APC: ADFv3 avoids a crash on load
    Ok(at)
}

/// Remove element `index` of `array`.
pub fn remove_element(adf: &Adf, b: &mut Vec<u8>, array: &Node, index: usize) -> Result<()> {
    let es = adf.element_size(array)?;
    let el = array.items().get(index).ok_or_else(|| anyhow!("no element {index}"))?;
    let at = el.data;
    fixup(adf, b, at, -(es as i64));
    let len_pos = array.info + 8;
    let len = read_u32(b, len_pos);
    write_u32(b, len_pos, len.saturating_sub(1));
    b.drain(at..at + es);
    write_u32(b, H_VERSION, 3);
    Ok(())
}

/// Write a primitive at a node's position using the node's real width.
pub fn write_prim(b: &mut [u8], n: &Node, v: f64) -> Result<()> {
    let p = n.info;
    match n.type_id {
        T_U8 | T_S8 => b[p] = v as i64 as u8,
        T_U16 | T_S16 => b[p..p + 2].copy_from_slice(&(v as i64 as u16).to_le_bytes()),
        T_U32 | T_S32 => b[p..p + 4].copy_from_slice(&(v as i64 as u32).to_le_bytes()),
        T_U64 | T_S64 => b[p..p + 8].copy_from_slice(&(v as i64 as u64).to_le_bytes()),
        T_F32 => b[p..p + 4].copy_from_slice(&(v as f32).to_le_bytes()),
        T_F64 => b[p..p + 8].copy_from_slice(&v.to_le_bytes()),
        // bitfield / enum flags (e.g. FeatureModifiers.Flags): APC writes a u32 here
        _ => b[p..p + 4].copy_from_slice(&(v as i64 as u32).to_le_bytes()),
    }
    Ok(())
}

pub fn write_u32_exact(b: &mut [u8], n: &Node, v: u32) -> Result<()> {
    match n.type_id {
        T_U32 | T_S32 => {
            write_u32(b, n.info, v);
            Ok(())
        }
        _ => write_prim(b, n, v as f64),
    }
}
