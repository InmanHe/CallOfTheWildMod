mod gameload;
pub mod hotkeys;
pub mod live;
mod mem_tab;
mod pop_tab;
pub mod settings;
mod sight_tab;

use cotwmod::ballistics::{self, relative_dir, wind_components, Camera, Shot};
use cotwmod::gamedata::{self, gd, Kind};
use cotwmod::memory::binding::Field;
use eframe::egui::{self, Color32, FontId, Pos2, Stroke, Vec2};
use hotkeys::{Action, Hotkeys};
use live::{LiveValues, Shared};
use settings::{ammo_name, weapon_name, Lang, Settings};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Population,
    Sight,
    Memory,
}

pub struct AimOut {
    pub dx: f64,
    pub dy: f64,
    pub drop_mrad: f64,
    pub drift_mrad: f64,
    pub tof: f64,
    pub distance: f64,
    pub incline: f64,
    pub wind_speed: f64,
    pub wind_rel: f64,
    pub live_dist: bool,
    pub live_pitch: bool,
    pub live_wind: bool,
    pub live_dir: bool,
    pub ladder: Vec<(f64, f64, f64)>,
}

pub struct App {
    pub s: Settings,
    tab: Tab,
    pub shared: Arc<Mutex<Shared>>,
    pub live: LiveValues,
    hotkeys: Hotkeys,
    pub calib_mode: bool,
    pub calib_marker: [f64; 2],
    pub status: String,
    last_save: Instant,
    dirty: bool,
    pop: pop_tab::PopState,
    mem: mem_tab::MemState,
    pub load_job: Option<std::thread::JoinHandle<gameload::LoadResult>>,
    pub load_prog: Arc<cotwmod::gamefiles::Progress>,
    pub data_status: (String, String),
    pub fit_status: String,
    pub fit_job: Option<std::thread::JoinHandle<Option<(gamedata::Engine, f64, f64, Vec<&'static str>)>>>,
}

pub fn tr(lang: Lang, zh: &'static str, en: &'static str) -> &'static str {
    if lang == Lang::Zh { zh } else { en }
}

fn load_cjk_font(ctx: &egui::Context) {
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    let candidates = ["msyh.ttc", "msyh.ttf", "simhei.ttf", "simsun.ttc", "msjh.ttc", "Deng.ttf"];
    for c in candidates {
        let p = std::path::Path::new(&windir).join("Fonts").join(c);
        if let Ok(bytes) = std::fs::read(&p) {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert("cjk".into(), Arc::new(egui::FontData::from_owned(bytes)));
            for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts.families.entry(fam).or_default().push("cjk".into());
            }
            ctx.set_fonts(fonts);
            return;
        }
    }
}

impl App {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        load_cjk_font(&cc.egui_ctx);
        let s = Settings::load();
        let shared = Arc::new(Mutex::new(Shared { bindings: s.bindings.clone(), ..Default::default() }));
        live::spawn(shared.clone());
        let mut app = App {
            pop: pop_tab::PopState::new(&s),
            mem: mem_tab::MemState::default(),
            s,
            tab: Tab::Population,
            shared,
            live: LiveValues::default(),
            hotkeys: Hotkeys::new(),
            calib_mode: false,
            calib_marker: [0.0, 0.0],
            status: String::new(),
            last_save: Instant::now(),
            dirty: false,
            load_job: None,
            load_prog: Arc::new(Default::default()),
            data_status: ("正在读取游戏数据…".into(), "Reading game data…".into()),
            fit_status: String::new(),
            fit_job: None,
        };
        app.sync_shared();
        app.reload_game_data(false);
        app
    }

    pub fn reload_game_data(&mut self, force: bool) {
        if self.load_job.is_some() {
            return;
        }
        self.load_prog = Arc::new(Default::default());
        self.data_status = ("正在读取游戏数据…".into(), "Reading game data…".into());
        self.load_job = Some(gameload::spawn(self.s.game_dir.clone(), force, gamedata::Engine::default(), self.load_prog.clone()));
    }

    /// Dynamic calibration: refit the engine constants from recorded shots in the background.
    pub fn refit_from_shots(&mut self) {
        if self.fit_job.is_none() && self.s.cal_shots.len() >= 2 {
            self.fit_job = Some(gameload::spawn_shot_fit(self.s.engine, self.s.cal_shots.clone()));
        }
    }

    fn poll_jobs(&mut self) {
        if self.load_job.as_ref().map(|h| h.is_finished()).unwrap_or(false) {
            if let Ok(r) = self.load_job.take().unwrap().join() {
                self.data_status = (
                    format!("{}（{} 把武器 / {} 种弹药）", r.msg_zh, r.weapons, r.ammo),
                    format!("{} ({} weapons / {} ammo)", r.msg_en, r.weapons, r.ammo),
                );
                if let Some((e, rb, mg, n)) = r.fit {
                    self.fit_status = format!("{rb:.2} / {mg:.2} mrad ({n})");
                    if self.s.engine_auto {
                        self.s.engine = gamedata::Engine { wind_factor: self.s.engine.wind_factor, ..e };
                        self.dirty = true;
                        self.refit_from_shots();
                    }
                }
                self.s.fix();
            }
        }
        if self.fit_job.as_ref().map(|h| h.is_finished()).unwrap_or(false) {
            if let Ok(Some((e, rms, max, names))) = self.fit_job.take().unwrap().join() {
                self.s.engine = e;
                self.dirty = true;
                self.status = format!("{} rms {rms:.3} mrad, max {max:.3} ({})", self.t("已根据射击记录自动校准", "Auto-calibrated from recorded shots"), names.join(", "));
            }
        }
    }

    pub fn t(&self, zh: &'static str, en: &'static str) -> &'static str {
        tr(self.s.lang, zh, en)
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn sync_shared(&mut self) {
        let mut sh = self.shared.lock().unwrap();
        sh.bindings = self.s.bindings.clone();
        sh.view_lock = self.s.view_lock.then_some(self.s.view_distance);
    }

    pub fn camera(&self) -> Camera {
        let mut c = self.s.camera;
        c.magnification = self.s.magnification();
        if self.s.use_live {
            if let Some(f) = self.live.values.get(&Field::CameraFov) {
                if *f > 1.0 && *f < 170.0 {
                    c.fov_deg = *f;
                    c.magnification = 1.0;
                }
            }
        }
        c
    }

    /// Resolve a raw in-memory id: equipment-name hash first, then learned ids.
    pub fn resolve_id(&self, raw: u64, learned: &std::collections::HashMap<String, String>) -> Option<String> {
        gamedata::by_hash(raw).map(|s| s.to_string()).or_else(|| learned.get(&format!("{raw:X}")).cloned())
    }

    fn apply_auto_weapon(&mut self) {
        if !(self.s.use_live && self.s.auto_weapon) {
            return;
        }
        if let Some(raw) = self.live.raw.get(&Field::WeaponId).copied() {
            if let Some(k) = self.resolve_id(raw, &self.s.weapon_ids) {
                if gd().weapons.contains_key(&k) && k != self.s.weapon {
                    self.s.set_weapon(&k);
                    self.dirty = true;
                }
            }
        }
        if let Some(raw) = self.live.raw.get(&Field::AmmoId).copied() {
            if let Some(k) = self.resolve_id(raw, &self.s.ammo_ids) {
                if gd().weapons[&self.s.weapon].ammo.contains(&k) && k != self.s.ammo {
                    self.s.ammo = k;
                    self.dirty = true;
                }
            }
        }
        if let Some(z) = self.live.values.get(&Field::ZeroIndex).copied() {
            let n = gd().tunings[&gd().weapons[&self.s.weapon].tuning].zeros.len();
            if z >= 0.0 && (z as usize) < n && z as usize != self.s.zero_index {
                self.s.zero_index = z as usize;
                self.dirty = true;
            }
        }
    }

    pub fn compute_aim(&self) -> Option<AimOut> {
        let s = &self.s;
        let lv = |f: Field| if s.use_live { self.live.values.get(&f).copied().filter(|v| v.is_finite()) } else { None };
        let live_d = lv(Field::AimDistance).filter(|d| *d > 0.5 && *d < 1000.0);
        let distance = live_d.unwrap_or(s.distance);
        let pitch = lv(Field::CameraPitch);
        let live_ws = lv(Field::WindSpeed);
        let wind_speed = live_ws.unwrap_or(s.wind_levels.get(s.wind_level).copied().unwrap_or(0.0));
        let (wind_rel, live_dir) = match (lv(Field::WindFrom), lv(Field::CameraYaw)) {
            (Some(w), Some(y)) => (relative_dir(w, y), true),
            _ => (s.wind_rel, false),
        };
        let (hw, cw) = wind_components(wind_speed, wind_rel);
        let st = gamedata::setup(&s.weapon, &s.ammo, s.zero_index, &s.engine)?;
        let incline = pitch.unwrap_or(s.incline);
        let shot = Shot { distance, incline_deg: incline, headwind: hw, crosswind: cw };
        let sol = if pitch.is_some() {
            ballistics::solve_bore(&st.proj, st.bore, st.bore_h, &shot)?
        } else {
            ballistics::solve_bore_hold(&st.proj, st.bore, st.bore_h, &shot)?
        };
        let cam = self.camera();
        let (dx, dy) = cam.impact_px(sol.drop_mrad, sol.drift_mrad);
        let mut ladder = vec![];
        if s.show_ladder {
            let steps: Vec<f64> = if gamedata::kind(&s.weapon) == Kind::Bow {
                (1..=8).map(|i| i as f64 * 10.0).collect()
            } else {
                (1..=8).map(|i| i as f64 * 50.0).collect()
            };
            for d in steps {
                if let Some(l) = ballistics::solve_bore(&st.proj, st.bore, st.bore_h, &Shot { distance: d, ..shot }) {
                    let (lx, ly) = cam.impact_px(l.drop_mrad, l.drift_mrad);
                    ladder.push((d, lx + s.aim_ref[0], ly + s.aim_ref[1]));
                }
            }
        }
        Some(AimOut {
            dx: dx + s.aim_ref[0],
            dy: dy + s.aim_ref[1],
            drop_mrad: sol.drop_mrad,
            drift_mrad: sol.drift_mrad,
            tof: sol.tof,
            distance,
            incline,
            wind_speed,
            wind_rel,
            live_dist: live_d.is_some(),
            live_pitch: pitch.is_some(),
            live_wind: live_ws.is_some(),
            live_dir,
            ladder,
        })
    }

    fn handle_action(&mut self, a: Action) {
        match a {
            Action::CalibMode => {
                self.calib_mode = !self.calib_mode;
                return;
            }
            Action::CalibRecord => {
                self.record_calibration_shot();
                return;
            }
            _ => {}
        }
        if self.calib_mode {
            let nudge = match a {
                Action::WindCw => Some((1.0, 0.0)),
                Action::WindCcw => Some((-1.0, 0.0)),
                Action::WindUp => Some((0.0, -1.0)),
                Action::WindDown => Some((0.0, 1.0)),
                _ => None,
            };
            if let Some((x, y)) = nudge {
                self.calib_marker[0] += x;
                self.calib_marker[1] += y;
                return;
            }
        }
        let s = &mut self.s;
        match a {
            Action::ToggleOverlay => s.overlay_on = !s.overlay_on,
            Action::NextWeapon => {
                // cycle through recently used weapons
                if s.recent.len() > 1 {
                    let i = s.recent.iter().position(|k| *k == s.weapon).map(|i| (i + 1) % s.recent.len()).unwrap_or(0);
                    let k = s.recent[i].clone();
                    s.weapon = k;
                    s.ammo.clear();
                    s.zero_index = 0;
                }
            }
            Action::NextAmmo => {
                let keys = gd().weapons[&s.weapon].ammo.clone();
                let i = keys.iter().position(|k| *k == s.ammo).map(|i| (i + 1) % keys.len()).unwrap_or(0);
                s.ammo = keys[i].clone();
            }
            Action::NextZero => {
                let n = gd().tunings[&gd().weapons[&s.weapon].tuning].zeros.len().max(1);
                s.zero_index = (s.zero_index + 1) % n;
            }
            Action::NextMag => s.mag_index = (s.mag_index + 1) % s.mags.len().max(1),
            Action::DistPlus1 => s.distance += 1.0,
            Action::DistMinus1 => s.distance -= 1.0,
            Action::DistPlus5 => s.distance += 5.0,
            Action::DistMinus5 => s.distance -= 5.0,
            Action::WindCw => s.wind_rel = (s.wind_rel + 45.0).rem_euclid(360.0),
            Action::WindCcw => s.wind_rel = (s.wind_rel - 45.0).rem_euclid(360.0),
            Action::WindUp => s.wind_level += 1,
            Action::WindDown => s.wind_level = s.wind_level.saturating_sub(1),
            Action::CalibMode | Action::CalibRecord => {}
        }
        s.fix();
        self.dirty = true;
    }

    pub fn record_calibration_shot(&mut self) {
        let Some(a) = self.compute_aim() else { return };
        let cam = self.camera();
        let (drop, drift) = cam.px_to_mrad(self.calib_marker[0] - self.s.aim_ref[0], self.calib_marker[1] - self.s.aim_ref[1]);
        let (hw, cw) = wind_components(a.wind_speed, a.wind_rel);
        self.s.cal_shots.push(ballistics::CalShot {
            weapon: self.s.weapon.clone(),
            ammo: self.s.ammo.clone(),
            distance: a.distance,
            zero: self.s.zero_distance(),
            zero_index: self.s.zero_index,
            incline_deg: a.incline,
            headwind: hw,
            crosswind: cw,
            drop_mrad: drop,
            drift_mrad: drift,
        });
        self.status = format!("{} #{}", self.t("已记录校准射击", "Recorded calibration shot"), self.s.cal_shots.len());
        self.dirty = true;
        if self.s.engine_auto {
            self.refit_from_shots();
        }
    }

    fn draw_overlay(&mut self, ctx: &egui::Context, aim: Option<&AimOut>) {
        if !self.s.overlay_on {
            return;
        }
        let monitor = ctx.input(|i| i.viewport().monitor_size).unwrap_or(Vec2::new(1920.0, 1080.0));
        let builder = egui::ViewportBuilder::default()
            .with_title("CotwMod Overlay")
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_mouse_passthrough(true)
            .with_taskbar(false)
            .with_position(Pos2::ZERO)
            .with_inner_size(monitor);
        let rgb = self.s.marker_rgb;
        let color = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
        let radius = self.s.marker_radius;
        let lang = self.s.lang;
        let wname = weapon_name(&self.s.weapon);
        let aname = ammo_name(&self.s.ammo);
        let calibrated = !self.s.cal_shots.is_empty() && self.s.engine != gamedata::Engine::default();
        let zero = self.s.zero_distance();
        let calib = self.calib_mode.then_some(self.calib_marker);
        let status = self.status.clone();
        let aim = aim.map(|a| (a.dx, a.dy, a.ladder.clone(), a.drop_mrad, a.drift_mrad, a.tof, a.distance, a.incline,
            a.wind_speed, a.wind_rel, [a.live_dist, a.live_pitch, a.live_wind, a.live_dir]));
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("cotwmod_overlay"), builder, move |ctx, _| {
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| {
                let ppp = ctx.pixels_per_point() as f64;
                let rect = ui.max_rect();
                let c = rect.center();
                let p = ui.painter();
                let px = |dx: f64, dy: f64| Pos2::new(c.x + (dx / ppp) as f32, c.y + (dy / ppp) as f32);
                let white = Color32::from_rgba_unmultiplied(255, 255, 255, 150);
                p.line_segment([c - Vec2::new(4.0, 0.0), c + Vec2::new(4.0, 0.0)], Stroke::new(1.0f32, white));
                p.line_segment([c - Vec2::new(0.0, 4.0), c + Vec2::new(0.0, 4.0)], Stroke::new(1.0f32, white));
                let mut lines: Vec<String> = vec![format!("{wname} | {aname}{}", if calibrated { tr(lang, "  [已校准]", "  [calibrated]") } else { "" })];
                if let Some((dx, dy, ladder, drop, drift, tof, dist, inc, ws, wr, live)) = &aim {
                    let mut last_y = f32::NAN;
                    for (d, lx, ly) in ladder {
                        let q = px(*lx, *ly);
                        p.line_segment([q - Vec2::new(6.0, 0.0), q + Vec2::new(6.0, 0.0)], Stroke::new(1.0f32, white));
                        if last_y.is_nan() || (q.y - last_y).abs() >= 11.0 {
                            p.text(q + Vec2::new(9.0, 0.0), egui::Align2::LEFT_CENTER, format!("{d:.0}"), FontId::monospace(10.0), white);
                            last_y = q.y;
                        }
                    }
                    let m = px(*dx, *dy);
                    let st = Stroke::new(2.0f32, color);
                    p.circle_stroke(m, radius, st);
                    for (a, b) in [(-2.0, -1.0), (1.0, 2.0)] {
                        p.line_segment([m + Vec2::new(radius * a, 0.0), m + Vec2::new(radius * b, 0.0)], st);
                        p.line_segment([m + Vec2::new(0.0, radius * a), m + Vec2::new(0.0, radius * b)], st);
                    }
                    p.circle_filled(m, 1.2, color);
                    let dot = |b: bool| if b { "●" } else { "○" };
                    lines.push(format!("{} {zero:.0} m", tr(lang, "归零", "Zero")));
                    lines.push(format!("{} {} {dist:5.1} m   {} {} {inc:+.1}°", dot(live[0]), tr(lang, "距离", "Dist"), dot(live[1]), tr(lang, "俯仰", "Incl")));
                    lines.push(format!("{} {} {ws:.1} m/s   {} {} {wr:.0}°", dot(live[2]), tr(lang, "风速", "Wind"), dot(live[3]), tr(lang, "来向", "from")));
                    lines.push(format!("{} {drop:+.1} mrad  {} {drift:+.1} mrad  {tof:.2}s", tr(lang, "下坠", "Drop"), tr(lang, "偏移", "Drift")));
                }
                if let Some(cm) = calib {
                    let m = px(cm[0], cm[1]);
                    p.rect_stroke(egui::Rect::from_center_size(m, Vec2::splat(10.0)), 0.0, Stroke::new(2.0f32, Color32::from_rgb(255, 51, 102)), egui::StrokeKind::Middle);
                    lines.push(format!("{} ({:+.0},{:+.0}px)", tr(lang, "[校准] Alt+方向键移动红框到箭落点, Alt+Insert 记录", "[CALIB] Alt+arrows move box to arrow, Alt+Insert records"), cm[0], cm[1]));
                }
                if !status.is_empty() {
                    lines.push(status.clone());
                }
                let mut y = rect.top() + 24.0;
                for l in lines {
                    let pos = Pos2::new(rect.left() + 20.0, y);
                    p.text(pos + Vec2::new(1.0, 1.0), egui::Align2::LEFT_TOP, &l, FontId::proportional(14.0), Color32::from_black_alpha(220));
                    p.text(pos, egui::Align2::LEFT_TOP, &l, FontId::proportional(14.0), Color32::from_rgb(230, 255, 230));
                    y += 18.0;
                }
            });
        });
    }
}

impl eframe::App for App {
    fn clear_color(&self, _v: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if (ctx.zoom_factor() - self.s.ui_scale).abs() > 0.001 {
            ctx.set_zoom_factor(self.s.ui_scale);
        }
        self.live = self.shared.lock().unwrap().out.clone();
        self.poll_jobs();
        for a in self.hotkeys.poll() {
            self.handle_action(a);
        }
        self.apply_auto_weapon();

        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("CotwMod");
                ui.separator();
                for (t, zh, en) in [(Tab::Population, "种群编辑", "Population"), (Tab::Sight, "落点瞄具", "Impact sight"), (Tab::Memory, "内存扫描", "Memory")] {
                    if ui.selectable_label(self.tab == t, tr(self.s.lang, zh, en)).clicked() {
                        self.tab = t;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut lang = self.s.lang;
                    ui.selectable_value(&mut lang, Lang::En, "English");
                    ui.selectable_value(&mut lang, Lang::Zh, "简体中文");
                    if lang != self.s.lang {
                        self.s.lang = lang;
                        self.dirty = true;
                    }
                    ui.separator();
                    let mut sc = self.s.ui_scale;
                    if ui.add(egui::DragValue::new(&mut sc).range(0.6..=1.5).speed(0.01).max_decimals(2).prefix(tr(self.s.lang, "界面 ", "UI "))).changed() {
                        self.s.ui_scale = sc;
                        self.dirty = true;
                    }
                    ui.separator();
                    let (txt, col) = if self.live.attached {
                        (tr(self.s.lang, "● 游戏已连接", "● game attached"), Color32::from_rgb(80, 200, 120))
                    } else {
                        (tr(self.s.lang, "○ 游戏未运行", "○ game not running"), Color32::GRAY)
                    };
                    ui.colored_label(col, txt);
                });
            });
        });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.label(&self.status);
        });
        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Population => pop_tab::ui(self, ui),
            Tab::Sight => sight_tab::ui(self, ui),
            Tab::Memory => mem_tab::ui(self, ui),
        });

        let aim = self.compute_aim();
        self.draw_overlay(ctx, aim.as_ref());

        if self.dirty && self.last_save.elapsed().as_secs_f32() > 1.5 {
            self.s.save();
            self.sync_shared();
            self.dirty = false;
            self.last_save = Instant::now();
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.s.save();
        self.shared.lock().unwrap().stop = true;
    }
}
