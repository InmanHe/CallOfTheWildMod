#![cfg_attr(not(windows), allow(dead_code, unused_imports))]
//! Global hotkeys (Alt + key), polled with GetAsyncKeyState — no hooks.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    ToggleOverlay,
    NextWeapon,
    NextAmmo,
    NextZero,
    NextMag,
    DistPlus1,
    DistMinus1,
    DistPlus5,
    DistMinus5,
    WindCw,
    WindCcw,
    WindUp,
    WindDown,
    CalibMode,
    CalibRecord,
    WeaponSwitched,
}

pub const KEYS: &[(u16, &str, Action)] = &[
    (0x75, "Alt+F6", Action::ToggleOverlay),
    (0x76, "Alt+F7", Action::NextWeapon),
    (0x77, "Alt+F8", Action::NextAmmo),
    (0x78, "Alt+F9", Action::NextZero),
    (0x79, "Alt+F10", Action::NextMag),
    (0x21, "Alt+PgUp", Action::DistPlus1),
    (0x22, "Alt+PgDn", Action::DistMinus1),
    (0x24, "Alt+Home", Action::DistPlus5),
    (0x23, "Alt+End", Action::DistMinus5),
    (0x27, "Alt+→", Action::WindCw),
    (0x25, "Alt+←", Action::WindCcw),
    (0x26, "Alt+↑", Action::WindUp),
    (0x28, "Alt+↓", Action::WindDown),
    (0x7A, "Alt+F11", Action::CalibMode),
    (0x2D, "Alt+Insert", Action::CalibRecord),
    (0x7B, "Alt+F12", Action::WeaponSwitched),
];

#[cfg_attr(not(windows), allow(dead_code))]
pub struct Hotkeys {
    down: [bool; 256],
}

impl Hotkeys {
    pub fn new() -> Self {
        Hotkeys { down: [false; 256] }
    }

    /// Newly pressed actions since the last poll.
    pub fn poll(&mut self) -> Vec<Action> {
        #[allow(unused_mut)]
        let mut out = vec![];
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
            let alt = (GetAsyncKeyState(0x12) as u16 & 0x8000) != 0;
            for &(vk, _, act) in KEYS {
                let d = (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0;
                if d && !self.down[vk as usize] && alt {
                    out.push(act);
                }
                self.down[vk as usize] = d;
            }
        }
        out
    }
}
