//! Windows process access.

use super::{Process, Region};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, STILL_ACTIVE};
use windows_sys::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Module32FirstW, Process32FirstW, Process32NextW, MODULEENTRY32W, PROCESSENTRY32W,
    TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Memory::{
    VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT, MEM_IMAGE, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE,
    PAGE_EXECUTE_WRITECOPY, PAGE_GUARD, PAGE_NOACCESS, PAGE_READONLY, PAGE_READWRITE, PAGE_WRITECOPY,
};
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
};

pub struct WinProcess {
    handle: HANDLE,
    pub pid: u32,
    module: (u64, u64),
}

unsafe impl Send for WinProcess {}
unsafe impl Sync for WinProcess {}

fn wide_eq(buf: &[u16], name: &str) -> bool {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end]).eq_ignore_ascii_case(name)
}

impl WinProcess {
    pub fn find_pid(name: &str) -> Option<u32> {
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE {
                return None;
            }
            let mut e: PROCESSENTRY32W = std::mem::zeroed();
            e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut found = None;
            if Process32FirstW(snap, &mut e) != 0 {
                loop {
                    if wide_eq(&e.szExeFile, name) {
                        found = Some(e.th32ProcessID);
                        break;
                    }
                    if Process32NextW(snap, &mut e) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snap);
            found
        }
    }

    pub fn open_by_name(name: &str) -> Option<WinProcess> {
        let pid = Self::find_pid(name)?;
        unsafe {
            let h = OpenProcess(
                PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_VM_OPERATION | PROCESS_QUERY_INFORMATION,
                0,
                pid,
            );
            if h.is_null() {
                return None;
            }
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid);
            let mut module = (0u64, 0u64);
            if snap != INVALID_HANDLE_VALUE {
                let mut m: MODULEENTRY32W = std::mem::zeroed();
                m.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;
                if Module32FirstW(snap, &mut m) != 0 {
                    module = (m.modBaseAddr as u64, m.modBaseSize as u64);
                }
                CloseHandle(snap);
            }
            Some(WinProcess { handle: h, pid, module })
        }
    }
}

impl Drop for WinProcess {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

const READABLE: u32 = PAGE_READONLY | PAGE_READWRITE | PAGE_WRITECOPY | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY;
const WRITABLE: u32 = PAGE_READWRITE | PAGE_WRITECOPY | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY;

impl Process for WinProcess {
    fn regions(&self) -> Vec<Region> {
        let mut out = vec![];
        let mut addr: u64 = 0x10000;
        unsafe {
            loop {
                let mut mbi: MEMORY_BASIC_INFORMATION = std::mem::zeroed();
                let n = VirtualQueryEx(self.handle, addr as *const _, &mut mbi, std::mem::size_of::<MEMORY_BASIC_INFORMATION>());
                if n == 0 {
                    break;
                }
                let base = mbi.BaseAddress as u64;
                let size = mbi.RegionSize as u64;
                let p = mbi.Protect;
                if mbi.State == MEM_COMMIT && p & PAGE_GUARD == 0 && p & PAGE_NOACCESS == 0 && p & READABLE != 0 {
                    out.push(Region { base, size, writable: p & WRITABLE != 0, image: mbi.Type == MEM_IMAGE });
                }
                let next = base.saturating_add(size);
                if next <= addr || next >= 0x7FFF_FFFF_FFFF {
                    break;
                }
                addr = next;
            }
        }
        out
    }

    fn read(&self, addr: u64, buf: &mut [u8]) -> bool {
        let mut got = 0usize;
        unsafe {
            ReadProcessMemory(self.handle, addr as *const _, buf.as_mut_ptr() as *mut _, buf.len(), &mut got) != 0
                && got == buf.len()
        }
    }

    fn write(&self, addr: u64, buf: &[u8]) -> bool {
        let mut put = 0usize;
        unsafe {
            WriteProcessMemory(self.handle, addr as *const _, buf.as_ptr() as *const _, buf.len(), &mut put) != 0
                && put == buf.len()
        }
    }

    fn main_module(&self) -> Option<(u64, u64)> {
        (self.module.0 != 0).then_some(self.module)
    }

    fn pid(&self) -> u32 {
        self.pid
    }

    fn alive(&self) -> bool {
        let mut code = 0u32;
        unsafe { GetExitCodeProcess(self.handle, &mut code) != 0 && code == STILL_ACTIVE as u32 }
    }
}

/// Full path of a running process' executable (for locating the game folder).
pub fn running_exe_path(name: &str) -> Option<std::path::PathBuf> {
    use windows_sys::Win32::System::Threading::{QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
    let pid = WinProcess::find_pid(name)?;
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut buf = vec![0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len) != 0;
        CloseHandle(h);
        ok.then(|| std::path::PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])))
    }
}

/// Steam install folder from the registry (HKCU\Software\Valve\Steam\SteamPath).
pub fn steam_path() -> Option<std::path::PathBuf> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
    let key: Vec<u16> = "Software\\Valve\\Steam\0".encode_utf16().collect();
    let val: Vec<u16> = "SteamPath\0".encode_utf16().collect();
    let mut buf = vec![0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    unsafe {
        let r = RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), val.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr() as *mut _, &mut size);
        if r != 0 {
            return None;
        }
    }
    let n = (size as usize / 2).saturating_sub(1);
    Some(std::path::PathBuf::from(String::from_utf16_lossy(&buf[..n]).replace('/', "\\")))
}

/// The game's main window for a pid: (hwnd, client rect on screen x, y, w, h, is foreground).
pub fn game_window(pid: u32) -> Option<(isize, [i32; 4], bool)> {
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
    use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetClientRect, GetForegroundWindow, GetWindowThreadProcessId, IsWindowVisible};
    struct Ctx { pid: u32, best: Option<(HWND, i64)> }
    unsafe extern "system" fn cb(h: HWND, l: LPARAM) -> BOOL {
        let ctx = &mut *(l as *mut Ctx);
        let mut p = 0u32;
        GetWindowThreadProcessId(h, &mut p);
        if p == ctx.pid && IsWindowVisible(h) != 0 {
            let mut r: RECT = std::mem::zeroed();
            if GetClientRect(h, &mut r) != 0 {
                let area = (r.right - r.left) as i64 * (r.bottom - r.top) as i64;
                if area > ctx.best.map(|b| b.1).unwrap_or(0) {
                    ctx.best = Some((h, area));
                }
            }
        }
        1
    }
    unsafe {
        let mut ctx = Ctx { pid, best: None };
        EnumWindows(Some(cb), &mut ctx as *mut Ctx as LPARAM);
        let (h, _) = ctx.best?;
        let mut r: RECT = std::mem::zeroed();
        GetClientRect(h, &mut r);
        let mut pt = POINT { x: 0, y: 0 };
        ClientToScreen(h, &mut pt);
        let fg = GetForegroundWindow();
        let mut fpid = 0u32;
        GetWindowThreadProcessId(fg, &mut fpid);
        Some((h as isize, [pt.x, pt.y, r.right - r.left, r.bottom - r.top], fpid == pid))
    }
}
