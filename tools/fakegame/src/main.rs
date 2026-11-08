//! Test double for the game process: a float behind static -> A -> B -> value,
//! increasing by 1.0 every second. Writes addresses to fakegame.txt.
use std::io::Write;

#[repr(C)]
struct B { pad: [u8; 0x40], distance: f32, weapon_id: u32 }
#[repr(C)]
struct A { pad: [u8; 0x30], b: *mut B }

static mut ROOT: *mut A = std::ptr::null_mut();

fn main() {
    let b = Box::into_raw(Box::new(B { pad: [0; 0x40], distance: 123.25, weapon_id: 0xC0FFEE }));
    let a = Box::into_raw(Box::new(A { pad: [0; 0x30], b }));
    unsafe { ROOT = a; }
    let mut f = std::fs::File::create("fakegame.txt").unwrap();
    unsafe {
        writeln!(f, "root={:p} a={:p} b={:p} value={:p}", std::ptr::addr_of!(ROOT), a, b, std::ptr::addr_of!((*b).distance)).unwrap();
    }
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        unsafe {
            let bb = std::ptr::read_volatile(std::ptr::addr_of!(ROOT));
            (*(*bb).b).distance += 1.0;
        }
    }
}
