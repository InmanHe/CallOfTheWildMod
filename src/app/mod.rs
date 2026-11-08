pub mod aim;
mod gameload;
pub mod hotkeys;
pub mod overlay_win;
mod wizard;
pub mod live;
mod mem_tab;
mod pop_tab;
pub mod settings;
mod sight_tab;

use aim::{AimOut, Calib};
use cotwmod::ballistics::Camera;
use cotwmod::gamedata;
use cotwmod::memory::binding::Field;
use eframe::egui::{self, Color32};
use hotkeys::Action;
use live::{LiveValues, Shared};
use overlay_win::{Event, OverlayShared};
use settings::{Lang, Settings};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Population,
    Sight,
    Memory,
}

pub struct App {
    pub s: Settings,
    tab: Tab,
    pub shared: Arc<Mutex<Shared>>,
    pub live: LiveValues,
    pub calib: Calib,
    pub ov: Arc<Mutex<OverlayShared>>,
    pub wizard: wizard::Wizard,
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
        let ov = Arc::new(Mutex::new(OverlayShared::new(s.clone())));
        overlay_win::spawn(shared.clone(), ov.clone());
        let mut app = App {
            pop: pop_tab::PopState::new(&s),
            mem: mem_tab::MemState::default(),
            s,
            tab: Tab::Population,
            shared,
            live: LiveValues::default(),
            calib: Calib::default(),
            ov,
            wizard: wizard::Wizard::default(),
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

    #[allow(dead_code)]
    pub fn camera(&self) -> Camera {
        aim::camera(&self.s, &self.live)
    }

    pub fn compute_aim(&self) -> Option<AimOut> {
        aim::compute(&self.s, &self.live)
    }

    pub fn add_shot(&mut self, shot: cotwmod::ballistics::CalShot) {
        self.s.cal_shots.push(shot);
        self.status = format!("{} #{}", self.t("已记录校准射击", "Recorded calibration shot"), self.s.cal_shots.len());
        self.dirty = true;
        if self.s.engine_auto {
            self.refit_from_shots();
        }
    }

    pub fn record_calibration_shot(&mut self) {
        if let Some(shot) = aim::make_shot(&self.s, &self.live, &self.calib) {
            self.add_shot(shot);
        }
    }

    /// Exchange state with the overlay thread (it owns hotkeys and live weapon tracking).
    fn sync_overlay(&mut self) {
        let events: Vec<Event> = std::mem::take(&mut self.ov.lock().unwrap().events);
        for e in events {
            match e {
                Event::Action(Action::WeaponSwitched) => {
                    let p = self.shared.lock().unwrap().process.clone();
                    self.wizard.on_switch(p);
                }
                Event::Action(a) => {
                    aim::apply_action(&mut self.s, &mut self.calib, a);
                    self.dirty = true;
                }
                Event::Shot(shot) => self.add_shot(shot),
                Event::Weapon { weapon, ammo, zero_index } => {
                    if weapon != self.s.weapon {
                        self.s.set_weapon(&weapon);
                    }
                    self.s.ammo = ammo;
                    self.s.zero_index = zero_index;
                    self.s.fix();
                    self.dirty = true;
                }
            }
        }
        if aim::apply_live_weapon(&mut self.s, &self.live) {
            self.dirty = true;
        }
        let mut o = self.ov.lock().unwrap();
        o.settings = self.s.clone();
        o.calib = self.calib;
        o.status = self.status.clone();
    }

    fn tick_wizard(&mut self) {
        let p = self.shared.lock().unwrap().process.clone();
        if let Some(b) = self.wizard.tick(p) {
            self.s.bindings.insert(Field::WeaponId, vec![b]);
            self.sync_shared();
            self.dirty = true;
            self.status = self.t("手持武器已自动绑定", "Held weapon bound automatically").into();
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if (ctx.zoom_factor() - self.s.ui_scale).abs() > 0.001 {
            ctx.set_zoom_factor(self.s.ui_scale);
        }
        self.live = self.shared.lock().unwrap().out.clone();
        self.poll_jobs();
        self.tick_wizard();
        self.sync_overlay();

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
        self.ov.lock().unwrap().stop = true;
    }
}
