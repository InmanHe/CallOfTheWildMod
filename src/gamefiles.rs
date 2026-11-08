//! Read-only access to the installed game's archives (TAB v3 + ARC, AAF
//! compression, SARC bundles, RTPC) and discovery of every weapon / ammo
//! ballistic definition. Formats as documented by deca (Krzysztof
//! Kamieniecki, MIT). Nothing is ever written to the game folder.

use crate::adf::{Adf, Node};
use crate::gamedata::{Ammo, GameData, Tuning, Weapon, Zero};
use anyhow::{anyhow, bail, Context, Result};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

// ------------------------------------------------------------------ hash

fn rot(x: u32, k: u32) -> u32 {
    x.rotate_left(k)
}

/// Bob Jenkins' lookup3 hashlittle2 (primary result), as used for Apex paths.
pub fn hash32(data: &[u8]) -> u32 {
    let len = data.len();
    let mut a = 0xdeadbeefu32.wrapping_add(len as u32);
    let mut b = a;
    let mut c = a;
    let w = |p: usize, n: usize| -> u32 {
        let mut v = 0u32;
        for i in 0..n {
            v |= (data[p + i] as u32) << (8 * i);
        }
        v
    };
    let mut p = 0;
    let mut rem = len;
    while rem > 12 {
        a = a.wrapping_add(w(p, 4));
        b = b.wrapping_add(w(p + 4, 4));
        c = c.wrapping_add(w(p + 8, 4));
        a = a.wrapping_sub(c); a ^= rot(c, 4); c = c.wrapping_add(b);
        b = b.wrapping_sub(a); b ^= rot(a, 6); a = a.wrapping_add(c);
        c = c.wrapping_sub(b); c ^= rot(b, 8); b = b.wrapping_add(a);
        a = a.wrapping_sub(c); a ^= rot(c, 16); c = c.wrapping_add(b);
        b = b.wrapping_sub(a); b ^= rot(a, 19); a = a.wrapping_add(c);
        c = c.wrapping_sub(b); c ^= rot(b, 4); b = b.wrapping_add(a);
        p += 12;
        rem -= 12;
    }
    if rem == 0 {
        return c;
    }
    a = a.wrapping_add(w(p, rem.min(4)));
    if rem > 4 {
        b = b.wrapping_add(w(p + 4, (rem - 4).min(4)));
    }
    if rem > 8 {
        c = c.wrapping_add(w(p + 8, rem - 8));
    }
    c ^= b; c = c.wrapping_sub(rot(b, 14));
    a ^= c; a = a.wrapping_sub(rot(c, 11));
    b ^= a; b = b.wrapping_sub(rot(a, 25));
    c ^= b; c = c.wrapping_sub(rot(b, 16));
    a ^= c; a = a.wrapping_sub(rot(c, 4));
    b ^= a; b = b.wrapping_sub(rot(a, 14));
    c ^= b; c = c.wrapping_sub(rot(b, 24));
    c
}

// ------------------------------------------------------------------ AAF

const AAF_HEADER: usize = 48;

/// Decompress an AAF container (sections of raw deflate). Non-AAF data is returned as is.
pub fn aaf_decompress(b: &[u8]) -> Result<Vec<u8>> {
    if !b.starts_with(b"AAF\0") {
        return Ok(b.to_vec());
    }
    let rd = |p: usize| -> Result<u32> { Ok(u32::from_le_bytes(b.get(p..p + 4).ok_or_else(|| anyhow!("AAF truncated"))?.try_into()?)) };
    let total = rd(36)? as usize;
    let count = rd(44)? as usize;
    let mut out = Vec::with_capacity(total);
    let mut p = AAF_HEADER;
    for _ in 0..count {
        let clen = rd(p)? as usize;
        let ulen = rd(p + 4)? as usize;
        let padded = rd(p + 8)? as usize;
        let data = b.get(p + 16..p + 16 + clen).ok_or_else(|| anyhow!("AAF section truncated"))?;
        let mut d = flate2::read::DeflateDecoder::new(data);
        let start = out.len();
        d.read_to_end(&mut out).context("AAF inflate")?;
        if out.len() - start > ulen {
            bail!("AAF section larger than declared");
        }
        p += padded;
    }
    Ok(out)
}

/// Inflate just the first `want` bytes of an AAF container (for peeking at headers).
fn aaf_prefix(b: &[u8], want: usize) -> Option<Vec<u8>> {
    if !b.starts_with(b"AAF\0") || b.len() < AAF_HEADER + 16 {
        return None;
    }
    let mut d = flate2::read::DeflateDecoder::new(&b[AAF_HEADER + 16..]);
    let mut out = vec![0u8; want];
    let mut got = 0;
    while got < want {
        match d.read(&mut out[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(_) => break,
        }
    }
    out.truncate(got);
    Some(out)
}

// ------------------------------------------------------------------ SARC

#[derive(Debug, Clone)]
pub struct SarcEntry {
    pub name: String,
    pub offset: u32,
    pub length: u32,
}

/// Parse a SARC directory (v2 or v3). `b` needs at least 16 + dir length bytes.
pub fn sarc_dir(b: &[u8]) -> Option<Vec<SarcEntry>> {
    if b.len() < 16 || &b[4..8] != b"SARC" || u32::from_le_bytes(b[0..4].try_into().ok()?) != 4 {
        return None;
    }
    let ver2 = u32::from_le_bytes(b[8..12].try_into().ok()?);
    let dir_len = u32::from_le_bytes(b[12..16].try_into().ok()?) as usize;
    let rd = |p: usize| -> Option<u32> { Some(u32::from_le_bytes(b.get(p..p + 4)?.try_into().ok()?)) };
    let mut out = vec![];
    match ver2 {
        2 => {
            let end = (16 + dir_len).min(b.len());
            let mut p = 16;
            while p + 12 <= end {
                let n = rd(p)? as usize;
                let raw = b.get(p + 4..p + 4 + n)?;
                let name = String::from_utf8_lossy(raw).trim_end_matches('\0').to_string();
                p += 4 + n;
                let offset = rd(p)?;
                let length = rd(p + 4)?;
                p += 8;
                if name.is_empty() {
                    break;
                }
                out.push(SarcEntry { name, offset, length });
            }
        }
        3 => {
            let slen = rd(16)? as usize;
            let strings = b.get(20..20 + slen)?;
            let names: Vec<String> = strings.split(|&c| c == 0).filter(|s| !s.is_empty()).map(|s| String::from_utf8_lossy(s).into_owned()).collect();
            let mut p = 20 + slen;
            for name in names {
                let offset = rd(p + 4)?;
                let length = rd(p + 8)?;
                out.push(SarcEntry { name, offset, length });
                p += 20;
            }
        }
        _ => return None,
    }
    Some(out)
}

fn sarc_dir_len(b: &[u8]) -> Option<usize> {
    if b.len() >= 16 && &b[4..8] == b"SARC" {
        Some(16 + u32::from_le_bytes(b[12..16].try_into().ok()?) as usize)
    } else {
        None
    }
}

// ------------------------------------------------------------------ RTPC

#[derive(Debug, Clone)]
pub enum RVal {
    U32(u32),
    F32(f32),
    Str(String),
    Other,
}

#[derive(Debug, Clone)]
pub struct RNode {
    pub name: u32,
    pub props: Vec<(u32, RVal)>,
    pub children: Vec<RNode>,
}

impl RNode {
    pub fn str(&self, h: u32) -> Option<&str> {
        self.props.iter().find(|p| p.0 == h).and_then(|p| if let RVal::Str(s) = &p.1 { Some(s.as_str()) } else { None })
    }
}

pub fn rtpc_parse(b: &[u8]) -> Result<RNode> {
    if !b.starts_with(b"RTPC") {
        bail!("not RTPC");
    }
    fn node(b: &[u8], p: usize, depth: usize) -> Result<RNode> {
        if depth > 64 {
            bail!("RTPC too deep");
        }
        let rd = |p: usize| -> Result<u32> { Ok(u32::from_le_bytes(b.get(p..p + 4).ok_or_else(|| anyhow!("RTPC truncated"))?.try_into()?)) };
        let name = rd(p)?;
        let off = rd(p + 4)? as usize;
        let pc = u16::from_le_bytes(b.get(p + 8..p + 10).ok_or_else(|| anyhow!("RTPC"))?.try_into()?) as usize;
        let cc = u16::from_le_bytes(b.get(p + 10..p + 12).ok_or_else(|| anyhow!("RTPC"))?.try_into()?) as usize;
        let mut props = Vec::with_capacity(pc);
        let mut q = off;
        for _ in 0..pc {
            let h = rd(q)?;
            let raw = rd(q + 4)?;
            let ty = *b.get(q + 8).ok_or_else(|| anyhow!("RTPC"))?;
            let v = match ty {
                1 => RVal::U32(raw),
                2 => RVal::F32(f32::from_bits(raw)),
                3 => {
                    let s = b.get(raw as usize..).ok_or_else(|| anyhow!("RTPC str"))?;
                    let e = s.iter().position(|&c| c == 0).unwrap_or(s.len());
                    RVal::Str(String::from_utf8_lossy(&s[..e]).into_owned())
                }
                _ => RVal::Other,
            };
            props.push((h, v));
            q += 9;
        }
        q += (4 - q % 4) % 4;
        let mut children = Vec::with_capacity(cc);
        for i in 0..cc {
            children.push(node(b, q + 12 * i, depth + 1)?);
        }
        Ok(RNode { name, props, children })
    }
    node(b, 8, 0)
}

// ------------------------------------------------------------------ archives

#[derive(Debug, Clone, Copy)]
pub struct Entry {
    pub arc: u32,
    pub offset: u32,
    pub size: u32,
}

pub struct Archives {
    pub arcs: Vec<PathBuf>,
    pub entries: HashMap<u32, Entry>,
    pub all: Vec<Entry>,
}

fn find_tabs(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        let mut v: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        v.sort();
        for p in v {
            if p.is_dir() {
                find_tabs(&p, out);
            } else if p.extension().map(|e| e.eq_ignore_ascii_case("tab")).unwrap_or(false) {
                out.push(p);
            }
        }
    }
}

impl Archives {
    pub fn open(archives_dir: &Path) -> Result<Archives> {
        let mut tabs = vec![];
        find_tabs(archives_dir, &mut tabs);
        if tabs.is_empty() {
            bail!("no .tab files in {}", archives_dir.display());
        }
        let mut a = Archives { arcs: vec![], entries: HashMap::new(), all: vec![] };
        for tab in tabs {
            let arc = tab.with_extension("arc");
            if !arc.exists() {
                continue;
            }
            let b = std::fs::read(&tab)?;
            if b.len() < 12 || &b[0..4] != b"TAB\0" {
                continue;
            }
            let v0 = u16::from_le_bytes([b[4], b[5]]);
            let v1 = u16::from_le_bytes([b[6], b[7]]);
            if !(v0 == 2 && v1 == 1) {
                bail!("unsupported TAB version {v0}.{v1} in {}", tab.display());
            }
            let idx = a.arcs.len() as u32;
            a.arcs.push(arc);
            for rec in b[12..].chunks_exact(12) {
                let h = u32::from_le_bytes(rec[0..4].try_into().unwrap());
                let e = Entry { arc: idx, offset: u32::from_le_bytes(rec[4..8].try_into().unwrap()), size: u32::from_le_bytes(rec[8..12].try_into().unwrap()) };
                a.all.push(e);
                // later archives (patches / DLC, sorted after the base) win
                a.entries.insert(h, e);
            }
        }
        Ok(a)
    }

    fn read_at(&self, files: &mut HashMap<u32, File>, e: Entry, len: usize) -> Result<Vec<u8>> {
        let f = match files.entry(e.arc) {
            std::collections::hash_map::Entry::Occupied(o) => o.into_mut(),
            std::collections::hash_map::Entry::Vacant(v) => v.insert(File::open(&self.arcs[e.arc as usize])?),
        };
        f.seek(SeekFrom::Start(e.offset as u64))?;
        let mut buf = vec![0u8; len.min(e.size as usize)];
        f.read_exact(&mut buf)?;
        Ok(buf)
    }

    pub fn read(&self, e: Entry) -> Result<Vec<u8>> {
        let mut files = HashMap::new();
        aaf_decompress(&self.read_at(&mut files, e, e.size as usize)?)
    }

    pub fn read_path(&self, vpath: &str) -> Option<Vec<u8>> {
        let e = *self.entries.get(&hash32(vpath.as_bytes()))?;
        self.read(e).ok()
    }
}

// ------------------------------------------------------------------ discovery

pub struct Progress {
    pub done: AtomicU64,
    pub total: AtomicU64,
}

impl Default for Progress {
    fn default() -> Self {
        Progress { done: AtomicU64::new(0), total: AtomicU64::new(1) }
    }
}

const P_NAME: u32 = 0xd31ab684;
const P_DISPLAY: u32 = 0x2314c9ea;
const CAT_AMMO: u32 = 0x1f3f2b00;
const CAT_WEAPON: u32 = 0xc04b7961;

fn f(n: &Node, k: &str) -> f64 {
    n.field(k).map(|v| v.num()).unwrap_or(0.0)
}

fn parse_tuning(b: &[u8]) -> Option<Tuning> {
    let adf = Adf::parse(b).ok()?;
    let r = adf.roots.first()?;
    let zs = r.field("ZeroingSettings")?;
    let bt = r.field("bullet_weapon")?.items().first()?.field("tuning")?;
    let bbt = bt.field("bullet_base_tuning")?;
    let sc = bt.field("base_tuning").and_then(|x| x.field("scatter"));
    let zeros = zs.items().iter().map(|z| Zero {
        distance: f(z, "zero_distance"),
        angle: f(z, "angle"),
        h_angle: f(z, "horizontal_angle"),
        overrides: BTreeMap::new(),
        override_hashes: z.field("BulletOverrides").map(|o| o.items().iter().map(|x| (x.field("BulletName").map(|n| n.uint() as u32).unwrap_or(0), f(x, "Angle"))).collect()).unwrap_or_default(),
    }).collect();
    Some(Tuning {
        ke: f(r, "kinetic_energy"),
        gravity: f(bbt, "gravity_strength"),
        gravity_on: f(bbt, "gravity_on") as i64,
        zeros,
        scatter_min: sc.map(|s| f(s, "scatter_minimum")).unwrap_or(0.0),
        scatter_max: sc.map(|s| f(s, "scatter_maximum")).unwrap_or(0.0),
    })
}

fn parse_ammo(b: &[u8], key: &str) -> Option<Ammo> {
    let adf = Adf::parse(b).ok()?;
    let r = adf.roots.first()?;
    r.field("drag_coefficient")?;
    Some(Ammo {
        display: key.to_string(),
        mass: f(r, "mass"),
        diameter: f(r, "diameter"),
        cd: f(r, "drag_coefficient"),
        ke: f(r, "kinetic_energy"),
        max_range: f(r, "max_range"),
        velocity_deviance: f(r, "velocity_deviance"),
        scatter_multiplier: f(r, "scatter_multiplier"),
        projectiles: f(r, "projectiles_per_shot") as i64,
        hash: hash32(key.as_bytes()),
    })
}

fn stem(path: &str) -> String {
    let file = path.rsplit('/').next().unwrap_or(path);
    file.split('.').next().unwrap_or(file).to_string()
}

fn weapon_names(b: &[u8], out: &mut HashSet<String>) {
    let pat = b"equipment_weapon_";
    let mut i = 0;
    while let Some(pos) = b[i..].windows(pat.len()).position(|w| w == pat) {
        let s = i + pos;
        let mut e = s + pat.len();
        while e < b.len() && (b[e].is_ascii_lowercase() || b[e].is_ascii_digit() || b[e] == b'_') {
            e += 1;
        }
        out.insert(String::from_utf8_lossy(&b[s..e]).into_owned());
        i = e;
    }
}

/// Discover all weapons and ammo in the game's archives.
pub fn build(archives_dir: &Path, prog: &Progress) -> Result<GameData> {
    let ar = Archives::open(archives_dir)?;
    // equipment database
    let eq = ar.read_path("settings/hp_settings/equipment_data.bin").ok_or_else(|| anyhow!("equipment_data.bin not found"))?;
    let eq = rtpc_parse(&eq)?;
    let mut weapons_meta: BTreeMap<String, String> = BTreeMap::new();
    let mut ammo_meta: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
    for cat in &eq.children {
        for it in &cat.children {
            let Some(name) = it.str(P_NAME) else { continue };
            let display = it.str(P_DISPLAY).unwrap_or(name).to_string();
            if cat.name == CAT_WEAPON {
                weapons_meta.insert(name.to_string(), display);
            } else if cat.name == CAT_AMMO {
                fn collect(n: &RNode, out: &mut Vec<String>) {
                    for (_, v) in &n.props {
                        if let RVal::Str(s) = v {
                            if s.starts_with("equipment_weapon_") {
                                out.push(s.clone());
                            }
                        }
                    }
                    n.children.iter().for_each(|c| collect(c, out));
                }
                let mut comp = vec![];
                it.children.iter().for_each(|c| collect(c, &mut comp));
                ammo_meta.insert(name.to_string(), (display, comp));
            }
        }
    }

    // scan every archive entry, peeking at SARC directories
    prog.total.store(ar.all.len() as u64, Ordering::Relaxed);
    let mut files: HashMap<u32, File> = HashMap::new();
    let mut candidates: Vec<Entry> = vec![];
    let mut seen: HashSet<(u32, u32)> = HashSet::new();
    for (i, e) in ar.all.iter().enumerate() {
        prog.done.store(i as u64, Ordering::Relaxed);
        if e.size < 32 || e.size > 256 << 20 || !seen.insert((e.arc, e.offset)) {
            continue;
        }
        let Ok(head) = ar.read_at(&mut files, *e, 64) else { continue };
        let dir = if &head[4..8] == b"SARC" {
            let n = sarc_dir_len(&head).unwrap_or(0).min(4 << 20);
            ar.read_at(&mut files, *e, n).ok().and_then(|b| sarc_dir(&b))
        } else if head.starts_with(b"AAF\0") {
            let first = ar.read_at(&mut files, *e, 64 << 10).ok();
            first.and_then(|raw| {
                let p = aaf_prefix(&raw, 16)?;
                let n = sarc_dir_len(&p)?.min(4 << 20);
                let raw = if n > 48 << 10 { ar.read_at(&mut files, *e, (n * 2 + 4096).min(e.size as usize)).ok()? } else { raw };
                sarc_dir(&aaf_prefix(&raw, n)?)
            })
        } else {
            None
        };
        if let Some(d) = dir {
            if d.iter().any(|x| (x.name.ends_with(".wtunec") && x.name.contains("/tuning/")) || x.name.ends_with(".ammotunec")) {
                candidates.push(*e);
            }
        }
    }

    let mut tunings: BTreeMap<String, Tuning> = BTreeMap::new();
    let mut ammo: BTreeMap<String, Ammo> = BTreeMap::new();
    let mut weapon_tuning: BTreeMap<String, String> = BTreeMap::new();
    for e in candidates {
        let Ok(b) = ar.read(e) else { continue };
        let Some(dir) = sarc_dir(&b) else { continue };
        // inline data if present and complete, else the loose file (symlink / patch)
        let get = |x: &SarcEntry| -> Option<Vec<u8>> {
            let inline = (x.offset > 0).then(|| b.get(x.offset as usize..x.offset as usize + x.length as usize)).flatten();
            match inline {
                Some(s) => Some(s.to_vec()),
                None => ar.read_path(&x.name),
            }
        };
        let mut main_tuning: Option<String> = None;
        for x in &dir {
            if x.name.ends_with(".wtunec") && x.name.contains("/tuning/") {
                if tunings.contains_key(&x.name) {
                    main_tuning.get_or_insert(x.name.clone());
                    continue;
                }
                if let Some(t) = get(x).and_then(|d| parse_tuning(&d)) {
                    tunings.insert(x.name.clone(), t);
                    main_tuning.get_or_insert(x.name.clone());
                }
            } else if x.name.ends_with(".ammotunec") {
                let key = stem(&x.name);
                if !ammo.contains_key(&key) {
                    if let Some(a) = get(x).and_then(|d| parse_ammo(&d, &key)) {
                        ammo.insert(key, a);
                    }
                }
            }
        }
        if let Some(t) = main_tuning {
            let mut names = HashSet::new();
            weapon_names(&b, &mut names);
            for n in names {
                if weapons_meta.contains_key(&n) {
                    weapon_tuning.entry(n).or_insert(t.clone());
                }
            }
        }
    }

    // resolve per-ammo zero overrides (BulletName = hash of the ammo name)
    let by_hash: HashMap<u32, String> = ammo.iter().map(|(k, a)| (a.hash, k.clone())).collect();
    for t in tunings.values_mut() {
        for z in &mut t.zeros {
            for (h, ang) in std::mem::take(&mut z.override_hashes) {
                if let Some(k) = by_hash.get(&h) {
                    z.overrides.insert(k.clone(), ang);
                }
            }
        }
    }
    for (k, a) in ammo.iter_mut() {
        if let Some((d, _)) = ammo_meta.get(k) {
            a.display = d.clone();
        }
    }
    let mut weapons = BTreeMap::new();
    for (name, display) in &weapons_meta {
        let Some(t) = weapon_tuning.get(name) else { continue };
        let mut am: Vec<String> = ammo_meta.iter().filter(|(k, (_, c))| ammo.contains_key(*k) && c.contains(name)).map(|(k, _)| k.clone()).collect();
        am.sort();
        if am.is_empty() {
            continue;
        }
        weapons.insert(name.clone(), Weapon { display: display.clone(), tuning: t.clone(), ammo: am, hash: hash32(name.as_bytes()) });
    }
    prog.done.store(ar.all.len() as u64, Ordering::Relaxed);
    if weapons.is_empty() {
        bail!("no weapons found in the archives");
    }
    let mut g = GameData { source: format!("game files: {}", archives_dir.display()), weapons, ammo, tunings, by_hash: HashMap::new() };
    g.index();
    Ok(g)
}

/// Fingerprint of the archive set (paths, sizes, mtimes) to know when to rebuild.
pub fn fingerprint(archives_dir: &Path) -> String {
    let mut tabs = vec![];
    find_tabs(archives_dir, &mut tabs);
    let mut h = 0u64;
    for t in &tabs {
        let m = std::fs::metadata(t).ok();
        let s = format!("{}|{}|{:?}", t.display(), m.as_ref().map(|m| m.len()).unwrap_or(0), m.and_then(|m| m.modified().ok()));
        h = h.wrapping_mul(1_000_003) ^ hash32(s.as_bytes()) as u64;
    }
    format!("{}-{h:016x}", tabs.len())
}

// ------------------------------------------------------------------ locating the game

fn has_archives(dir: &Path) -> Option<PathBuf> {
    let a = dir.join("archives_win64");
    a.is_dir().then_some(a)
}

/// Candidate game folders: running process, Steam libraries, Epic default.
pub fn locate_game() -> Vec<PathBuf> {
    #[allow(unused_mut)]
    let mut out: Vec<PathBuf> = vec![];
    #[cfg(windows)]
    {
        if let Some(p) = crate::memory::win::running_exe_path(crate::memory::GAME_EXE) {
            if let Some(d) = p.parent() {
                out.push(d.to_path_buf());
            }
        }
        let mut steam_roots: Vec<PathBuf> = vec![];
        if let Some(s) = crate::memory::win::steam_path() {
            steam_roots.push(s);
        }
        for d in ["C:\\Program Files (x86)\\Steam", "C:\\Program Files\\Steam", "D:\\Steam", "D:\\SteamLibrary", "E:\\SteamLibrary"] {
            steam_roots.push(PathBuf::from(d));
        }
        let mut libs: Vec<PathBuf> = vec![];
        for r in &steam_roots {
            libs.push(r.clone());
            if let Ok(t) = std::fs::read_to_string(r.join("steamapps").join("libraryfolders.vdf")) {
                for line in t.lines() {
                    let l = line.trim();
                    if l.starts_with("\"path\"") {
                        if let Some(v) = l.split('"').nth(3) {
                            libs.push(PathBuf::from(v.replace("\\\\", "\\")));
                        }
                    }
                }
            }
        }
        for l in libs {
            out.push(l.join("steamapps").join("common").join("theHunterCotW"));
        }
        out.push(PathBuf::from("C:\\Program Files\\Epic Games\\theHunterCotW"));
    }
    let mut seen = HashSet::new();
    out.into_iter().filter(|p| has_archives(p).is_some() && seen.insert(p.clone())).collect()
}

pub fn archives_dir(game_dir: &Path) -> Option<PathBuf> {
    has_archives(game_dir).or_else(|| (game_dir.file_name().map(|n| n == "archives_win64").unwrap_or(false) && game_dir.is_dir()).then(|| game_dir.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_matches_deca() {
        // values computed with deca's hash32_func
        assert_eq!(hash32(b"equipment_ammo_bh_bow_arrow_420gr_01"), 968892393);
        assert_eq!(hash32(b"equipment_ammo_jp_bow_arrow_300gr_01"), 3926360921);
        assert_eq!(hash32(b"editor/entities/hp_weapons/weapon_bows_01/tuning/equipment_weapon_compound_bow_01.wtunec"), 0x81A2CD33);
        assert_eq!(hash32(b""), 0xdeadbeef);
    }
}
