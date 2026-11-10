#![cfg_attr(not(windows), allow(dead_code, unused_imports))]
//! Native Win32 overlay: a layered, colour-keyed, click-through, topmost
//! window placed exactly over the game's client area and drawn with GDI.
//! Runs on its own thread (with the global hotkeys), so it keeps working
//! when the main window is minimised.

use super::aim::{self, Calib};
use super::hotkeys::{Action, Hotkeys};
use super::live::{LiveValues, Shared};
use super::settings::{ammo_name, weapon_name, Lang, Settings};
use cotwmod::ballistics::CalShot;
use std::sync::{Arc, Mutex};

pub enum Event {
    Action(Action),
    Shot(CalShot),
    Weapon { weapon: String, ammo: String, zero_index: usize },
}

pub struct OverlayShared {
    pub settings: Settings,
    pub calib: Calib,
    pub status: String,
    /// guided-setup instruction, drawn large at the top of the game
    pub prompt: String,
    pub events: Vec<Event>,
    pub stop: bool,
    /// show even when the game is not the foreground window
    pub always_show: bool,
    pub visible_now: bool,
}

impl OverlayShared {
    pub fn new(s: Settings) -> Self {
        OverlayShared { settings: s, calib: Calib::default(), status: String::new(), prompt: String::new(), events: vec![], stop: false, always_show: false, visible_now: false }
    }
}

fn tr(lang: Lang, zh: &'static str, en: &'static str) -> &'static str {
    if lang == Lang::Zh { zh } else { en }
}

/// Draw list in client pixels.
#[derive(PartialEq, Clone)]
enum Shape {
    Line(i32, i32, i32, i32, u32, i32),
    Circle(i32, i32, i32, u32, i32),
    Rect(i32, i32, i32, i32, u32, i32),
    Text(i32, i32, String, u32),
}

fn rgb(r: u8, g: u8, b: u8) -> u32 {
    // COLORREF 0x00BBGGRR; never exactly the colour key
    let c = (r as u32) | ((g as u32) << 8) | ((b as u32) << 16);
    if c == KEY { c ^ 0x010000 } else { c }
}

const KEY: u32 = 0x00FF00FF; // magenta = transparent

fn build(s: &Settings, live: &LiveValues, calib: &Calib, status: &str, prompt: &str, w: i32, h: i32) -> Vec<Shape> {
    let lang = s.lang;
    let mut v = vec![];
    if !prompt.is_empty() {
        // rough centring: CJK glyphs are about twice as wide as ASCII
        let px: i32 = prompt.chars().map(|c| if c.is_ascii() { 8 } else { 15 }).sum();
        let x = ((w - px) / 2).max(8);
        let y = h / 6;
        v.push(Shape::Rect(x - 10, y - 8, x + px + 10, y + 28, rgb(255, 220, 0), 2));
        v.push(Shape::Text(x, y, prompt.to_string(), rgb(255, 230, 40)));
    }
    let (cx, cy) = (w / 2, h / 2);
    let white = rgb(235, 235, 235);
    v.push(Shape::Line(cx - 4, cy, cx + 5, cy, white, 1));
    v.push(Shape::Line(cx, cy - 4, cx, cy + 5, white, 1));
    let marker = rgb(s.marker_rgb[0], s.marker_rgb[1], s.marker_rgb[2]);
    let r = s.marker_radius.round() as i32;
    let mut lines: Vec<String> = vec![format!("{} | {}", weapon_name(&s.weapon), ammo_name(&s.ammo))];
    if let Some(a) = aim::compute(s, live) {
        let mut last_y = i32::MIN;
        for (d, lx, ly) in &a.ladder {
            let (x, y) = (cx + lx.round() as i32, cy + ly.round() as i32);
            v.push(Shape::Line(x - 6, y, x + 7, y, white, 1));
            if (y - last_y).abs() >= 13 {
                v.push(Shape::Text(x + 10, y - 7, format!("{d:.0}"), white));
                last_y = y;
            }
        }
        let (mx, my) = (cx + a.dx.round() as i32, cy + a.dy.round() as i32);
        v.push(Shape::Circle(mx, my, r, marker, 2));
        v.push(Shape::Line(mx - 2 * r, my, mx - r, my, marker, 2));
        v.push(Shape::Line(mx + r, my, mx + 2 * r + 1, my, marker, 2));
        v.push(Shape::Line(mx, my - 2 * r, mx, my - r, marker, 2));
        v.push(Shape::Line(mx, my + r, mx, my + 2 * r + 1, marker, 2));
        let dot = |b: bool| if b { "●" } else { "○" };
        lines.push(format!("{} {:.0} m", tr(lang, "归零", "Zero"), s.zero_distance()));
        lines.push(format!("{} {} {:.1} m   {} {} {:+.1}°", dot(a.live_dist), tr(lang, "距离", "Dist"), a.distance, dot(a.live_pitch), tr(lang, "俯仰", "Incl"), a.incline));
        lines.push(format!("{} {} {:.1} m/s   {} {} {:.0}°", dot(a.live_wind), tr(lang, "风速", "Wind"), a.wind_speed, dot(a.live_dir), tr(lang, "来向", "from"), a.wind_rel));
        lines.push(format!("{} {:+.1} mrad  {} {:+.1} mrad  {:.2}s", tr(lang, "下坠", "Drop"), a.drop_mrad, tr(lang, "偏移", "Drift"), a.drift_mrad, a.tof));
    }
    if calib.mode {
        let (x, y) = (cx + calib.marker[0].round() as i32, cy + calib.marker[1].round() as i32);
        v.push(Shape::Rect(x - 5, y - 5, x + 6, y + 6, rgb(255, 60, 100), 2));
        lines.push(format!("{} ({:+.0},{:+.0}px)", tr(lang, "[校准] Alt+方向键移动红框到弹着点, Alt+Insert 记录", "[CALIB] Alt+arrows move the box to the hit, Alt+Insert records"), calib.marker[0], calib.marker[1]));
    }
    if !status.is_empty() {
        lines.push(status.to_string());
    }
    for (i, l) in lines.into_iter().enumerate() {
        v.push(Shape::Text(16, 14 + 19 * i as i32, l, rgb(225, 255, 225)));
    }
    v
}

#[cfg(not(windows))]
pub fn spawn(_live: Arc<Mutex<Shared>>, _ov: Arc<Mutex<OverlayShared>>) {}

#[cfg(windows)]
pub fn spawn(live: Arc<Mutex<Shared>>, ov: Arc<Mutex<OverlayShared>>) {
    std::thread::spawn(move || unsafe { run(live, ov) });
}

#[cfg(windows)]
unsafe fn run(live: Arc<Mutex<Shared>>, ov: Arc<Mutex<OverlayShared>>) {
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    unsafe extern "system" fn wndproc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        if m == WM_NCHITTEST {
            return HTTRANSPARENT as LRESULT;
        }
        DefWindowProcW(h, m, w, l)
    }

    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
    let class = wide("CotwModOverlay");
    let hinst = GetModuleHandleW(std::ptr::null());
    let wc = WNDCLASSW {
        style: 0,
        lpfnWndProc: Some(wndproc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinst,
        hIcon: null_mut(),
        hCursor: null_mut(),
        hbrBackground: null_mut(),
        lpszMenuName: std::ptr::null(),
        lpszClassName: class.as_ptr(),
    };
    RegisterClassW(&wc);
    let title = wide("CotwMod Overlay");
    let hwnd = CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        class.as_ptr(),
        title.as_ptr(),
        WS_POPUP,
        0, 0, 100, 100,
        null_mut(), null_mut(), hinst, std::ptr::null(),
    );
    if hwnd.is_null() {
        return;
    }
    SetLayeredWindowAttributes(hwnd, KEY, 255, LWA_COLORKEY);
    let face = wide("Microsoft YaHei");
    let font = CreateFontW(-15, 0, 0, 0, 600, 0, 0, 0, 1 /*DEFAULT_CHARSET*/ as _, 0, 0, 3 /*NONANTIALIASED_QUALITY*/ as _, 0, face.as_ptr());
    let key_brush = CreateSolidBrush(KEY);

    let mut hk = Hotkeys::new();
    let mut shown = false;
    let mut cur_rect = [0i32; 4];
    let mut last_shapes: Vec<Shape> = vec![];
    let mut mem_dc: HDC = null_mut();
    let mut bmp: HBITMAP = null_mut();
    let mut bmp_size = (0, 0);

    loop {
        let mut msg: MSG = std::mem::zeroed();
        while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let lv = live.lock().unwrap().out.clone();
        let (s, calib, status, prompt, visible) = {
            let mut o = ov.lock().unwrap();
            if o.stop {
                break;
            }
            for a in hk.poll() {
                let mut s = o.settings.clone();
                let mut c = o.calib;
                let record = aim::apply_action(&mut s, &mut c, a);
                if record {
                    if let Some(shot) = aim::make_shot(&s, &lv, &c) {
                        o.events.push(Event::Shot(shot));
                    }
                }
                o.settings = s;
                o.calib = c;
                o.events.push(Event::Action(a));
            }
            let mut s = o.settings.clone();
            if aim::apply_live_weapon(&mut s, &lv) {
                o.events.push(Event::Weapon { weapon: s.weapon.clone(), ammo: s.ammo.clone(), zero_index: s.zero_index });
                o.settings = s.clone();
            }
            let visible = (s.overlay_on || !o.prompt.is_empty()) && lv.window.is_some() && (lv.foreground || o.always_show);
            o.visible_now = visible;
            (s, o.calib, o.status.clone(), o.prompt.clone(), visible)
        };
        if !visible {
            if shown {
                ShowWindow(hwnd, SW_HIDE);
                shown = false;
            }
            std::thread::sleep(std::time::Duration::from_millis(30));
            continue;
        }
        let rect = lv.window.unwrap();
        if rect != cur_rect || !shown {
            SetWindowPos(hwnd, HWND_TOPMOST, rect[0], rect[1], rect[2], rect[3], SWP_NOACTIVATE | SWP_SHOWWINDOW);
            cur_rect = rect;
            shown = true;
            last_shapes.clear();
        }
        let (w, h) = (rect[2], rect[3]);
        let shapes = build(&s, &lv, &calib, &status, &prompt, w, h);
        if shapes != last_shapes {
            let wdc = GetDC(hwnd);
            if bmp_size != (w, h) {
                if !mem_dc.is_null() {
                    DeleteDC(mem_dc);
                    DeleteObject(bmp);
                }
                mem_dc = CreateCompatibleDC(wdc);
                bmp = CreateCompatibleBitmap(wdc, w, h);
                SelectObject(mem_dc, bmp);
                SelectObject(mem_dc, font);
                SetBkMode(mem_dc, TRANSPARENT as _);
                bmp_size = (w, h);
            }
            let full = RECT { left: 0, top: 0, right: w, bottom: h };
            FillRect(mem_dc, &full, key_brush);
            SelectObject(mem_dc, GetStockObject(NULL_BRUSH));
            for sh in &shapes {
                match sh {
                    Shape::Line(x0, y0, x1, y1, c, wd) => {
                        let pen = CreatePen(PS_SOLID as _, *wd, *c);
                        let old = SelectObject(mem_dc, pen);
                        MoveToEx(mem_dc, *x0, *y0, null_mut());
                        LineTo(mem_dc, *x1, *y1);
                        SelectObject(mem_dc, old);
                        DeleteObject(pen);
                    }
                    Shape::Circle(x, y, r, c, wd) => {
                        let pen = CreatePen(PS_SOLID as _, *wd, *c);
                        let old = SelectObject(mem_dc, pen);
                        Ellipse(mem_dc, x - r, y - r, x + r + 1, y + r + 1);
                        SelectObject(mem_dc, old);
                        DeleteObject(pen);
                    }
                    Shape::Rect(x0, y0, x1, y1, c, wd) => {
                        let pen = CreatePen(PS_SOLID as _, *wd, *c);
                        let old = SelectObject(mem_dc, pen);
                        Rectangle(mem_dc, *x0, *y0, *x1, *y1);
                        SelectObject(mem_dc, old);
                        DeleteObject(pen);
                    }
                    Shape::Text(x, y, t, c) => {
                        let u: Vec<u16> = t.encode_utf16().collect();
                        SetTextColor(mem_dc, rgb(8, 8, 8));
                        for (ox, oy) in [(-1, 0), (1, 0), (0, -1), (0, 1), (1, 1)] {
                            TextOutW(mem_dc, x + ox, y + oy, u.as_ptr(), u.len() as i32);
                        }
                        SetTextColor(mem_dc, *c);
                        TextOutW(mem_dc, *x, *y, u.as_ptr(), u.len() as i32);
                    }
                }
            }
            BitBlt(wdc, 0, 0, w, h, mem_dc, 0, 0, SRCCOPY);
            ReleaseDC(hwnd, wdc);
            last_shapes = shapes;
        }
        std::thread::sleep(std::time::Duration::from_millis(12));
    }
    DestroyWindow(hwnd);
}
