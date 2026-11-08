//! Process memory access, value scanning, pointer scanning and field bindings.
//!
//! Everything goes through the [`Process`] trait: a Windows implementation
//! (ReadProcessMemory / WriteProcessMemory / VirtualQueryEx) and an in-memory
//! fake used by the tests.

pub mod binding;
pub mod pointer;
pub mod scan;

#[cfg(windows)]
pub mod win;

use std::sync::RwLock;

pub const GAME_EXE: &str = "theHunterCotW_F.exe";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub base: u64,
    pub size: u64,
    pub writable: bool,
    pub image: bool,
}

impl Region {
    pub fn end(&self) -> u64 {
        self.base + self.size
    }
    pub fn contains(&self, a: u64) -> bool {
        a >= self.base && a < self.end()
    }
}

pub trait Process: Send + Sync {
    fn regions(&self) -> Vec<Region>;
    fn read(&self, addr: u64, buf: &mut [u8]) -> bool;
    fn write(&self, addr: u64, buf: &[u8]) -> bool;
    /// (base, size) of the main executable image
    fn main_module(&self) -> Option<(u64, u64)>;
    fn alive(&self) -> bool {
        true
    }
    fn pid(&self) -> u32 {
        0
    }

    fn read_u64(&self, addr: u64) -> Option<u64> {
        let mut b = [0u8; 8];
        self.read(addr, &mut b).then(|| u64::from_le_bytes(b))
    }
}

/// Fake process for tests: a set of regions backed by Vec<u8>.
pub struct FakeProcess {
    pub mem: RwLock<Vec<(Region, Vec<u8>)>>,
    pub module: (u64, u64),
}

impl FakeProcess {
    pub fn new(regions: Vec<(u64, usize, bool, bool)>, module: (u64, u64)) -> Self {
        let mem = regions
            .into_iter()
            .map(|(base, size, writable, image)| (Region { base, size: size as u64, writable, image }, vec![0u8; size]))
            .collect();
        FakeProcess { mem: RwLock::new(mem), module }
    }
    pub fn poke(&self, addr: u64, bytes: &[u8]) {
        assert!(self.write(addr, bytes), "poke out of range {addr:x}");
    }
}

impl Process for FakeProcess {
    fn regions(&self) -> Vec<Region> {
        self.mem.read().unwrap().iter().map(|(r, _)| *r).collect()
    }
    fn read(&self, addr: u64, buf: &mut [u8]) -> bool {
        let m = self.mem.read().unwrap();
        for (r, data) in m.iter() {
            if r.contains(addr) && addr + buf.len() as u64 <= r.end() {
                let o = (addr - r.base) as usize;
                buf.copy_from_slice(&data[o..o + buf.len()]);
                return true;
            }
        }
        false
    }
    fn write(&self, addr: u64, buf: &[u8]) -> bool {
        let mut m = self.mem.write().unwrap();
        for (r, data) in m.iter_mut() {
            if r.contains(addr) && addr + buf.len() as u64 <= r.end() {
                let o = (addr - r.base) as usize;
                data[o..o + buf.len()].copy_from_slice(buf);
                return true;
            }
        }
        false
    }
    fn main_module(&self) -> Option<(u64, u64)> {
        Some(self.module)
    }
}

/// Attach to the game (Windows only). Returns None if not running.
pub fn attach_game() -> Option<std::sync::Arc<dyn Process>> {
    #[cfg(windows)]
    {
        win::WinProcess::open_by_name(GAME_EXE).map(|p| std::sync::Arc::new(p) as std::sync::Arc<dyn Process>)
    }
    #[cfg(not(windows))]
    {
        None
    }
}
