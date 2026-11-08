//! Pointer scanner: finds static pointer chains (module + offset -> ... ->
//! target) so a found address keeps working after the game restarts.
//!
//! Breadth-first, one full memory pass per level: at each level we look for
//! 8-byte values `v` with `target - max_offset <= v <= target` for any current
//! target. Hits inside the main executable are chain roots; other hits become
//! the targets of the next level (capped, smallest offsets first).

use super::scan::Progress;
use super::{Process, Region};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct PtrChain {
    /// base = main module base + module_offset
    pub module_offset: u64,
    /// Cheat Engine semantics: every offset but the last is dereferenced
    pub offsets: Vec<u64>,
}

impl PtrChain {
    pub fn describe(&self) -> String {
        let offs: Vec<String> = self.offsets.iter().map(|o| format!("{o:X}")).collect();
        format!("[\"{}\"+{:X}] -> {}", super::GAME_EXE, self.module_offset, offs.join(" -> "))
    }
}

pub fn resolve(p: &dyn Process, c: &PtrChain) -> Option<u64> {
    let (base, _) = p.main_module()?;
    let mut addr = p.read_u64(base + c.module_offset)?;
    let (last, rest) = c.offsets.split_last()?;
    for o in rest {
        addr = p.read_u64(addr.wrapping_add(*o))?;
        if addr < 0x10000 {
            return None;
        }
    }
    Some(addr.wrapping_add(*last))
}

pub struct PtrScanOptions {
    pub max_depth: usize,
    pub max_offset: u64,
    pub max_targets_per_level: usize,
    pub max_results: usize,
}

impl Default for PtrScanOptions {
    fn default() -> Self {
        PtrScanOptions { max_depth: 4, max_offset: 0x1000, max_targets_per_level: 4000, max_results: 300 }
    }
}

const CHUNK: usize = 4 << 20;

pub fn scan(p: &dyn Process, target: u64, opt: &PtrScanOptions, prog: &Progress) -> Vec<PtrChain> {
    let Some((mbase, msize)) = p.main_module() else { return vec![] };
    let in_module = |a: u64| a >= mbase && a < mbase + msize;
    let regions: Vec<Region> = p
        .regions()
        .into_iter()
        .filter(|r| (r.writable || (r.base >= mbase && r.end() <= mbase + msize)) && r.size <= (2u64 << 30))
        .collect();
    let level_bytes: u64 = regions.iter().map(|r| r.size).sum();
    prog.total.store((level_bytes * opt.max_depth as u64).max(1), Ordering::Relaxed);
    prog.done.store(0, Ordering::Relaxed);

    // links[addr] = [(points-into target, offset)]
    let mut links: HashMap<u64, Vec<(u64, u64)>> = HashMap::new();
    let mut roots: Vec<(u64, u64, u64)> = vec![]; // (static addr, target, offset)
    let mut targets = vec![target];
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(16);

    for _level in 0..opt.max_depth {
        if targets.is_empty() || prog.cancel.load(Ordering::Relaxed) {
            break;
        }
        targets.sort_unstable();
        targets.dedup();
        let lo = targets[0].saturating_sub(opt.max_offset);
        let hi = *targets.last().unwrap();
        let tg = &targets;
        let parts: Vec<Vec<Region>> = (0..threads).map(|t| regions.iter().skip(t).step_by(threads).copied().collect()).collect();
        let overflow = AtomicBool::new(false);
        let found: Vec<(u64, u64, u64)> = std::thread::scope(|sc| {
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
                                if prog.cancel.load(Ordering::Relaxed) || overflow.load(Ordering::Relaxed) {
                                    return out;
                                }
                                let n = (CHUNK as u64).min(r.size - off) as usize;
                                let b = &mut buf[..n];
                                if p.read(r.base + off, b) {
                                    for s in (0..n.saturating_sub(7)).step_by(8) {
                                        let v = u64::from_le_bytes(b[s..s + 8].try_into().unwrap());
                                        if v < lo || v > hi {
                                            continue;
                                        }
                                        let mut k = tg.partition_point(|&t| t < v);
                                        while k < tg.len() && tg[k] - v <= opt.max_offset {
                                            out.push((r.base + off + s as u64, tg[k], tg[k] - v));
                                            k += 1;
                                        }
                                    }
                                    if out.len() > 4_000_000 {
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
        let mut next: Vec<(u64, u64)> = vec![]; // (offset, addr)
        for (addr, t, o) in found {
            if in_module(addr) {
                roots.push((addr, t, o));
            } else if !links.contains_key(&addr) || links[&addr].iter().all(|x| x.0 != t) {
                links.entry(addr).or_default().push((t, o));
                next.push((o, addr));
            }
        }
        next.sort_unstable();
        next.dedup_by_key(|x| x.1);
        next.truncate(opt.max_targets_per_level);
        targets = next.into_iter().map(|x| x.1).collect();
        if roots.len() >= opt.max_results * 4 {
            break;
        }
    }

    // reconstruct chains (DFS from static roots down to the target)
    let mut out: Vec<PtrChain> = vec![];
    fn dfs(t: u64, target: u64, depth: usize, links: &HashMap<u64, Vec<(u64, u64)>>, acc: &mut Vec<u64>, emit: &mut dyn FnMut(&[u64]) -> bool) -> bool {
        if t == target {
            return emit(acc);
        }
        if depth == 0 {
            return true;
        }
        if let Some(ls) = links.get(&t) {
            for &(nt, o) in ls {
                acc.push(o);
                let go_on = dfs(nt, target, depth - 1, links, acc, emit);
                acc.pop();
                if !go_on {
                    return false;
                }
            }
        }
        true
    }
    roots.sort_by_key(|r| r.2);
    // each root gets its own budget so one busy root can't starve the others
    for links_v in links.values_mut() {
        links_v.sort_by_key(|x| x.1);
    }
    let per_root = 16usize;
    for (addr, t, o) in roots {
        let mut acc = vec![o];
        let mut n = 0usize;
        let mut emit = |offs: &[u64]| {
            out.push(PtrChain { module_offset: addr - mbase, offsets: offs.to_vec() });
            n += 1;
            n < per_root
        };
        dfs(t, target, opt.max_depth, &links, &mut acc, &mut emit);
        if out.len() >= opt.max_results * 20 {
            break;
        }
    }
    // rank: shorter chains first, then smaller offsets (structural, usually stable)
    let key = |c: &PtrChain| (c.offsets.iter().copied().max().unwrap_or(0) / 0x100, c.offsets.len(), c.offsets.iter().sum::<u64>(), c.module_offset);
    out.sort_by_key(key);
    out.dedup();
    // keep only chains that resolve right now
    out.retain(|c| resolve(p, c) == Some(target));
    out.truncate(opt.max_results);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::FakeProcess;

    #[test]
    fn finds_three_level_chain() {
        // module .data at 0x400000, heap at 0x10000000
        let p = FakeProcess::new(vec![(0x400000, 0x10000, true, true), (0x1000_0000, 0x100000, true, false)], (0x400000, 0x10000));
        let a = 0x1000_1000u64; // object A
        let b = 0x1000_8000u64; // object B
        let target = 0x1002_0040u64; // value inside object C at 0x10020000
        p.poke(0x400000 + 0x5A8, &a.to_le_bytes()); // static -> A
        p.poke(a + 0x30, &b.to_le_bytes()); // A+30 -> B
        p.poke(b + 0x18, &0x1002_0000u64.to_le_bytes()); // B+18 -> C
        p.poke(target, &55.0f32.to_le_bytes());
        let res = scan(&p, target, &PtrScanOptions::default(), &Progress::default());
        let want = PtrChain { module_offset: 0x5A8, offsets: vec![0x30, 0x18, 0x40] };
        assert!(res.contains(&want), "{res:?}");
        assert_eq!(resolve(&p, &want), Some(target));
        // object C moves (game reallocated it): chain follows
        let c2 = 0x1005_0000u64;
        p.poke(b + 0x18, &c2.to_le_bytes());
        assert_eq!(resolve(&p, &want), Some(c2 + 0x40));
    }
}
