use super::hotkeys::KEYS;
use super::settings::Lang;
use super::{tr, App};
use cotwmod::ballistics::{self, wind_components, Shot};
use eframe::egui::{self, Color32, RichText};

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let lang = app.s.lang;
    let t = |zh, en| tr(lang, zh, en);
    let mut changed = false;
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.columns(2, |cols| {
            // ---------------------------------------------------- left column
            let ui = &mut cols[0];
            ui.heading(t("武器", "Weapon"));
            egui::Grid::new("weapon").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label(t("武器", "Weapon"));
                let wname = |w: &super::settings::Weapon| if lang == Lang::Zh { w.name_zh.clone() } else { w.name_en.clone() };
                let cur = wname(app.s.weapon());
                egui::ComboBox::from_id_salt("w").width(260.0).selected_text(cur).show_ui(ui, |ui| {
                    for (k, w) in app.s.weapons.clone() {
                        changed |= ui.selectable_value(&mut app.s.weapon, k, wname(&w)).changed();
                    }
                });
                ui.end_row();
                if changed {
                    app.s.fix();
                }
                ui.label(t("箭矢", "Ammo"));
                let w = app.s.weapon().clone();
                let an = |a: &super::settings::Ammo| if lang == Lang::Zh { a.name_zh.clone() } else { a.name_en.clone() };
                egui::ComboBox::from_id_salt("a").width(260.0).selected_text(an(&w.ammo[&app.s.ammo])).show_ui(ui, |ui| {
                    for (k, a) in &w.ammo {
                        let label = format!("{}{}", an(a), if a.calibrated { " ✔" } else { "" });
                        changed |= ui.selectable_value(&mut app.s.ammo, k.clone(), label).changed();
                    }
                });
                ui.end_row();
                ui.label(t("归零距离", "Zero"));
                ui.horizontal(|ui| {
                    for z in &w.zero_options {
                        changed |= ui.selectable_value(&mut app.s.zero, *z, format!("{z:.0} m")).changed();
                    }
                });
                ui.end_row();
                if w.magnification.len() > 1 {
                    ui.label(t("倍率", "Zoom"));
                    ui.horizontal(|ui| {
                        for (i, m) in w.magnification.iter().enumerate() {
                            changed |= ui.selectable_value(&mut app.s.mag_index, i, format!("{m}x")).changed();
                        }
                    });
                    ui.end_row();
                }
            });
            ui.colored_label(Color32::LIGHT_YELLOW, t("⚠ 归零距离必须与游戏内设置一致。", "⚠ The zero must match the in-game setting."));
            {
                let am = app.s.weapons[&app.s.weapon].ammo.get_mut(&app.s.ammo).unwrap();
                ui.collapsing(t("弹道参数（校准会自动填写）", "Ballistic parameters (calibration fills these)"), |ui| {
                    egui::Grid::new("proj").show(ui, |ui| {
                        ui.label(t("初速 m/s", "Velocity m/s"));
                        changed |= ui.add(egui::DragValue::new(&mut am.proj.muzzle_velocity).range(10.0..=400.0).speed(0.1)).changed();
                        ui.end_row();
                        ui.label(t("阻力系数 k", "Drag k"));
                        changed |= ui.add(egui::DragValue::new(&mut am.proj.drag_k).range(0.0..=0.05).speed(0.00001).max_decimals(6)).changed();
                        ui.end_row();
                        ui.label(t("风偏系数", "Wind factor"));
                        changed |= ui.add(egui::DragValue::new(&mut am.proj.wind_factor).range(0.0..=10.0).speed(0.01)).changed();
                        ui.end_row();
                        ui.label(t("瞄具高度 m", "Sight height m"));
                        changed |= ui.add(egui::DragValue::new(&mut am.proj.sight_height).range(0.0..=0.5).speed(0.001)).changed();
                        ui.end_row();
                    });
                });
            }

            ui.separator();
            ui.heading(t("输入", "Inputs"));
            changed |= ui.checkbox(&mut app.s.use_live, t("使用内存实时数据（在“内存扫描”页绑定）", "Use live memory values (bind them on the Memory tab)")).changed();
            changed |= ui.checkbox(&mut app.s.auto_weapon, t("根据内存自动识别手持武器/箭矢", "Auto-detect held weapon/ammo from memory")).changed();
            egui::Grid::new("manual").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label(t("距离 m（手动）", "Distance m (manual)"));
                changed |= ui.add(egui::Slider::new(&mut app.s.distance, 1.0..=150.0).step_by(1.0)).changed();
                ui.end_row();
                ui.label(t("目标俯仰角 °（手动）", "Target incline ° (manual)"));
                changed |= ui.add(egui::Slider::new(&mut app.s.incline, -60.0..=60.0).step_by(1.0)).changed();
                ui.end_row();
                ui.label(t("风来向 °（0=迎面, 90=右, 270=左）", "Wind from ° (0=face, 90=right, 270=left)"));
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
            }

            // ---------------------------------------------------- right column
            let ui = &mut cols[1];
            ui.heading(t("叠加层", "Overlay"));
            changed |= ui.checkbox(&mut app.s.overlay_on, t("显示落点叠加层（Alt+F6）", "Show impact overlay (Alt+F6)")).changed();
            ui.label(RichText::new(t("游戏需设为“无边框窗口”。叠加层不会拦截鼠标。", "Run the game in Borderless mode. The overlay is click-through.")).small());
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
            ui.heading(t("校准", "Calibration"));
            ui.label(t("在靶场对已知距离射击：Alt+F11 进入校准模式，Alt+方向键把红框移到箭的落点，Alt+Insert 记录。每个距离射 2-3 箭，至少 3 个距离。",
                "At a range: Alt+F11 = calibration mode, Alt+arrows move the red box onto the arrow, Alt+Insert records. 2-3 arrows per distance, at least 3 distances."));
            ui.horizontal(|ui| {
                if ui.button(if app.calib_mode { t("退出校准模式", "Leave calibration mode") } else { t("进入校准模式", "Enter calibration mode") }).clicked() {
                    app.calib_mode = !app.calib_mode;
                }
                if ui.button(t("记录一发", "Record shot")).clicked() {
                    app.record_calibration_shot();
                }
            });
            let (w, a) = (app.s.weapon.clone(), app.s.ammo.clone());
            let mine: Vec<usize> = app.s.cal_shots.iter().enumerate().filter(|(_, s)| s.weapon == w && s.ammo == a).map(|(i, _)| i).collect();
            ui.label(format!("{}: {}", t("当前武器+箭矢的记录", "Shots for this weapon+ammo"), mine.len()));
            let mut del = None;
            egui::ScrollArea::vertical().id_salt("shots").max_height(140.0).show(ui, |ui| {
                for &i in &mine {
                    let s = &app.s.cal_shots[i];
                    ui.horizontal(|ui| {
                        ui.label(format!("{:.0} m  z{:.0}  {:+.2}/{:+.2} mrad  wind {:+.1}/{:+.1}", s.distance, s.zero, s.drop_mrad, s.drift_mrad, s.headwind, s.crosswind));
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
            if ui.add_enabled(!mine.is_empty(), egui::Button::new(t("拟合参数", "Fit parameters"))).clicked() {
                let shots: Vec<_> = mine.iter().map(|&i| app.s.cal_shots[i].clone()).collect();
                let init = app.s.projectile();
                match ballistics::fit(&init, &shots) {
                    Some((p, rep)) => {
                        let am = app.s.weapons[&w].ammo.get_mut(&a).unwrap();
                        am.proj = p;
                        am.calibrated = true;
                        app.status = format!("{} v0={:.1} k={:.5} wind={:.2}  rms={:.3} mrad ({})",
                            t("校准完成", "Calibrated"), p.muzzle_velocity, p.drag_k, p.wind_factor, rep.rms_mrad, rep.params.join(","));
                        changed = true;
                    }
                    None => app.status = t("拟合失败", "Fit failed").into(),
                }
            }

            ui.separator();
            ui.collapsing(t("弹道表", "Trajectory table"), |ui| {
                let p = app.s.projectile();
                let (hw, cw) = wind_components(app.s.wind_levels.get(app.s.wind_level).copied().unwrap_or(0.0), app.s.wind_rel);
                egui::Grid::new("table").striped(true).show(ui, |ui| {
                    for h in [t("距离", "Dist"), t("下坠 cm", "Drop cm"), "mrad", t("偏移 cm", "Drift cm"), "mrad", t("时间 s", "TOF s")] {
                        ui.strong(h);
                    }
                    ui.end_row();
                    for d in (5..=100).step_by(5) {
                        if let Some(r) = ballistics::solve(&p, app.s.zero, &Shot { distance: d as f64, incline_deg: 0.0, headwind: hw, crosswind: cw }) {
                            ui.label(format!("{d}"));
                            ui.label(format!("{:.1}", r.drop_m * 100.0));
                            ui.label(format!("{:.2}", r.drop_mrad));
                            ui.label(format!("{:.1}", r.drift_m * 100.0));
                            ui.label(format!("{:.2}", r.drift_mrad));
                            ui.label(format!("{:.3}", r.tof));
                            ui.end_row();
                        }
                    }
                });
            });
            ui.collapsing(t("热键", "Hotkeys"), |ui| {
                for (_, k, a) in KEYS {
                    ui.label(format!("{k:12}  {a:?}"));
                }
            });
        });
    });
    if changed {
        app.s.fix();
        app.mark_dirty();
    }
}
