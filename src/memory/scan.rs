//! Cheat-Engine-style value scanner.
//!
//! Two result representations:
//! * `List`     — explicit (address, previous value) pairs, for known-value
//!                scans and once results get small.
//! * `Snapshot` — for "unknown initial value" scans over gigabytes: previous
//!                values live in a file on disk and candidates are a bitmap
//!                (1 bit per aligned slot). Converted to a list once the
//!                candidate count drops below `LIST_THRESHOLD`.

use super::{Process, Region};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub const LIST_CAP: usize = 30_000_000;
pub const LIST_THRESHOLD: u64 = 5_000_000;
const CHUNK: usize = 4 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VType {
    U8,
    I16,
    I32,
    I64,
    F32,
    F64,
}

impl VType {
    pub const ALL: [VType; 6] = [VType::F32, VType::I32, VType::F64, VType::I64, VType::I16, VType::U8];
    pub fn size(self) -> usize {
        match self {
            VType::U8 => 1,
            VType::I16 => 2,
            VType::I32 | VType::F32 => 4,
            VType::I64 | VType::F64 => 8,
        }
    }
    pub fn is_float(self) -> bool {
        matches!(self, VType::F32 | VType::F64)
    }
    pub fn name(self) -> &'static str {
        match self {
            VType::U8 => "Byte",
            VType::I16 => "2 Bytes",
            VType::I32 => "4 Bytes",
            VType::I64 => "8 Bytes",
            VType::F32 => "Float",
            VType::F64 => "Double",
        }
    }
    pub fn decode(self, b: &[u8]) -> f64 {
        match self {
            VType::U8 => b[0] as f64,
            VType::I16 => i16::from_le_bytes([b[0], b[1]]) as f64,
            VType::I32 => i32::from_le_bytes(b[..4].try_into().unwrap()) as f64,
            VType::I64 => i64::from_le_bytes(b[..8].try_into().unwrap()) as f64,
            VType::F32 => f32::from_le_bytes(b[..4].try_into().unwrap()) as f64,
            VType::F64 => f64::from_le_bytes(b[..8].try_into().unwrap()),
        }
    }
    /// Raw bits (zero-extended) for exact comparisons of integers / ids.
    pub fn raw(self, b: &[u8]) -> u64 {
        let mut x = [0u8; 8];
        x[..self.size()].copy_from_slice(&b[..self.size()]);
        u64::from_le_bytes(x)
    }
    pub fn encode(self, v: f64) -> Vec<u8> {
        match self {
            VType::U8 => vec![v as i64 as u8],
            VType::I16 => (v as i64 as i16).to_le_bytes().to_vec(),
            VType::I32 => (v as i64 as i32).to_le_bytes().to_vec(),
            VType::I64 => (v as i64).to_le_bytes().to_vec(),
            VType::F32 => (v as f32).to_le_bytes().to_vec(),
            VType::F64 => v.to_le_bytes().to_vec(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cmp {
    Exact(f64),
    Between(f64, f64),
    Unknown,
    Increased,
    Decreased,
    Changed,
    Unchanged,
}

impl Cmp {
    fn needs_prev(&self) -> bool {
        matches!(self, Cmp::Increased | Cmp::Decreased | Cmp::Changed | Cmp::Unchanged)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Hit {
    pub addr: u64,
    pub prev: u64,
}

enum State {
    Empty,
    List(Vec<Hit>),
    Snapshot { path: PathBuf, regions: Vec<Region>, bitmap: Vec<u64>, count: u64 },
}

pub struct Progress {
    pub done: AtomicU64,
    pub total: AtomicU64,
    pub cancel: AtomicBool,
}

impl Default for Progress {
    fn default() -> Self {
        Progress { done: AtomicU64::new(0), total: AtomicU64::new(1), cancel: AtomicBool::new(false) }
    }
}

impl Progress {
    pub fn fraction(&self) -> f32 {
        self.done.load(Ordering::Relaxed) as f32 / self.total.load(Ordering::Relaxed).max(1) as f32
    }
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

pub struct Scanner {
    pub vtype: VType,
    pub tolerance: f64,
    pub writable_only: bool,
    state: State,
    scans: usize,
}

fn matches(vt: VType, tol: f64, cmp: Cmp, cur: &[u8], prev: u64) -> bool {
    let v = vt.decode(cur);
    if vt.is_float() && !v.is_finite() {
        return false;
    }
    let pv = || {
        let b = prev.to_le_bytes();
        vt.decode(&b)
    };
    match cmp {
        Cmp::Exact(x) => {
            if vt.is_float() { (v - x).abs() <= tol } else { v == x.round() }
        }
        Cmp::Between(a, b) => v >= a.min(b) && v <= a.max(b),
        Cmp::Unknown => true,
        Cmp::Increased => v > pv(),
        Cmp::Decreased => v < pv(),
        Cmp::Changed => vt.raw(cur) != prev,
        Cmp::Unchanged => vt.raw(cur) == prev,
    }
}

fn scan_regions(p: &dyn Process, writable_only: bool) -> Vec<Region> {
    let module = p.main_module();
    p.regions()
        .into_iter()
        .filter(|r| r.writable || !writable_only || module.map(|(b, s)| r.base >= b && r.end() <= b + s).unwrap_or(false))
        .filter(|r| r.size <= (2u64 << 30))
        .collect()
}

impl Scanner {
    pub fn new(vtype: VType) -> Self {
        Scanner { vtype, tolerance: 0.01, writable_only: true, state: State::Empty, scans: 0 }
    }

    pub fn reset(&mut self) {
        if let State::Snapshot { path, .. } = &self.state {
            let _ = std::fs::remove_file(path);
        }
        self.state = State::Empty;
        self.scans = 0;
    }

    pub fn scans_done(&self) -> usize {
        self.scans
    }

    pub fn count(&self) -> u64 {
        match &self.state {
            State::Empty => 0,
            State::List(v) => v.len() as u64,
            State::Snapshot { count, .. } => *count,
        }
    }

    pub fn has_results(&self) -> bool {
        !matches!(self.state, State::Empty)
    }

    /// First `max` results (address + previous raw value). In snapshot mode
    /// only addresses are known cheaply; prev is 0.
    pub fn hits(&self, max: usize) -> Vec<Hit> {
        match &self.state {
            State::Empty => vec![],
            State::List(v) => v.iter().take(max).copied().collect(),
            State::Snapshot { regions, bitmap, .. } => {
                let mut out = vec![];
                let al = self.vtype.size() as u64;
                let mut bit = 0u64;
                'outer: for r in regions {
                    let slots = r.size / al;
                    for s in 0..slots {
                        let i = bit + s;
                        if bitmap[(i / 64) as usize] >> (i % 64) & 1 == 1 {
                            out.push(Hit { addr: r.base + s * al, prev: 0 });
                            if out.len() >= max {
                                break 'outer;
                            }
                        }
                    }
                    bit += slots;
                }
                out
            }
        }
    }

    fn snapshot_path() -> PathBuf {
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        std::env::temp_dir().join(format!("cotwmod_snapshot_{n}.bin"))
    }

    pub fn first_scan(&mut self, p: &dyn Process, cmp: Cmp, prog: &Progress) -> Result<u64> {
        self.reset();
        if cmp.needs_prev() {
            bail!("first scan needs a value, a range or 'unknown'");
        }
        let regions = scan_regions(p, self.writable_only);
        prog.total.store(regions.iter().map(|r| r.size).sum::<u64>().max(1), Ordering::Relaxed);
        prog.done.store(0, Ordering::Relaxed);
        let vt = self.vtype;
        let al = vt.size();
        if cmp == Cmp::Unknown {
            if al < 4 {
                bail!("unknown-value scans need a 4 or 8 byte type");
            }
            let path = Self::snapshot_path();
            let mut w = BufWriter::with_capacity(8 << 20, File::create(&path)?);
            let total_slots: u64 = regions.iter().map(|r| r.size / al as u64).sum();
            let mut bitmap = vec![u64::MAX; total_slots.div_ceil(64) as usize];
            let mut count = 0u64;
            let mut bit = 0u64;
            let mut buf = vec![0u8; CHUNK];
            for r in &regions {
                let slots = r.size / al as u64;
                let mut off = 0u64;
                while off < r.size {
                    if prog.cancelled() {
                        drop(w);
                        let _ = std::fs::remove_file(&path);
                        bail!("cancelled");
                    }
                    let n = (CHUNK as u64).min(r.size - off) as usize;
                    let b = &mut buf[..n];
                    let ok = p.read(r.base + off, b);
                    if !ok {
                        b.fill(0);
                    }
                    w.write_all(b)?;
                    for s in 0..(n / al) as u64 {
                        let i = bit + off / al as u64 + s;
                        let valid = ok && {
                            let c = &b[s as usize * al..s as usize * al + al];
                            !vt.is_float() || vt.decode(c).is_finite()
                        };
                        if valid {
                            count += 1;
                        } else {
                            bitmap[(i / 64) as usize] &= !(1u64 << (i % 64));
                        }
                    }
                    off += n as u64;
                    prog.done.fetch_add(n as u64, Ordering::Relaxed);
                }
                bit += slots;
            }
            // clear padding bits past the last slot
            for i in total_slots..bitmap.len() as u64 * 64 {
                bitmap[(i / 64) as usize] &= !(1u64 << (i % 64));
            }
            w.flush()?;
            self.state = State::Snapshot { path, regions, bitmap, count };
        } else {
            let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(16);
            let tol = self.tolerance;
            let parts: Vec<Vec<Region>> = (0..threads).map(|t| regions.iter().skip(t).step_by(threads).copied().collect()).collect();
            let overflow = AtomicBool::new(false);
            let mut all: Vec<Hit> = std::thread::scope(|sc| {
                let hs: Vec<_> = parts
                    .iter()
                    .map(|rs| {
                        let overflow = &overflow;
                        sc.spawn(move || {
                            let mut out = vec![];
                            let mut buf = vec![0u8; CHUNK];
                            for r in rs {
                                let mut off = 0u64;
                                while off < r.size {
                                    if prog.cancelled() || overflow.load(Ordering::Relaxed) {
                                        return out;
                                    }
                                    let n = (CHUNK as u64).min(r.size - off) as usize;
                                    let b = &mut buf[..n];
                                    if p.read(r.base + off, b) {
                                        for s in (0..n.saturating_sub(al - 1)).step_by(al) {
                                            let c = &b[s..s + al];
                                            if matches(vt, tol, cmp, c, 0) {
                                                out.push(Hit { addr: r.base + off + s as u64, prev: vt.raw(c) });
                                            }
                                        }
                                        if out.len() > LIST_CAP / threads {
                                            overflow.store(true, Ordering::Relaxed);
                                        }
                                    }
                                    off += n as u64;
                                    prog.done.fetch_add(n as u64, Ordering::Relaxed);
                                }
                            }
                            out
                        })
                    })
                    .collect();
                hs.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
            });
            if prog.cancelled() {
                bail!("cancelled");
            }
            if overflow.load(Ordering::Relaxed) {
                bail!("too many results (> {LIST_CAP}); use a more specific value or a range");
            }
            all.sort_unstable_by_key(|h| h.addr);
            self.state = State::List(all);
        }
        self.scans = 1;
        Ok(self.count())
    }

    pub fn next_scan(&mut self, p: &dyn Process, cmp: Cmp, prog: &Progress) -> Result<u64> {
        if cmp == Cmp::Unknown {
            bail!("'unknown' is only valid for the first scan");
        }
        let vt = self.vtype;
        let al = vt.size();
        let tol = self.tolerance;
        prog.done.store(0, Ordering::Relaxed);
        match std::mem::replace(&mut self.state, State::Empty) {
            State::Empty => bail!("no first scan yet"),
            State::List(hits) => {
                prog.total.store(hits.len().max(1) as u64, Ordering::Relaxed);
                let mut out = Vec::with_capacity(hits.len() / 2);
                let mut i = 0;
                let mut buf = vec![0u8; 1 << 16];
                while i < hits.len() {
                    if prog.cancelled() {
                        self.state = State::List(hits);
                        bail!("cancelled");
                    }
                    // batch hits within a 64 KiB window into one read
                    let start = hits[i].addr;
                    let mut j = i;
                    while j < hits.len() && hits[j].addr + al as u64 - start <= buf.len() as u64 {
                        j += 1;
                    }
                    let span = (hits[j - 1].addr + al as u64 - start) as usize;
                    let ok = p.read(start, &mut buf[..span]);
                    for h in &hits[i..j] {
                        let mut tmp = [0u8; 8];
                        let cur: &[u8] = if ok {
                            let o = (h.addr - start) as usize;
                            &buf[o..o + al]
                        } else if p.read(h.addr, &mut tmp[..al]) {
                            &tmp[..al]
                        } else {
                            continue;
                        };
                        if matches(vt, tol, cmp, cur, h.prev) {
                            out.push(Hit { addr: h.addr, prev: vt.raw(cur) });
                        }
                    }
                    prog.done.fetch_add((j - i) as u64, Ordering::Relaxed);
                    i = j;
                }
                self.state = State::List(out);
            }
            State::Snapshot { path, regions, mut bitmap, .. } => {
                prog.total.store(regions.iter().map(|r| r.size).sum::<u64>().max(1), Ordering::Relaxed);
                let new_path = Self::snapshot_path();
                let res: Result<u64> = (|| {
                    let mut rd = BufReader::with_capacity(8 << 20, File::open(&path)?);
                    let mut w = BufWriter::with_capacity(8 << 20, File::create(&new_path)?);
                    let mut cur = vec![0u8; CHUNK];
                    let mut old = vec![0u8; CHUNK];
                    let mut bit = 0u64;
                    let mut count = 0u64;
                    for r in &regions {
                        let slots = r.size / al as u64;
                        let mut off = 0u64;
                        while off < r.size {
                            if prog.cancelled() {
                                bail!("cancelled");
                            }
                            let n = (CHUNK as u64).min(r.size - off) as usize;
                            rd.read_exact(&mut old[..n])?;
                            let ok = p.read(r.base + off, &mut cur[..n]);
                            if !ok {
                                cur[..n].fill(0);
                            }
                            w.write_all(&cur[..n])?;
                            for s in 0..(n / al) as u64 {
                                let i = bit + off / al as u64 + s;
                                let word = &mut bitmap[(i / 64) as usize];
                                if *word >> (i % 64) & 1 == 0 {
                                    continue;
                                }
                                let o = s as usize * al;
                                let keep = ok && matches(vt, tol, cmp, &cur[o..o + al], vt.raw(&old[o..o + al]));
                                if keep {
                                    count += 1;
                                } else {
                                    *word &= !(1u64 << (i % 64));
                                }
                            }
                            off += n as u64;
                            prog.done.fetch_add(n as u64, Ordering::Relaxed);
                        }
                        bit += slots;
                    }
                    w.flush()?;
                    Ok(count)
                })();
                match res {
                    Ok(count) => {
                        let _ = std::fs::remove_file(&path);
                        self.state = State::Snapshot { path: new_path, regions, bitmap, count };
                        if count <= LIST_THRESHOLD {
                            let addrs = self.hits(count as usize);
                            let mut list = Vec::with_capacity(addrs.len());
                            let mut tmp = [0u8; 8];
                            for h in addrs {
                                if p.read(h.addr, &mut tmp[..al]) {
                                    list.push(Hit { addr: h.addr, prev: vt.raw(&tmp[..al]) });
                                }
                            }
                            self.reset();
                            self.state = State::List(list);
                        }
                    }
                    Err(e) => {
                        let _ = std::fs::remove_file(&new_path);
                        self.state = State::Snapshot { path, regions, bitmap, count: 0 };
                        return Err(e);
                    }
                }
            }
        }
        self.scans += 1;
        Ok(self.count())
    }
}

impl Drop for Scanner {
    fn drop(&mut self) {
        self.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::FakeProcess;

    fn proc_() -> FakeProcess {
        FakeProcess::new(vec![(0x10000, 0x4000, true, false), (0x20000, 0x8000, true, false), (0x400000, 0x2000, false, true)], (0x400000, 0x2000))
    }

    #[test]
    fn exact_then_changed() {
        let p = proc_();
        p.poke(0x10010, &123.5f32.to_le_bytes());
        p.poke(0x20200, &123.5f32.to_le_bytes());
        p.poke(0x20300, &99.0f32.to_le_bytes());
        let mut s = Scanner::new(VType::F32);
        let pr = Progress::default();
        assert_eq!(s.first_scan(&p, Cmp::Exact(123.5), &pr).unwrap(), 2);
        p.poke(0x20200, &130.0f32.to_le_bytes());
        assert_eq!(s.next_scan(&p, Cmp::Increased, &pr).unwrap(), 1);
        assert_eq!(s.hits(10)[0].addr, 0x20200);
        assert_eq!(s.next_scan(&p, Cmp::Unchanged, &pr).unwrap(), 1);
        assert_eq!(s.next_scan(&p, Cmp::Exact(130.0), &pr).unwrap(), 1);
    }

    #[test]
    fn unknown_value_flow() {
        let p = proc_();
        // a "weapon id" that changes when switching weapons
        let addr = 0x20444u64;
        p.poke(addr, &0xAAAA_0001u32.to_le_bytes());
        let mut s = Scanner::new(VType::I32);
        let pr = Progress::default();
        let n0 = s.first_scan(&p, Cmp::Unknown, &pr).unwrap();
        assert_eq!(n0, (0x4000 + 0x8000 + 0x2000) / 4); // includes the exe image (static data)
        p.poke(addr, &0xBBBB_0002u32.to_le_bytes());
        p.poke(0x10020, &7u32.to_le_bytes()); // noise
        assert_eq!(s.next_scan(&p, Cmp::Changed, &pr).unwrap(), 2);
        p.poke(addr, &0xAAAA_0001u32.to_le_bytes());
        assert_eq!(s.next_scan(&p, Cmp::Changed, &pr).unwrap(), 1);
        assert_eq!(s.hits(5)[0].addr, addr);
    }

    #[test]
    fn integer_and_range() {
        let p = proc_();
        p.poke(0x10100, &450i32.to_le_bytes());
        p.poke(0x10200, &300i32.to_le_bytes());
        let mut s = Scanner::new(VType::I32);
        let pr = Progress::default();
        assert_eq!(s.first_scan(&p, Cmp::Between(400.0, 500.0), &pr).unwrap(), 1);
        p.poke(0x10100, &10i32.to_le_bytes());
        assert_eq!(s.next_scan(&p, Cmp::Decreased, &pr).unwrap(), 1);
    }
}
