//! Test double for the game process: a window (like the real game) and a float
//! behind static -> A -> B -> value that increases by 1.0 every second.
use std::io::Write;

#[repr(C)]
struct B { pad: [u8; 0x40], distance: f32, weapon_id: u32 }
#[repr(C)]
struct A { pad: [u8; 0x30], b: *mut B }

static mut ROOT: *mut A = std::ptr::null_mut();

fn main() {
    let b = Box::into_raw(Box::new(B { pad: [0; 0x40], distance: 123.25, weapon_id: 1294205923 }));
    let a = Box::into_raw(Box::new(A { pad: [0; 0x30], b }));
    unsafe { ROOT = a; }
    let mut f = std::fs::File::create("fakegame.txt").unwrap();
    unsafe {
        writeln!(f, "root={:p} a={:p} b={:p} value={:p}", std::ptr::addr_of!(ROOT), a, b, std::ptr::addr_of!((*b).distance)).unwrap();
    }
    std::thread::spawn(|| loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        unsafe {
            let bb = std::ptr::read_volatile(std::ptr::addr_of!(ROOT));
            (*(*bb).b).distance += 1.0;
        }
    });
    // 'W' switches the held weapon between two real equipment hashes
    std::thread::spawn(|| {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
        let mut down = false;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(20));
            let file = std::path::Path::new("switch.txt");
            let trig = file.exists();
            if trig { let _ = std::fs::remove_file(file); }
            let d = trig || unsafe { GetAsyncKeyState(0x57) as u16 & 0x8000 != 0 };
            if d && !down {
                unsafe {
                    let bb = std::ptr::read_volatile(std::ptr::addr_of!(ROOT));
                    let w = &mut (*(*bb).b).weapon_id;
                    *w = if *w == 1294205923 { 846411716 } else { 1294205923 };
                    let mut f = std::fs::OpenOptions::new().append(true).open("fakegame.txt").unwrap();
                    writeln!(f, "weapon -> {}", *w).unwrap();
                }
            }
            down = d;
        }
    });
    unsafe { window() }
}

unsafe fn window() {
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    unsafe extern "system" fn wp(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        if m == WM_PAINT {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let dc = BeginPaint(h, &mut ps);
            let br = CreateSolidBrush(0x00305020);
            FillRect(dc, &ps.rcPaint, br);
            DeleteObject(br);
            EndPaint(h, &ps);
            return 0;
        }
        if m == WM_DESTROY { PostQuitMessage(0); return 0; }
        DefWindowProcW(h, m, w, l)
    }
    let w = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
    let cls = w("FakeCotw");
    let title = w("theHunter: Call of the Wild (fake)");
    let hi = GetModuleHandleW(std::ptr::null());
    let wc = WNDCLASSW { style: 0, lpfnWndProc: Some(wp), cbClsExtra: 0, cbWndExtra: 0, hInstance: hi, hIcon: std::ptr::null_mut(),
        hCursor: std::ptr::null_mut(), hbrBackground: std::ptr::null_mut(), lpszMenuName: std::ptr::null(), lpszClassName: cls.as_ptr() };
    RegisterClassW(&wc);
    let h = CreateWindowExW(0, cls.as_ptr(), title.as_ptr(), WS_POPUP | WS_VISIBLE, 200, 120, 1280, 720, std::ptr::null_mut(), std::ptr::null_mut(), hi, std::ptr::null());
    SetForegroundWindow(h);
    let mut msg: MSG = std::mem::zeroed();
    while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
}
