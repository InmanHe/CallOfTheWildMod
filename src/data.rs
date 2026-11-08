//! Embedded species / fur / reserve tables (generated from APC, MIT) and helpers.

use indexmap::IndexMap;
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Deserialize, Debug, Clone)]
pub struct Range4 {
    pub weight_low: f64,
    pub weight_high: f64,
    pub score_low: f64,
    pub score_high: f64,
}

#[derive(Deserialize, Debug, Clone)]
pub struct GenderData {
    pub fur_total_probability: f64,
    pub furs: IndexMap<String, f64>,
    pub score_low: f64,
    pub score_high: f64,
    pub weight_low: f64,
    pub weight_high: f64,
}

impl GenderData {
    pub fn range(&self) -> Range4 {
        Range4 {
            weight_low: self.weight_low,
            weight_high: self.weight_high,
            score_low: self.score_low,
            score_high: self.score_high,
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct Species {
    pub en: String,
    pub zh: String,
    pub gender: IndexMap<String, GenderData>,
    pub trophy: IndexMap<String, Range4>,
    #[serde(default)]
    pub truracs: bool,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Reserve {
    pub key: String,
    pub index: u32,
    pub species: Vec<String>,
    pub en: String,
    pub zh: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Name {
    pub en: String,
    pub zh: String,
}

#[derive(Deserialize, Debug)]
pub struct Data {
    pub reserves: Vec<Reserve>,
    pub species: IndexMap<String, Species>,
    pub furs: IndexMap<String, Name>,
}

static DATA: OnceLock<Data> = OnceLock::new();

pub fn data() -> &'static Data {
    DATA.get_or_init(|| serde_json::from_str(include_str!("../data/apc_data.json")).expect("embedded data"))
}

pub fn species(key: &str) -> Option<&'static Species> {
    data().species.get(key)
}

pub fn reserve(key: &str) -> Option<&'static Reserve> {
    data().reserves.iter().find(|r| r.key == key)
}

pub fn fur_name(key: &str, zh: bool) -> String {
    match data().furs.get(key) {
        Some(n) => if zh { n.zh.clone() } else { n.en.clone() },
        None => key.to_string(),
    }
}

pub fn species_name(key: &str, zh: bool) -> String {
    match species(key) {
        Some(s) => if zh { s.zh.clone() } else { s.en.clone() },
        None => key.to_string(),
    }
}

/// "male" | "female" | "both" | None
pub fn diamond_gender(key: &str) -> Option<&'static str> {
    let s = species(key)?;
    let dmin = s.trophy.get("diamond")?.score_low;
    let m = s.gender.get("male").map(|g| g.score_high >= dmin).unwrap_or(false);
    let f = s.gender.get("female").map(|g| g.score_high >= dmin).unwrap_or(false);
    match (m, f) {
        (true, true) => Some("both"),
        (true, false) => Some("male"),
        (false, true) => Some("female"),
        _ => None,
    }
}

pub fn great_one_gender(key: &str) -> Option<&'static str> {
    let s = species(key)?;
    let m = s.gender.contains_key("great_one_male");
    let f = s.gender.contains_key("great_one_female");
    match (m, f) {
        (true, true) => Some("both"),
        (true, false) => Some("male"),
        (false, true) => Some("female"),
        _ => None,
    }
}

pub fn gender_data(key: &str, gender: &str, great_one: bool) -> Option<&'static GenderData> {
    let s = species(key)?;
    if great_one {
        s.gender.get(&format!("great_one_{gender}"))
    } else {
        s.gender.get(gender)
    }
}

pub fn furs(key: &str, gender: &str, great_one: bool) -> Vec<String> {
    let mut v: Vec<String> = gender_data(key, gender, great_one)
        .map(|g| g.furs.iter().filter(|(_, p)| **p > 0.0).map(|(k, _)| k.clone()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

pub fn rare_furs(key: &str, gender: &str) -> Vec<String> {
    gender_data(key, gender, false)
        .map(|g| {
            g.furs.iter()
                .filter(|(_, p)| **p > 0.0 && **p / g.fur_total_probability < 0.01)
                .map(|(k, _)| k.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// Diamond weight/score range; TruRACS species get 20% padding at the low end (as APC).
pub fn safe_diamond(key: &str) -> Option<Range4> {
    let s = species(key)?;
    let d = s.trophy.get("diamond")?;
    let mut r = d.clone();
    if s.truracs {
        r.weight_low = d.weight_low.min(d.weight_high) + 0.2 * (d.weight_high - d.weight_low).abs();
        r.score_low = d.score_low.min(d.score_high) + 0.2 * (d.score_high - d.score_low).abs();
    }
    Some(r)
}

// ---------------------------------------------------------------- fur seeds
// Algorithm cracked by xpltive / 0xSthSth1337, as used by APC.

pub fn seed_to_probability(seed: u32) -> f64 {
    let x = ((0x343FDu64 * seed as u64 + 0x269EC3) >> 16) | 0x3F8000;
    let converted = ((x << 8) & 0xFFFF_FFFF) as u32;
    (f32::from_bits(converted) as f64).abs() - 1.0
}

pub fn fur_for_seed(seed: u32, key: &str, gender: &str, great_one: bool) -> Option<String> {
    let g = gender_data(key, gender, great_one)?;
    let p = seed_to_probability(seed);
    if !p.is_finite() {
        return None;
    }
    let mut cum = 0.0f64;
    for (fur, prob) in &g.furs {
        cum += prob / g.fur_total_probability;
        if cum >= p {
            return Some(fur.clone());
        }
    }
    None
}

pub fn find_fur_seed(key: &str, gender: &str, great_one: bool, fur: Option<&str>, rng: &mut impl rand::Rng) -> Option<u32> {
    for _ in 0..20_000_000u32 {
        let seed: u32 = rng.gen();
        if let Some(f) = fur_for_seed(seed, key, gender, great_one) {
            if fur.map(|x| x == f).unwrap_or(true) {
                return Some(seed);
            }
        }
    }
    None
}

/// (weight, score) in roughly the same percentile, with +-1% fuzz (as APC).
pub fn gen_weight_score(r: &Range4, percentile: Option<f64>, fuzz: bool, rng: &mut impl rand::Rng) -> (f64, f64) {
    let p = percentile.unwrap_or_else(|| rng.gen_range(0.01..1.0));
    let wv = if fuzz { rng.gen_range(-0.01..0.01) * (r.weight_high - r.weight_low) } else { 0.0 };
    let sv = if fuzz { rng.gen_range(-0.01..0.01) * (r.score_high - r.score_low) } else { 0.0 };
    let w = (r.weight_low + p * (r.weight_high - r.weight_low) + wv).clamp(r.weight_low.min(r.weight_high), r.weight_high.max(r.weight_low));
    let s = (r.score_low + p * (r.score_high - r.score_low) + sv).clamp(r.score_low.min(r.score_high), r.score_high.max(r.score_low));
    (w, s)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Trophy {
    None,
    Bronze,
    Silver,
    Gold,
    Diamond,
    GreatOne,
}

pub fn trophy_for(key: &str, score: f64, great_one: bool) -> Trophy {
    if great_one {
        return Trophy::GreatOne;
    }
    let Some(s) = species(key) else { return Trophy::None };
    for (lvl, t) in [("diamond", Trophy::Diamond), ("gold", Trophy::Gold), ("silver", Trophy::Silver), ("bronze", Trophy::Bronze)] {
        if let Some(r) = s.trophy.get(lvl) {
            if score >= r.score_low {
                return t;
            }
        }
    }
    Trophy::None
}
