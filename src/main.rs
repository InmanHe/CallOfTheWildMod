#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;

use std::io::Write;

/// `CotwMod.exe --selftest [value]` : headless diagnostics. Attaches to the game,
/// reports module/regions, and if a float `value` is given, scans for it, waits,
/// rescans for "increased" and runs a pointer scan on what is left.
/// Output goes to cotwmod_selftest.txt next to the working directory.
fn selftest(args: &[String]) {
    use cotwmod::memory::pointer::{self, PtrScanOptions};
    use cotwmod::memory::scan::{Cmp, Progress, Scanner, VType};
    let mut out = std::fs::File::create("cotwmod_selftest.txt").expect("create log");
    let mut log = |s: String| {
        let _ = writeln!(out, "{s}");
        println!("{s}");
    };
    log(format!("CotwMod {} selftest", env!("CARGO_PKG_VERSION")));
    let Some(p) = cotwmod::memory::attach_game() else {
        log(format!("could not attach to {} (not running, or run as administrator)", cotwmod::memory::GAME_EXE));
        return;
    };
    let regions = p.regions();
    log(format!("attached; module {:X?}; {} readable regions, {:.1} MiB writable",
        p.main_module(), regions.len(), regions.iter().filter(|r| r.writable).map(|r| r.size).sum::<u64>() as f64 / 1048576.0));
    let Some(v) = args.get(2).and_then(|s| s.parse::<f64>().ok()) else { return };
    let prog = Progress::default();
    let mut sc = Scanner::new(VType::F32);
    sc.tolerance = 0.01;
    let t = std::time::Instant::now();
    log(format!("first scan exact {v}: {:?} results in {:?}", sc.first_scan(p.as_ref(), Cmp::Exact(v), &prog).map_err(|e| e.to_string()), t.elapsed()));
    std::thread::sleep(std::time::Duration::from_millis(2500));
    log(format!("next scan increased: {:?}", sc.next_scan(p.as_ref(), Cmp::Increased, &prog).map_err(|e| e.to_string())));
    for h in sc.hits(5) {
        let mut b = [0u8; 4];
        p.read(h.addr, &mut b);
        log(format!("  hit {:X} = {}", h.addr, f32::from_le_bytes(b)));
        let t = std::time::Instant::now();
        let chains = pointer::scan(p.as_ref(), h.addr, &PtrScanOptions::default(), &Progress::default());
        log(format!("  pointer scan: {} chains in {:?}", chains.len(), t.elapsed()));
        for c in chains.iter().take(5) {
            log(format!("    {} -> {:X?}", c.describe(), pointer::resolve(p.as_ref(), c)));
        }
    }
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|a| a == "--selftest").unwrap_or(false) {
        selftest(&args);
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("CotwMod — theHunter: Call of the Wild")
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native("CotwMod", options, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}
