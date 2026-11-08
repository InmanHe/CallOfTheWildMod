use cotwmod::ballistics::{CalShot, Camera};
use cotwmod::gamedata::{gd, Engine};
use cotwmod::memory::binding::{Binding, Field};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lang {
    Zh,
    En,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub lang: Lang,
    pub ui_scale: f32,
    pub save_dir: String,
    /// game folder override (empty = auto-detect)
    pub game_dir: String,
    /// automatically calibrate engine constants (game zero tables + recorded shots)
    pub engine_auto: bool,
    /// game equipment keys (from data/ballistics.json)
    pub weapon: String,
    pub ammo: String,
    pub zero_index: usize,
    pub recent: Vec<String>,
    pub engine: Engine,
    pub mags: Vec<f64>,
    pub mag_index: usize,
    pub distance: f64,
    pub incline: f64,
    pub wind_rel: f64,
    pub wind_level: usize,
    pub wind_levels: Vec<f64>,
    pub camera: Camera,
    /// take the resolution from the game window automatically
    pub auto_resolution: bool,
    pub aim_ref: [f64; 2],
    pub overlay_on: bool,
    pub show_ladder: bool,
    pub marker_rgb: [u8; 3],
    pub marker_radius: f32,
    pub use_live: bool,
    pub auto_weapon: bool,
    pub bindings: HashMap<Field, Vec<Binding>>,
    /// raw in-memory id (hex) -> game key, for ids that are not equipment hashes
    pub weapon_ids: HashMap<String, String>,
    pub ammo_ids: HashMap<String, String>,
    pub view_distance: f64,
    pub view_lock: bool,
    pub cal_shots: Vec<CalShot>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            lang: Lang::Zh,
            ui_scale: 0.85,
            save_dir: String::new(),
            game_dir: String::new(),
            engine_auto: true,
            weapon: "equipment_weapon_compound_bow_70lb_01".into(),
            ammo: "equipment_ammo_bh_bow_arrow_420gr_01".into(),
            zero_index: 0,
            recent: vec![],
            engine: Engine::default(),
            mags: vec![1.0, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0],
            mag_index: 0,
            distance: 30.0,
            incline: 0.0,
            wind_rel: 0.0,
            wind_level: 0,
            wind_levels: vec![0.0, 1.5, 3.5, 6.0, 9.0],
            auto_resolution: true,
            camera: Camera { screen_w: 1920.0, screen_h: 1080.0, fov_deg: 75.0, fov_horizontal: false, magnification: 1.0, px_per_mrad: 0.0 },
            aim_ref: [0.0, 0.0],
            overlay_on: false,
            show_ladder: true,
            marker_rgb: [0x33, 0xFF, 0x66],
            marker_radius: 6.0,
            use_live: true,
            auto_weapon: true,
            bindings: HashMap::new(),
            weapon_ids: HashMap::new(),
            ammo_ids: HashMap::new(),
            view_distance: 600.0,
            view_lock: false,
            cal_shots: vec![],
        }
    }
}

pub fn config_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("CotwMod")
}

impl Settings {
    pub fn load() -> Settings {
        let p = config_dir().join("settings.json");
        let mut s: Settings = std::fs::read_to_string(&p).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        s.fix();
        s
    }
    pub fn save(&self) {
        let d = config_dir();
        let _ = std::fs::create_dir_all(&d);
        if let Ok(t) = serde_json::to_string_pretty(self) {
            let tmp = d.join("settings.json.tmp");
            if std::fs::write(&tmp, t).is_ok() {
                let _ = std::fs::rename(tmp, d.join("settings.json"));
            }
        }
    }
    pub fn fix(&mut self) {
        let g = gd();
        if !g.weapons.contains_key(&self.weapon) {
            self.weapon = Settings::default().weapon;
        }
        let w = &g.weapons[&self.weapon];
        if !w.ammo.contains(&self.ammo) {
            self.ammo = w.ammo.iter().find(|a| !a.contains("tracer")).or(w.ammo.first()).cloned().unwrap_or_default();
        }
        let nz = g.tunings[&w.tuning].zeros.len();
        if self.zero_index >= nz {
            self.zero_index = 0;
        }
        if self.mags.is_empty() {
            self.mags = vec![1.0];
        }
        if self.mag_index >= self.mags.len() {
            self.mag_index = 0;
        }
        self.wind_level = self.wind_level.min(self.wind_levels.len().saturating_sub(1));
        self.distance = self.distance.clamp(1.0, 1000.0);
        self.view_distance = self.view_distance.clamp(400.0, 800.0);
        self.ui_scale = self.ui_scale.clamp(0.6, 1.5);
        self.recent.retain(|k| g.weapons.contains_key(k));
    }
    pub fn set_weapon(&mut self, key: &str) {
        if self.weapon != key {
            self.weapon = key.to_string();
            self.ammo.clear();
            self.zero_index = 0;
        }
        self.recent.retain(|k| k != key);
        self.recent.insert(0, key.to_string());
        self.recent.truncate(6);
        self.fix();
    }
    pub fn magnification(&self) -> f64 {
        self.mags.get(self.mag_index).copied().unwrap_or(1.0)
    }
    pub fn zero_distance(&self) -> f64 {
        let g = gd();
        g.tunings[&g.weapons[&self.weapon].tuning].zeros.get(self.zero_index).map(|z| z.distance).unwrap_or(0.0)
    }
}

pub fn weapon_name(key: &str) -> String {
    gd().weapons.get(key).map(|w| w.display.clone()).unwrap_or_else(|| key.to_string())
}

pub fn ammo_name(key: &str) -> String {
    gd().ammo.get(key).map(|a| a.display.clone()).unwrap_or_else(|| key.to_string())
}
