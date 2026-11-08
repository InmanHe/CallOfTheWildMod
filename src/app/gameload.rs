//! Background loading of the installed game's ballistic data + automatic
//! calibration of the engine constants.

use super::settings::config_dir;
use cotwmod::ballistics::CalShot;
use cotwmod::gamedata::{self, Engine, GameData};
use cotwmod::gamefiles::{self, Progress};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread::JoinHandle;

pub struct LoadResult {
    pub ok: bool,
    pub msg_zh: String,
    pub msg_en: String,
    pub weapons: usize,
    pub ammo: usize,
    pub fit: Option<(Engine, f64, f64, usize)>,
}

#[derive(Serialize, Deserialize)]
struct Cache {
    fingerprint: String,
    data: GameData,
}

fn cache_path() -> PathBuf {
    config_dir().join("gamedata_cache.json")
}

/// Load game data (cache or archives), install it globally, then fit the
/// engine constants to every zero table in it.
pub fn spawn(game_dir: String, force: bool, e0: Engine, prog: Arc<Progress>) -> JoinHandle<LoadResult> {
    std::thread::spawn(move || {
        let dirs: Vec<PathBuf> = if game_dir.trim().is_empty() { gamefiles::locate_game() } else { vec![PathBuf::from(game_dir.trim())] };
        let mut res = LoadResult { ok: false, msg_zh: String::new(), msg_en: String::new(), weapons: 0, ammo: 0, fit: None };
        let mut loaded = false;
        for d in &dirs {
            let Some(ad) = gamefiles::archives_dir(d) else { continue };
            let fp = gamefiles::fingerprint(&ad);
            if !force {
                if let Some(c) = std::fs::read_to_string(cache_path()).ok().and_then(|t| serde_json::from_str::<Cache>(&t).ok()) {
                    if c.fingerprint == fp && !c.data.weapons.is_empty() {
                        gamedata::set_gd(c.data);
                        res.msg_zh = format!("已从本机游戏读取（缓存）：{}", d.display());
                        res.msg_en = format!("Loaded from your game (cached): {}", d.display());
                        loaded = true;
                        break;
                    }
                }
            }
            match gamefiles::build(&ad, &prog) {
                Ok(g) => {
                    let _ = std::fs::create_dir_all(config_dir());
                    if let Ok(t) = serde_json::to_string(&Cache { fingerprint: fp, data: g.clone() }) {
                        let _ = std::fs::write(cache_path(), t);
                    }
                    gamedata::set_gd(g);
                    res.msg_zh = format!("已从本机游戏文件读取：{}", d.display());
                    res.msg_en = format!("Read from your game files: {}", d.display());
                    loaded = true;
                    break;
                }
                Err(e) => {
                    res.msg_zh = format!("读取游戏文件失败（{e:#}），使用内置数据");
                    res.msg_en = format!("Reading game files failed ({e:#}); using built-in data");
                }
            }
        }
        if !loaded && res.msg_zh.is_empty() {
            res.msg_zh = "未找到游戏安装目录，使用内置数据（可在下方填写游戏文件夹）".into();
            res.msg_en = "Game folder not found; using built-in data (enter the game folder below)".into();
        }
        res.ok = loaded;
        let g = gamedata::gd();
        res.weapons = g.weapons.len();
        res.ammo = g.ammo.len();
        res.fit = gamedata::fit_engine_to_tables(&g, &e0);
        res
    })
}

/// Re-fit from recorded shots (starting from the current constants).
pub fn spawn_shot_fit(e0: Engine, shots: Vec<CalShot>) -> JoinHandle<Option<(Engine, f64, f64, Vec<&'static str>)>> {
    std::thread::spawn(move || gamedata::fit_engine(&e0, &shots))
}
