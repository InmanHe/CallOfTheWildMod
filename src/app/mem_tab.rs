use super::settings::Lang;
use super::{tr, App};
use cotwmod::memory::binding::{Binding, Field, Loc};
use cotwmod::memory::pointer::{self, PtrChain, PtrScanOptions};
use cotwmod::memory::scan::{Cmp, Hit, Progress, Scanner, VType};
use cotwmod::memory::{attach_game, Process};
use eframe::egui::{self, Color32, RichText};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Exact,
    Between,
    Unknown,
    Increased,
    Decreased,
    Changed,
    Unchanged,
}

enum Job {
    Scan(JoinHandle<anyhow::Result<u64>>),
    Ptr(JoinHandle<Vec<PtrChain>>),
}

pub struct MemState {
    scanner: Arc<Mutex<Scanner>>,
    vtype: VType,
    mode: Mode,
    value: String,
    value2: String,
    tolerance: f64,
    progress: Arc<Progress>,
    job: Option<Job>,
    hits: Vec<Hit>,
    count: u64,
    selected: Option<u64>,
    bind_field: Field,
    ptr_opts: (usize, u64),
    ptr_results: Vec<PtrChain>,
    ptr_target: u64,
    weapon_pick: String,
    ammo_pick: String,
    manual_addr: String,
}

impl Default for MemState {
    fn default() -> Self {
        MemState {
            scanner: Arc::new(Mutex::new(Scanner::new(VType::F32))),
            vtype: VType::F32,
            mode: Mode::Exact,
            value: String::new(),
            value2: String::new(),
            tolerance: 0.5,
            progress: Arc::new(Progress::default()),
            job: None,
            hits: vec![],
            count: 0,
            selected: None,
            bind_field: Field::AimDistance,
            ptr_opts: (4, 0x1000),
            ptr_results: vec![],
            ptr_target: 0,
            weapon_pick: String::new(),
            ammo_pick: String::new(),
            manual_addr: String::new(),
        }
    }
}

fn field_name(f: Field, lang: Lang) -> &'static str {
    match f {
        Field::WeaponId => tr(lang, "手持武器 ID", "Held weapon ID"),
        Field::AmmoId => tr(lang, "弹药 ID", "Ammo ID"),
        Field::ZeroIndex => tr(lang, "归零档位", "Zero index"),
        Field::AimDistance => tr(lang, "瞄准距离 (m)", "Aim distance (m)"),
        Field::CameraPitch => tr(lang, "镜头俯仰 (°)", "Camera pitch (°)"),
        Field::CameraYaw => tr(lang, "镜头朝向 (°)", "Camera yaw (°)"),
        Field::WindFrom => tr(lang, "风来向 (°)", "Wind from (°)"),
        Field::WindSpeed => tr(lang, "风速 (m/s)", "Wind speed (m/s)"),
        Field::CameraFov => tr(lang, "镜头 FOV (°)", "Camera FOV (°)"),
        Field::ViewDistance => tr(lang, "猎物可视距离 (m)", "Animal view distance (m)"),
    }
}

fn field_hint(f: Field, lang: Lang) -> &'static str {
    match f {
        Field::WeaponId => tr(lang, "类型 4 Bytes，首次扫描“未知初始值”；切换武器后“已改变”，不切换时“未改变”，反复几轮。再切回原武器，值应回到之前的数。",
            "4 Bytes, first scan 'Unknown'; switch weapon -> 'Changed', don't switch -> 'Unchanged', repeat. Switching back should restore the old value."),
        Field::AmmoId => tr(lang, "同上，切换弹药类型。", "Same as weapon, switching ammo type."),
        Field::ZeroIndex => tr(lang, "4 Bytes 或 Byte。游戏里切换归零距离：第 1 档=0、第 2 档=1、第 3 档=2，用“精确值”扫描 0/1/2。档位顺序见“落点瞄具”页的归零按钮。",
            "4 Bytes or Byte. Change the zero in game: 1st setting = 0, 2nd = 1, 3rd = 2; exact-scan 0/1/2. Order = the zero buttons on the Bow sight tab."),
        Field::AimDistance => tr(lang, "Float。用测距仪/望远镜看着物体，按显示的距离做“精确值”扫描（容差 0.5），换目标后再扫。",
            "Float. Look at things with the rangefinder and scan the shown distance (tolerance 0.5); change target and rescan."),
        Field::CameraPitch => tr(lang, "Float。抬头扫“增大”，低头扫“减小”。可能是弧度：绑定后把缩放设为 57.29578。",
            "Float. Look up -> 'Increased', down -> 'Decreased'. Might be radians: set scale 57.29578 after binding."),
        Field::CameraYaw => tr(lang, "Float。原地转身配合“增大/减小”，与屏幕上方指南针对照；方向相反用缩放 -1。",
            "Float. Turn in place with Increased/Decreased, compare with the compass; use scale -1 if reversed."),
        Field::WindFrom => tr(lang, "Float。风向变化时扫“已改变”，稳定时“未改变”，与 HUD 风向对照。", "Float. 'Changed' when the wind shifts, 'Unchanged' otherwise; compare with the HUD."),
        Field::WindSpeed => tr(lang, "Float。风力等级变化时扫“增大/减小”。", "Float. 'Increased/Decreased' as the wind level changes."),
        Field::CameraFov => tr(lang, "Float。开镜/关镜时扫“减小/增大”。绑定后自动处理放大倍率。", "Float. Aim in -> 'Decreased', out -> 'Increased'. Handles zoom automatically."),
        Field::ViewDistance => tr(lang, "Float，可绑定多个地址。常见做法：在不同画质预设间切换配合“已改变”，或在 100–1000 范围内扫描后逐个试写，观察远处动物是否出现。写错地址可能导致游戏崩溃，请先保存游戏。",
            "Float, several addresses allowed. Switch quality presets with 'Changed', or range-scan 100–1000 and test-write candidates while watching distant animals. Wrong addresses can crash the game — save first."),
    }
}

fn parse_num(s: &str) -> Option<f64> {
    let s = s.trim();
    if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return u64::from_str_radix(h, 16).ok().map(|v| v as f64);
    }
    s.parse().ok()
}

fn read_val(p: &dyn Process, vt: VType, a: u64) -> Option<f64> {
    let mut b = [0u8; 8];
    p.read(a, &mut b[..vt.size()]).then(|| vt.decode(&b[..vt.size()]))
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let lang = app.s.lang;
    let t = |zh, en| tr(lang, zh, en);
    let process = app.shared.lock().unwrap().process.clone();

    // finish background jobs
    if let Some(job) = app.mem.job.take() {
        match job {
            Job::Scan(h) if h.is_finished() => {
                match h.join() {
                    Ok(Ok(n)) => app.status = format!("{} {n}", t("扫描完成，结果数：", "Scan done, results:")),
                    Ok(Err(e)) => app.status = format!("{e:#}"),
                    Err(_) => app.status = "scan thread panicked".into(),
                }
                let sc = app.mem.scanner.lock().unwrap();
                app.mem.count = sc.count();
                app.mem.hits = sc.hits(500);
            }
            Job::Ptr(h) if h.is_finished() => {
                app.mem.ptr_results = h.join().unwrap_or_default();
                app.status = format!("{} {}", t("指针扫描完成，找到链：", "Pointer scan done, chains:"), app.mem.ptr_results.len());
            }
            j => app.mem.job = Some(j),
        }
    }
    let busy = app.mem.job.is_some();

    ui.horizontal(|ui| {
        match &process {
            Some(_) => ui.colored_label(Color32::from_rgb(80, 200, 120), t("● 已连接 theHunterCotW_F.exe", "● Attached to theHunterCotW_F.exe")),
            None => ui.colored_label(Color32::GRAY, t("○ 未连接：请先启动游戏（若失败，以管理员身份运行本程序）", "○ Not attached: start the game (run as admin if it fails)")),
        };
        if process.is_none() && ui.button(t("立即连接", "Attach now")).clicked() {
            if let Some(p) = attach_game() {
                let mut sh = app.shared.lock().unwrap();
                sh.process = Some(p);
                sh.out.attached = true;
            }
        }
    });
    ui.label(RichText::new(t("仅读取/写入游戏进程内存，不修改任何游戏文件。只在单人模式使用。", "Reads/writes game process memory only; no game files are changed. Single-player only.")).small());
    ui.separator();

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.columns(2, |cols| {
            // ================================================= scanner
            let ui = &mut cols[0];
            ui.heading(t("数值扫描", "Value scan"));
            let first = !app.mem.scanner.lock().unwrap().has_results();
            ui.horizontal(|ui| {
                ui.label(t("类型", "Type"));
                ui.add_enabled_ui(first && !busy, |ui| {
                    egui::ComboBox::from_id_salt("vt").selected_text(app.mem.vtype.name()).show_ui(ui, |ui| {
                        for v in VType::ALL {
                            ui.selectable_value(&mut app.mem.vtype, v, v.name());
                        }
                    });
                });
                ui.label(t("方式", "Mode"));
                let modes: Vec<(Mode, &str)> = if first {
                    vec![(Mode::Exact, t("精确值", "Exact")), (Mode::Between, t("范围", "Between")), (Mode::Unknown, t("未知初始值", "Unknown initial"))]
                } else {
                    vec![(Mode::Exact, t("精确值", "Exact")), (Mode::Between, t("范围", "Between")), (Mode::Increased, t("增大", "Increased")),
                        (Mode::Decreased, t("减小", "Decreased")), (Mode::Changed, t("已改变", "Changed")), (Mode::Unchanged, t("未改变", "Unchanged"))]
                };
                if !modes.iter().any(|m| m.0 == app.mem.mode) {
                    app.mem.mode = modes[0].0;
                }
                egui::ComboBox::from_id_salt("mode").selected_text(modes.iter().find(|m| m.0 == app.mem.mode).unwrap().1).show_ui(ui, |ui| {
                    for (m, n) in &modes {
                        ui.selectable_value(&mut app.mem.mode, *m, *n);
                    }
                });
            });
            ui.horizontal(|ui| {
                if matches!(app.mem.mode, Mode::Exact | Mode::Between) {
                    ui.add(egui::TextEdit::singleline(&mut app.mem.value).desired_width(100.0).hint_text(t("数值", "value")));
                }
                if app.mem.mode == Mode::Between {
                    ui.label("~");
                    ui.add(egui::TextEdit::singleline(&mut app.mem.value2).desired_width(100.0));
                }
                if app.mem.vtype.is_float() {
                    ui.label(t("容差", "tol."));
                    ui.add(egui::DragValue::new(&mut app.mem.tolerance).range(0.0..=100.0).speed(0.01));
                }
            });
            ui.horizontal(|ui| {
                let cmp = match app.mem.mode {
                    Mode::Exact => parse_num(&app.mem.value).map(Cmp::Exact),
                    Mode::Between => parse_num(&app.mem.value).zip(parse_num(&app.mem.value2)).map(|(a, b)| Cmp::Between(a, b)),
                    Mode::Unknown => Some(Cmp::Unknown),
                    Mode::Increased => Some(Cmp::Increased),
                    Mode::Decreased => Some(Cmp::Decreased),
                    Mode::Changed => Some(Cmp::Changed),
                    Mode::Unchanged => Some(Cmp::Unchanged),
                };
                let label = if first { t("首次扫描", "First scan") } else { t("再次扫描", "Next scan") };
                if ui.add_enabled(!busy && process.is_some() && cmp.is_some(), egui::Button::new(RichText::new(label).strong())).clicked() {
                    let p = process.clone().unwrap();
                    let sc = app.mem.scanner.clone();
                    let prog = Arc::new(Progress::default());
                    app.mem.progress = prog.clone();
                    let (vt, tol) = (app.mem.vtype, app.mem.tolerance);
                    let cmp = cmp.unwrap();
                    app.mem.job = Some(Job::Scan(std::thread::spawn(move || {
                        let mut s = sc.lock().unwrap();
                        if first {
                            s.vtype = vt;
                        }
                        s.tolerance = tol;
                        if first { s.first_scan(p.as_ref(), cmp, &prog) } else { s.next_scan(p.as_ref(), cmp, &prog) }
                    })));
                }
                if ui.add_enabled(!busy, egui::Button::new(t("新扫描（重置）", "New scan (reset)"))).clicked() {
                    app.mem.scanner.lock().unwrap().reset();
                    app.mem.hits.clear();
                    app.mem.count = 0;
                    app.mem.selected = None;
                }
                if busy && ui.button(t("取消", "Cancel")).clicked() {
                    app.mem.progress.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            });
            if busy {
                ui.add(egui::ProgressBar::new(app.mem.progress.fraction()).show_percentage());
            }
            ui.label(format!("{} {}", t("结果：", "Results:"), app.mem.count));
            let vt = app.mem.scanner.try_lock().map(|s| s.vtype).unwrap_or(app.mem.vtype);
            egui::ScrollArea::vertical().id_salt("hits").max_height(220.0).show(ui, |ui| {
                for h in app.mem.hits.iter().take(300) {
                    let cur = process.as_ref().and_then(|p| read_val(p.as_ref(), vt, h.addr));
                    let txt = format!("{:016X}   {}", h.addr, cur.map(|v| format!("{v}")).unwrap_or("??".into()));
                    if ui.selectable_label(app.mem.selected == Some(h.addr), RichText::new(txt).monospace()).clicked() {
                        app.mem.selected = Some(h.addr);
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label(t("或手动地址", "or address"));
                ui.add(egui::TextEdit::singleline(&mut app.mem.manual_addr).desired_width(150.0).hint_text("0x..."));
                if ui.button(t("选用", "Use")).clicked() {
                    if let Some(a) = parse_num(&app.mem.manual_addr) {
                        app.mem.selected = Some(a as u64);
                    }
                }
            });

            if let Some(addr) = app.mem.selected {
                ui.separator();
                ui.strong(format!("{} {addr:016X}", t("选中地址", "Selected")));
                ui.horizontal(|ui| {
                    ui.label(t("绑定为", "Bind as"));
                    egui::ComboBox::from_id_salt("bf").selected_text(field_name(app.mem.bind_field, lang)).show_ui(ui, |ui| {
                        for f in Field::ALL {
                            ui.selectable_value(&mut app.mem.bind_field, f, field_name(f, lang));
                        }
                    });
                    if ui.button(t("绑定（绝对地址）", "Bind (absolute)")).clicked() {
                        let b = Binding { vtype: vt, loc: Loc::Absolute(addr), scale: 1.0, add: 0.0 };
                        bind(app, app.mem.bind_field, b);
                    }
                });
                ui.label(RichText::new(t("绝对地址在游戏重启后失效；用指针扫描获得长期有效的地址。", "Absolute addresses break when the game restarts; use a pointer scan for a permanent one.")).small());
                ui.horizontal(|ui| {
                    ui.label(t("深度", "Depth"));
                    ui.add(egui::DragValue::new(&mut app.mem.ptr_opts.0).range(1..=6));
                    ui.label(t("最大偏移", "Max offset"));
                    ui.add(egui::DragValue::new(&mut app.mem.ptr_opts.1).range(0x100..=0x4000).hexadecimal(4, false, true));
                    if ui.add_enabled(!busy && process.is_some(), egui::Button::new(t("指针扫描", "Pointer scan"))).clicked() {
                        let p = process.clone().unwrap();
                        let prog = Arc::new(Progress::default());
                        app.mem.progress = prog.clone();
                        app.mem.ptr_target = addr;
                        let opts = PtrScanOptions { max_depth: app.mem.ptr_opts.0, max_offset: app.mem.ptr_opts.1, ..Default::default() };
                        app.mem.job = Some(Job::Ptr(std::thread::spawn(move || pointer::scan(p.as_ref(), addr, &opts, &prog))));
                    }
                });
            }
            if !app.mem.ptr_results.is_empty() {
                ui.label(format!("{} ({:016X})", t("指针链，优先选最短的：", "Pointer chains (prefer the shortest):"), app.mem.ptr_target));
                let mut chosen = None;
                egui::ScrollArea::vertical().id_salt("ptrs").max_height(160.0).show(ui, |ui| {
                    for c in &app.mem.ptr_results {
                        ui.horizontal(|ui| {
                            if ui.small_button(t("绑定", "Bind")).clicked() {
                                chosen = Some(c.clone());
                            }
                            ui.label(RichText::new(c.describe()).monospace().small());
                        });
                    }
                });
                if let Some(c) = chosen {
                    let b = Binding { vtype: vt, loc: Loc::Pointer(c), scale: 1.0, add: 0.0 };
                    bind(app, app.mem.bind_field, b);
                }
            }
            ui.separator();
            ui.collapsing(t("如何找各个数值", "How to find each value"), |ui| {
                for f in Field::ALL {
                    ui.label(RichText::new(field_name(f, lang)).strong());
                    ui.label(field_hint(f, lang));
                }
            });

            // ================================================= bindings
            let ui = &mut cols[1];
            ui.heading(t("已绑定的字段", "Bound fields"));
            let mut remove: Option<(Field, usize)> = None;
            let mut edited = false;
            egui::Grid::new("binds").striped(true).show(ui, |ui| {
                for f in Field::ALL {
                    let n = app.s.bindings.get(&f).map(|v| v.len()).unwrap_or(0);
                    if n == 0 {
                        ui.label(field_name(f, lang));
                        ui.label(RichText::new(t("未绑定", "not bound")).color(Color32::GRAY));
                        ui.end_row();
                        continue;
                    }
                    for i in 0..n {
                        let val = match f {
                            Field::ViewDistance => app.live.view_values.get(i).copied().flatten(),
                            _ => app.live.values.get(&f).copied(),
                        };
                        ui.label(field_name(f, lang));
                        let b = &mut app.s.bindings.get_mut(&f).unwrap()[i];
                        ui.vertical(|ui| {
                            ui.label(RichText::new(b.describe()).monospace().small());
                            ui.horizontal(|ui| {
                                ui.label(format!("= {}", val.map(|v| format!("{v:.3}")).unwrap_or("—".into())));
                                if matches!(f, Field::WeaponId | Field::AmmoId) {
                                    if let Some(r) = app.live.raw.get(&f) {
                                        ui.label(format!("(0x{r:X})"));
                                    }
                                }
                                ui.label(t("缩放", "scale"));
                                edited |= ui.add(egui::DragValue::new(&mut b.scale).speed(0.01)).changed();
                                ui.label(t("加", "add"));
                                edited |= ui.add(egui::DragValue::new(&mut b.add).speed(0.1)).changed();
                                if ui.small_button("✖").clicked() {
                                    remove = Some((f, i));
                                }
                            });
                        });
                        ui.end_row();
                    }
                }
            });
            if let Some((f, i)) = remove {
                if let Some(v) = app.s.bindings.get_mut(&f) {
                    v.remove(i);
                    if v.is_empty() {
                        app.s.bindings.remove(&f);
                    }
                }
                edited = true;
            }
            if edited {
                app.sync_shared();
                app.mark_dirty();
            }

            // ------------------------------------------- weapon id learning
            ui.separator();
            ui.heading(t("手持武器识别", "Held weapon recognition"));
            ui.label(RichText::new(t("若内存中的 ID 正是游戏装备哈希，会自动识别（无需设置）；否则每个 ID 指定一次即可。",
                "If the in-memory ID is the game's equipment hash it is recognised automatically; otherwise assign each ID once.")).small());
            for (field, is_weapon) in [(Field::WeaponId, true), (Field::AmmoId, false)] {
                let Some(r) = app.live.raw.get(&field).copied() else {
                    ui.label(format!("{}: {}", field_name(field, lang), t("未绑定", "not bound")));
                    continue;
                };
                let key = format!("{r:X}");
                let auto = cotwmod::gamedata::by_hash(r).map(|s| s.to_string());
                let learned = if is_weapon { app.s.weapon_ids.get(&key).cloned() } else { app.s.ammo_ids.get(&key).cloned() };
                let name = |k: &str| if is_weapon { super::settings::weapon_name(k) } else { super::settings::ammo_name(k) };
                match (&auto, &learned) {
                    (Some(k), _) => { ui.colored_label(Color32::from_rgb(80, 200, 120), format!("0x{key} → {} ({})", name(k), t("自动", "auto"))); }
                    (None, Some(k)) => { ui.label(format!("0x{key} → {} ({})", name(k), t("已学习", "learned"))); }
                    (None, None) => { ui.colored_label(Color32::YELLOW, format!("0x{key} → {}", t("未知，请指定", "unknown, assign it"))); }
                }
                if auto.is_none() {
                    ui.horizontal(|ui| {
                        let pick = if is_weapon { &mut app.mem.weapon_pick } else { &mut app.mem.ammo_pick };
                        if pick.is_empty() {
                            *pick = if is_weapon { app.s.weapon.clone() } else { app.s.ammo.clone() };
                        }
                        let g = cotwmod::gamedata::gd();
                        egui::ComboBox::from_id_salt(("assign", is_weapon)).width(300.0).height(400.0).selected_text(name(pick)).show_ui(ui, |ui| {
                            let mut items: Vec<(String, String)> = if is_weapon {
                                g.weapons.iter().map(|(k, w)| (k.clone(), w.display.clone())).collect()
                            } else {
                                g.ammo.iter().map(|(k, a)| (k.clone(), a.display.clone())).collect()
                            };
                            items.sort_by(|a, b| a.1.cmp(&b.1));
                            for (k, d) in items {
                                ui.selectable_value(pick, k, d);
                            }
                        });
                        if ui.button(t("指定", "Assign")).clicked() {
                            let v = pick.clone();
                            if is_weapon { app.s.weapon_ids.insert(key.clone(), v); } else { app.s.ammo_ids.insert(key.clone(), v); }
                            app.mark_dirty();
                        }
                    });
                }
            }
            if !app.s.weapon_ids.is_empty() || !app.s.ammo_ids.is_empty() {
                ui.collapsing(t("已学习的 ID", "Learned IDs"), |ui| {
                    let mut del: Option<(bool, String)> = None;
                    for (is_w, map) in [(true, &app.s.weapon_ids), (false, &app.s.ammo_ids)] {
                        for (id, k) in map {
                            ui.horizontal(|ui| {
                                ui.label(format!("0x{id} → {}", if is_w { super::settings::weapon_name(k) } else { super::settings::ammo_name(k) }));
                                if ui.small_button("✖").clicked() {
                                    del = Some((is_w, id.clone()));
                                }
                            });
                        }
                    }
                    if let Some((w, id)) = del {
                        if w { app.s.weapon_ids.remove(&id); } else { app.s.ammo_ids.remove(&id); }
                        app.mark_dirty();
                    }
                });
            }

            // ------------------------------------------- view distance
            ui.separator();
            ui.heading(t("猎物可视距离", "Animal view distance"));
            let n = app.s.bindings.get(&Field::ViewDistance).map(|v| v.len()).unwrap_or(0);
            if n == 0 {
                ui.label(t("尚未绑定“猎物可视距离”地址。用左侧扫描找到后绑定（可绑定多个）。", "No 'Animal view distance' address bound yet. Find it with the scanner and bind it (several allowed)."));
            }
            let mut ch = ui.add(egui::Slider::new(&mut app.s.view_distance, 400.0..=800.0).step_by(10.0).suffix(" m")).changed();
            ui.horizontal(|ui| {
                ch |= ui.checkbox(&mut app.s.view_lock, t("持续锁定（每 0.25 秒写入）", "Keep locked (writes every 0.25 s)")).changed();
                if ui.add_enabled(n > 0 && process.is_some(), egui::Button::new(t("写入一次", "Write once"))).clicked() {
                    if let (Some(p), Some(bs)) = (&process, app.s.bindings.get(&Field::ViewDistance)) {
                        let ok = bs.iter().filter(|b| b.write(p.as_ref(), app.s.view_distance)).count();
                        app.status = format!("{} {ok}/{}", t("已写入", "Written"), bs.len());
                    }
                }
            });
            if ch {
                app.sync_shared();
                app.mark_dirty();
            }
            ui.label(RichText::new(t("提示：游戏可能在切换地图/画质时重置该值，开启“持续锁定”即可。距离过大会增加 CPU 负担。",
                "Tip: the game may reset this on map/quality changes — keep it locked. Larger distances cost CPU.")).small());
        });
    });
    if busy {
        ui.ctx().request_repaint();
    }
}

fn bind(app: &mut App, f: Field, b: Binding) {
    let v = app.s.bindings.entry(f).or_default();
    if f.multi() {
        if !v.contains(&b) {
            v.push(b);
        }
    } else {
        *v = vec![b];
    }
    app.sync_shared();
    app.mark_dirty();
    app.status = format!("{} {}", tr(app.s.lang, "已绑定", "Bound"), field_name(f, app.s.lang));
}
