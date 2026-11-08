use cotwmod::ballistics::{CalShot, Camera, Projectile};
use cotwmod::memory::binding::{Binding, Field};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lang {
    Zh,
    En,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ammo {
    pub name_zh: String,
    pub name_en: String,
    pub proj: Projectile,
    #[serde(default)]
    pub calibrated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Weapon {
    pub name_zh: String,
    pub name_en: String,
    pub zero_options: Vec<f64>,
    pub default_zero: f64,
    pub magnification: Vec<f64>,
    pub ammo: IndexMap<String, Ammo>,
}

fn ammo(zh: &str, en: &str, v: f64, k: f64, sh: f64) -> Ammo {
    Ammo { name_zh: zh.into(), name_en: en.into(), proj: Projectile { muzzle_velocity: v, drag_k: k, wind_factor: 1.0, sight_height: sh }, calibrated: false }
}

pub fn default_weapons() -> IndexMap<String, Weapon> {
    let mut m = IndexMap::new();
    let arrows = |sh: f64, v3: f64, v4: f64, v6: f64| {
        IndexMap::from([
            ("arrow_300gr".to_string(), ammo("300格令箭", "300gr arrow", v3, 0.0010, sh)),
            ("arrow_420gr".to_string(), ammo("420格令箭", "420gr arrow", v4, 0.0008, sh)),
            ("arrow_600gr".to_string(), ammo("600格令箭", "600gr arrow", v6, 0.0006, sh)),
        ])
    };
    m.insert("bearclaw_cb60".into(), Weapon { name_zh: "Bearclaw Lite CB-60 复合弓".into(), name_en: "Bearclaw Lite CB-60 (compound)".into(),
        zero_options: vec![20.0, 40.0, 60.0], default_zero: 40.0, magnification: vec![1.0], ammo: arrows(0.08, 85.0, 75.0, 62.0) });
    m.insert("hawkedge_cb70".into(), Weapon { name_zh: "Hawk Edge CB-70 复合弓".into(), name_en: "Hawk Edge CB-70 (compound)".into(),
        zero_options: vec![20.0, 40.0, 60.0], default_zero: 40.0, magnification: vec![1.0], ammo: arrows(0.08, 92.0, 81.0, 67.0) });
    m.insert("crosspoint_cb165".into(), Weapon { name_zh: "Crosspoint CB-165 十字弩".into(), name_en: "Crosspoint CB-165 (crossbow)".into(),
        zero_options: vec![10.0, 30.0, 50.0], default_zero: 30.0, magnification: vec![1.0, 2.0, 4.0],
        ammo: IndexMap::from([
            ("bolt_300gr".to_string(), ammo("300格令弩箭", "300gr bolt", 105.0, 0.0009, 0.06)),
            ("bolt_420gr".to_string(), ammo("420格令弩箭", "420gr bolt", 95.0, 0.0007, 0.06)),
            ("bolt_600gr".to_string(), ammo("600格令弩箭", "600gr bolt", 80.0, 0.0005, 0.06)),
        ]) });
    m.insert("recurve".into(), Weapon { name_zh: "反曲弓（模板，请校准）".into(), name_en: "Recurve bow (template, calibrate)".into(),
        zero_options: vec![20.0, 40.0, 60.0], default_zero: 20.0, magnification: vec![1.0],
        ammo: IndexMap::from([
            ("arrow_420gr".to_string(), ammo("420格令箭", "420gr arrow", 58.0, 0.0008, 0.05)),
            ("arrow_600gr".to_string(), ammo("600格令箭", "600gr arrow", 49.0, 0.0006, 0.05)),
        ]) });
    m.insert("longbow".into(), Weapon { name_zh: "长弓（模板，请校准）".into(), name_en: "Longbow (template, calibrate)".into(),
        zero_options: vec![20.0, 40.0, 60.0], default_zero: 20.0, magnification: vec![1.0],
        ammo: IndexMap::from([
            ("arrow_420gr".to_string(), ammo("420格令箭", "420gr arrow", 55.0, 0.0008, 0.05)),
            ("arrow_600gr".to_string(), ammo("600格令箭", "600gr arrow", 46.0, 0.0006, 0.05)),
        ]) });
    m
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub lang: Lang,
    pub save_dir: String,
    pub weapons: IndexMap<String, Weapon>,
    pub weapon: String,
    pub ammo: String,
    pub zero: f64,
    pub mag_index: usize,
    pub distance: f64,
    pub incline: f64,
    pub wind_rel: f64,
    pub wind_level: usize,
    pub wind_levels: Vec<f64>,
    pub camera: Camera,
    pub aim_ref: [f64; 2],
    pub overlay_on: bool,
    pub show_ladder: bool,
    pub ladder: Vec<f64>,
    pub marker_rgb: [u8; 3],
    pub marker_radius: f32,
    pub use_live: bool,
    pub auto_weapon: bool,
    pub bindings: HashMap<Field, Vec<Binding>>,
    /// raw weapon id (hex string) -> weapon key
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
            save_dir: String::new(),
            weapons: default_weapons(),
            weapon: "hawkedge_cb70".into(),
            ammo: "arrow_420gr".into(),
            zero: 40.0,
            mag_index: 0,
            distance: 30.0,
            incline: 0.0,
            wind_rel: 0.0,
            wind_level: 0,
            wind_levels: vec![0.0, 1.5, 3.5, 6.0, 9.0],
            camera: Camera { screen_w: 1920.0, screen_h: 1080.0, fov_deg: 75.0, fov_horizontal: false, magnification: 1.0, px_per_mrad: 0.0 },
            aim_ref: [0.0, 0.0],
            overlay_on: false,
            show_ladder: true,
            ladder: vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0],
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
        // new built-in weapons appear after upgrades; keep user calibration
        for (k, w) in default_weapons() {
            s.weapons.entry(k).or_insert(w);
        }
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
        if !self.weapons.contains_key(&self.weapon) {
            self.weapon = self.weapons.keys().next().cloned().unwrap_or_default();
        }
        if let Some(w) = self.weapons.get(&self.weapon) {
            if !w.ammo.contains_key(&self.ammo) {
                self.ammo = w.ammo.keys().next().cloned().unwrap_or_default();
            }
            if !w.zero_options.iter().any(|z| (*z - self.zero).abs() < 1e-6) {
                self.zero = w.default_zero;
            }
            if self.mag_index >= w.magnification.len() {
                self.mag_index = 0;
            }
        }
        self.wind_level = self.wind_level.min(self.wind_levels.len().saturating_sub(1));
        self.distance = self.distance.clamp(1.0, 500.0);
        self.view_distance = self.view_distance.clamp(400.0, 800.0);
    }
    pub fn weapon(&self) -> &Weapon {
        &self.weapons[&self.weapon]
    }
    pub fn projectile(&self) -> Projectile {
        self.weapon().ammo[&self.ammo].proj
    }
    pub fn magnification(&self) -> f64 {
        self.weapon().magnification.get(self.mag_index).copied().unwrap_or(1.0)
    }
}
