use super::hotkeys::KEYS;
use super::settings::{ammo_name, weapon_name};
use super::{tr, App};
use cotwmod::ballistics::{self, wind_components, Shot};
use cotwmod::gamedata::{self, gd, Kind};
use cotwmod::memory::binding::Field;
use eframe::egui::{self, Color32, RichText};

thread_local! {
    static FILTER: std::cell::RefCell<String> = std::cell::RefCell::new(String::new());
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let lang = app.s.lang;
    let t = |zh, en| tr(lang, zh, en);
    let mut changed = false;
    let g = gd();
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.columns(2, |cols| {
            // ---------------------------------------------------- left column
            let ui = &mut cols[0];
            ui.heading(t("游戏数据", "Game data"));
            ui.label(RichText::new(if lang == super::settings::Lang::Zh { &app.data_status.0 } else { &app.data_status.1 }).small());
            if app.load_job.is_some() {
                ui.add(egui::ProgressBar::new(app.load_prog.done.load(std::sync::atomic::Ordering::Relaxed) as f32
                    / app.load_prog.total.load(std::sync::atomic::Ordering::Relaxed).max(1) as f32).show_percentage());
            }
            ui.horizontal(|ui| {
                ui.label(t("游戏文件夹", "Game folder"));
                changed |= ui.add(egui::TextEdit::singleline(&mut app.s.game_dir).desired_width(230.0).hint_text(t("留空 = 自动查找", "empty = auto-detect"))).changed();
                if ui.add_enabled(app.load_job.is_none(), egui::Button::new(t("重新读取", "Reload"))).clicked() {
                    app.reload_game_data(true);
                }
            });
            ui.separator();
            ui.heading(t("当前武器", "Current weapon"));
            let auto = app.s.use_live && app.s.auto_weapon && app.live.raw.contains_key(&Field::WeaponId);
            if auto {
                ui.colored_label(Color32::from_rgb(80, 200, 120), t("● 由内存自动识别手持武器", "● Held weapon detected from memory"));
            }
            ui.horizontal(|ui| {
                ui.label(t("武器", "Weapon"));
                let mut pick: Option<String> = None;
                egui::ComboBox::from_id_salt("w").width(330.0).height(420.0).selected_text(weapon_name(&app.s.weapon)).show_ui(ui, |ui| {
                    FILTER.with(|f| {
                        let mut f = f.borrow_mut();
                        ui.add(egui::TextEdit::singleline(&mut *f).hint_text(t("搜索…", "search…")).desired_width(300.0));
                        let q = f.to_lowercase();
                        if !app.s.recent.is_empty() && q.is_empty() {
                            ui.label(RichText::new(t("最近使用", "Recent")).small());
                            for k in app.s.recent.clone() {
                                if ui.selectable_label(k == app.s.weapon, weapon_name(&k)).clicked() {
                                    pick = Some(k);
                                }
                            }
                            ui.separator();
                        }
                        let mut list: Vec<(&String, &gamedata::Weapon)> = g.weapons.iter()
                            .filter(|(k, w)| q.is_empty() || w.display.to_lowercase().contains(&q) || k.contains(&q)).collect();
                        list.sort_by(|a, b| a.1.display.cmp(&b.1.display));
                        for (k, w) in list {
                            if ui.selectable_label(*k == app.s.weapon, &w.display).clicked() {
                                pick = Some(k.clone());
                            }
                        }
                    });
                });
                if let Some(k) = pick {
                    app.s.set_weapon(&k);
                    changed = true;
                }
            });
            let w = g.weapons[&app.s.weapon].clone();
            let tun = &g.tunings[&w.tuning];
            ui.horizontal(|ui| {
                ui.label(t("弹药", "Ammo"));
                egui::ComboBox::from_id_salt("a").width(330.0).selected_text(ammo_name(&app.s.ammo)).show_ui(ui, |ui| {
                    for k in &w.ammo {
                        changed |= ui.selectable_value(&mut app.s.ammo, k.clone(), ammo_name(k)).changed();
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.label(t("归零", "Zero"));
                for (i, z) in tun.zeros.iter().enumerate() {
                    changed |= ui.selectable_value(&mut app.s.zero_index, i, format!("{:.0} m", z.distance)).changed();
                }
                if app.live.values.contains_key(&Field::ZeroIndex) {
                    ui.colored_label(Color32::from_rgb(80, 200, 120), "● live");
                }
            });
            ui.horizontal(|ui| {
                ui.label(t("瞄具倍率", "Scope zoom"));
                for (i, m) in app.s.mags.clone().iter().enumerate() {
                    changed |= ui.selectable_value(&mut app.s.mag_index, i, format!("{m}x")).changed();
                }
            });
            if let Some(raw) = app.live.raw.get(&Field::WeaponId).copied() {
                if super::aim::resolve_id(raw, &app.s.weapon_ids).is_none() {
                    ui.colored_label(ui.visuals().warn_fg_color, format!("{} 0x{raw:X}",
                        t("检测到未识别的手持武器 ID：先在上面选中你现在拿的武器，再点“记住”", "Unrecognised held-weapon ID: select the weapon you are holding above, then Remember")));
                    if ui.button(t("记住", "Remember")).clicked() {
                        app.s.weapon_ids.insert(format!("{raw:X}"), app.s.weapon.clone());
                        changed = true;
                    }
                }
            }
            if let Some(raw) = app.live.raw.get(&Field::AmmoId).copied() {
                if super::aim::resolve_id(raw, &app.s.ammo_ids).is_none() {
                    ui.colored_label(ui.visuals().warn_fg_color, format!("{} 0x{raw:X}",
                        t("检测到未识别的弹药 ID：先选好当前弹药，再点“记住弹药”", "Unrecognised ammo ID: select the current ammo, then Remember ammo")));
                    if ui.button(t("记住弹药", "Remember ammo")).clicked() {
                        app.s.ammo_ids.insert(format!("{raw:X}"), app.s.ammo.clone());
                        changed = true;
                    }
                }
            }
            ui.label(RichText::new(t("⚠ 归零距离需与游戏内一致（或在“内存扫描”绑定“归零档位”自动同步）。",
                "⚠ The zero must match the game (or bind 'Zero index' on the Memory tab to sync it).")).small().color(ui.visuals().warn_fg_color));

            if let Some(st) = gamedata::setup(&app.s.weapon, &app.s.ammo, app.s.zero_index, &app.s.engine) {
                let a = &g.ammo[&app.s.ammo];
                ui.collapsing(t("游戏弹道数据", "Game ballistic data"), |ui| {
                    egui::Grid::new("gd").num_columns(2).show(ui, |ui| {
                        ui.label(t("动能", "Kinetic energy")); ui.label(format!("{:.0} J", st.ke)); ui.end_row();
                        ui.label(t("弹头质量", "Projectile mass")); ui.label(format!("{:.1} g", a.mass * 1000.0)); ui.end_row();
                        ui.label(t("直径 / 阻力系数", "Diameter / Cd")); ui.label(format!("{:.1} mm / {:.3}", a.diameter * 1000.0, a.cd)); ui.end_row();
                        ui.label(t("初速", "Muzzle velocity")); ui.label(format!("{:.1} m/s", st.v0)); ui.end_row();
                        ui.label(t("游戏归零角", "Game zero angle")); ui.label(format!("{:.3}°  ({:.0} m)", st.bore.to_degrees(), st.zero_distance)); ui.end_row();
                        ui.label(t("重力", "Gravity")); ui.label(format!("{:.2} m/s²", st.proj.gravity)); ui.end_row();
                        ui.label(t("速度随机偏差", "Velocity deviance")); ui.label(format!("{:.3}", a.velocity_deviance)); ui.end_row();
                        ui.label(t("最大射程", "Max range")); ui.label(format!("{:.0} m", a.max_range)); ui.end_row();
                    });
                });
            }

            ui.separator();
            ui.label(RichText::new(t("自动读取手持武器 / 归零 / 俯仰 / 视角 / 距离：在“快速开始”页一键设置。", "Held weapon / zero / pitch / FOV / distance are set up on the Quick start page.")).small());
            ui.separator();
            ui.heading(t("输入", "Inputs"));
            changed |= ui.checkbox(&mut app.s.use_live, t("使用内存实时数据", "Use live memory values")).changed();
            changed |= ui.checkbox(&mut app.s.auto_weapon, t("自动识别手持武器 / 弹药 / 归零", "Auto-detect held weapon / ammo / zero")).changed();
            let max_d = if gamedata::kind(&app.s.weapon) == Kind::Bow { 150.0 } else { 800.0 };
            egui::Grid::new("manual").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label(t("距离 m（手动）", "Distance m (manual)"));
                changed |= ui.add(egui::Slider::new(&mut app.s.distance, 1.0..=max_d).step_by(1.0)).changed();
                ui.end_row();
                ui.label(t("目标俯仰 °（手动）", "Target incline ° (manual)"));
                changed |= ui.add(egui::Slider::new(&mut app.s.incline, -60.0..=60.0).step_by(1.0)).changed();
                ui.end_row();
                ui.label(t("风来向 °（0迎面 90右 270左）", "Wind from ° (0 face, 90 R, 270 L)"));
                changed |= ui.add(egui::Slider::new(&mut app.s.wind_rel, 0.0..=359.0).step_by(5.0)).changed();
                ui.end_row();
                ui.label(t("风力等级", "Wind level"));
                let n = app.s.wind_levels.len().saturating_sub(1);
                changed |= ui.add(egui::Slider::new(&mut app.s.wind_level, 0..=n)).changed();
                ui.end_row();
            });

            ui.separator();
            ui.heading(t("当前落点", "Current prediction"));
            if let Some(a) = app.compute_aim() {
                let dot = |b: bool| if b { RichText::new("● live").color(Color32::from_rgb(80, 200, 120)) } else { RichText::new("○ manual").color(Color32::GRAY) };
                egui::Grid::new("pred").show(ui, |ui| {
                    ui.label(t("距离", "Distance")); ui.label(format!("{:.1} m", a.distance)); ui.label(dot(a.live_dist)); ui.end_row();
                    ui.label(t("俯仰", "Incline")); ui.label(format!("{:+.1}°", a.incline)); ui.label(dot(a.live_pitch)); ui.end_row();
                    ui.label(t("风速", "Wind")); ui.label(format!("{:.1} m/s", a.wind_speed)); ui.label(dot(a.live_wind)); ui.end_row();
                    ui.label(t("风来向", "Wind from")); ui.label(format!("{:.0}°", a.wind_rel)); ui.label(dot(a.live_dir)); ui.end_row();
                    ui.label(t("下坠", "Drop")); ui.label(format!("{:+.2} mrad", a.drop_mrad)); ui.end_row();
                    ui.label(t("偏移", "Drift")); ui.label(format!("{:+.2} mrad", a.drift_mrad)); ui.end_row();
                    ui.label(t("屏幕偏移", "Screen offset")); ui.label(format!("{:+.0}, {:+.0} px", a.dx, a.dy)); ui.end_row();
                    ui.label(t("飞行时间", "Time of flight")); ui.label(format!("{:.3} s", a.tof)); ui.end_row();
                });
            } else {
                ui.label(t("此武器/弹药组合没有可用的弹道数据。", "No ballistic data for this weapon/ammo."));
            }

            // ---------------------------------------------------- right column
            let ui = &mut cols[1];
            ui.heading(t("叠加层", "Overlay"));
            changed |= ui.checkbox(&mut app.s.overlay_on, t("显示落点叠加层（Alt+F6）", "Show impact overlay (Alt+F6)")).changed();
            ui.label(RichText::new(t("原生透明窗口，自动贴合游戏画面，只在游戏处于前台时显示，不拦截鼠标。游戏需为无边框窗口。",
                "Native transparent window fitted to the game, shown only while the game is in front, click-through. Game must be borderless.")).small());
            {
                let mut o = app.ov.lock().unwrap();
                ui.checkbox(&mut o.always_show, t("游戏不在前台时也显示（调试用）", "Show even when the game is not in front (setup)"));
                let state = if o.visible_now { t("● 叠加层显示中", "● overlay visible") } else { t("○ 叠加层未显示", "○ overlay hidden") };
                ui.label(RichText::new(state).small());
            }
            changed |= ui.checkbox(&mut app.s.auto_resolution, t("自动读取游戏分辨率（游戏窗口）", "Read game resolution automatically (game window)")).changed();
            match app.live.window {
                Some(w) => { ui.label(RichText::new(format!("{} {}×{}", t("游戏画面：", "Game view:"), w[2], w[3])).small()); }
                None => { ui.label(RichText::new(t("未找到游戏窗口", "Game window not found")).small()); }
            }
            changed |= ui.checkbox(&mut app.s.show_ladder, t("显示距离刻度", "Show range ladder")).changed();
            ui.horizontal(|ui| {
                ui.label(t("标记颜色", "Marker colour"));
                changed |= ui.color_edit_button_srgb(&mut app.s.marker_rgb).changed();
                ui.label(t("大小", "Size"));
                changed |= ui.add(egui::DragValue::new(&mut app.s.marker_radius).range(2.0..=30.0)).changed();
            });
            ui.collapsing(t("屏幕 / 视野", "Screen / FOV"), |ui| {
                egui::Grid::new("cam").show(ui, |ui| {
                    ui.label(t("分辨率", "Resolution"));
                    ui.horizontal(|ui| {
                        changed |= ui.add(egui::DragValue::new(&mut app.s.camera.screen_w).range(640.0..=7680.0)).changed();
                        ui.label("×");
                        changed |= ui.add(egui::DragValue::new(&mut app.s.camera.screen_h).range(480.0..=4320.0)).changed();
                    });
                    ui.end_row();
                    ui.label(t("游戏 FOV °", "Game FOV °"));
                    changed |= ui.add(egui::DragValue::new(&mut app.s.camera.fov_deg).range(30.0..=120.0).speed(0.5)).changed();
                    ui.end_row();
                    ui.label(t("FOV 为水平方向", "FOV is horizontal"));
                    changed |= ui.checkbox(&mut app.s.camera.fov_horizontal, "").changed();
                    ui.end_row();
                    ui.label(t("像素/mrad（0=按FOV计算）", "px per mrad (0 = from FOV)"));
                    changed |= ui.add(egui::DragValue::new(&mut app.s.camera.px_per_mrad).range(0.0..=50.0).speed(0.01)).changed();
                    ui.end_row();
                    ui.label(t("瞄准基准偏移 px", "Aim reference offset px"));
                    ui.horizontal(|ui| {
                        changed |= ui.add(egui::DragValue::new(&mut app.s.aim_ref[0]).speed(0.5)).changed();
                        changed |= ui.add(egui::DragValue::new(&mut app.s.aim_ref[1]).speed(0.5)).changed();
                    });
                    ui.end_row();
                });
            });

            ui.separator();
            ui.heading(t("引擎常数与自动校准", "Engine constants & auto-calibration"));
            changed |= ui.checkbox(&mut app.s.engine_auto, t("自动校准（启动时用游戏全部归零表拟合；每记录一发射击自动重新拟合）",
                "Auto-calibrate (fit to all of the game's zero tables at start; refit after every recorded shot)")).changed();
            if !app.fit_status.is_empty() {
                ui.label(RichText::new(format!("{} {}", t("归零表拟合误差 弓弩 RMS / 枪械中位数：", "Zero-table fit, bows RMS / guns median:"), app.fit_status)).small());
            }
            if app.fit_job.is_some() {
                ui.label(RichText::new(t("正在根据射击记录重新拟合…", "Refitting from shots…")).small());
            }
            egui::Grid::new("engine").show(ui, |ui| {
                let e = &mut app.s.engine;
                ui.label(t("发射点低于视线 m", "Launch below eye m")); changed |= ui.add(egui::DragValue::new(&mut e.launch_height).range(-0.3..=0.3).speed(0.001).max_decimals(3)).changed(); ui.end_row();
                ui.label(t("阻力倍数", "Drag multiplier")); changed |= ui.add(egui::DragValue::new(&mut e.drag_mul).range(0.0..=10.0).speed(0.01)).changed(); ui.end_row();
                ui.label(t("弓弩速度效率", "Bow velocity eff.")); changed |= ui.add(egui::DragValue::new(&mut e.vel_eff_bow).range(0.5..=1.5).speed(0.001).max_decimals(3)).changed(); ui.end_row();
                ui.label(t("枪械速度效率", "Gun velocity eff.")); changed |= ui.add(egui::DragValue::new(&mut e.vel_eff_gun).range(0.5..=1.5).speed(0.001).max_decimals(3)).changed(); ui.end_row();
                ui.label(t("风偏系数", "Wind factor")); changed |= ui.add(egui::DragValue::new(&mut e.wind_factor).range(0.0..=10.0).speed(0.01)).changed(); ui.end_row();
            });
            if ui.small_button(t("重新自动校准", "Re-run auto-calibration")).clicked() {
                app.reload_game_data(false);
            }
            ui.label(t("校准：靶场对已知距离射击，Alt+F11 进入校准模式，Alt+方向键把红框移到弹着点，Alt+Insert 记录。",
                "Calibrate: shoot known distances, Alt+F11 = calibration mode, Alt+arrows move the red box to the hit, Alt+Insert records."));
            ui.horizontal(|ui| {
                if ui.button(if app.calib.mode { t("退出校准模式", "Leave calibration mode") } else { t("进入校准模式", "Enter calibration mode") }).clicked() {
                    app.calib.mode = !app.calib.mode;
                }
                if ui.button(t("记录一发", "Record shot")).clicked() {
                    app.record_calibration_shot();
                }
                ui.label(format!("{}: {}", t("记录", "Shots"), app.s.cal_shots.len()));
            });
            let mut del = None;
            egui::ScrollArea::vertical().id_salt("shots").max_height(120.0).show(ui, |ui| {
                for (i, s) in app.s.cal_shots.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{} | {:.0} m z{:.0} | {:+.2}/{:+.2} mrad", weapon_name(&s.weapon), s.distance, s.zero, s.drop_mrad, s.drift_mrad)).small());
                        if ui.small_button("✖").clicked() {
                            del = Some(i);
                        }
                    });
                }
            });
            if let Some(i) = del {
                app.s.cal_shots.remove(i);
                changed = true;
            }
            if ui.add_enabled(app.s.cal_shots.len() >= 2 && app.fit_job.is_none(), egui::Button::new(t("立即用射击记录拟合", "Fit from shots now"))).clicked() {
                app.refit_from_shots();
            }

            ui.separator();
            ui.collapsing(t("弹道表", "Trajectory table"), |ui| {
                if let Some(st) = gamedata::setup(&app.s.weapon, &app.s.ammo, app.s.zero_index, &app.s.engine) {
                    let (hw, cw) = wind_components(app.s.wind_levels.get(app.s.wind_level).copied().unwrap_or(0.0), app.s.wind_rel);
                    let step = if gamedata::kind(&app.s.weapon) == Kind::Bow { 5 } else { 25 };
                    egui::Grid::new("table").striped(true).show(ui, |ui| {
                        for h in [t("距离", "Dist"), t("下坠 cm", "Drop cm"), "mrad", t("偏移 cm", "Drift cm"), "mrad", t("时间 s", "TOF s"), "m/s"] {
                            ui.strong(h);
                        }
                        ui.end_row();
                        for i in 1..=20 {
                            let d = (i * step) as f64;
                            if let Some(r) = ballistics::solve_bore(&st.proj, st.bore, st.bore_h, &Shot { distance: d, incline_deg: 0.0, headwind: hw, crosswind: cw }) {
                                ui.label(format!("{d:.0}"));
                                ui.label(format!("{:.1}", r.drop_m * 100.0));
                                ui.label(format!("{:.2}", r.drop_mrad));
                                ui.label(format!("{:.1}", r.drift_m * 100.0));
                                ui.label(format!("{:.2}", r.drift_mrad));
                                ui.label(format!("{:.3}", r.tof));
                                ui.label(format!("{:.0}", r.speed));
                                ui.end_row();
                            }
                        }
                    });
                }
            });
            ui.collapsing(t("热键", "Hotkeys"), |ui| {
                for (_, k, a) in KEYS {
                    ui.label(format!("{k:12}  {}", t(a.describe().0, a.describe().1)));
                }
            });
        });
    });
    if changed {
        app.s.fix();
        app.mark_dirty();
    }
}
