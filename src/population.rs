//! Reserve population files: load / inspect / edit / save, with backups.
//!
//! File layout (as handled by APC): 32-byte file header, then a zlib stream
//! whose output is a 5-byte header followed by the ADF data.

use crate::adf::{self, Adf, Node};
use crate::data::{self, Trophy};
use anyhow::{anyhow, bail, Context, Result};
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use rand::seq::SliceRandom;
use rand::Rng;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const MAX_GROUP: usize = 30;

pub struct PopFile {
    pub file_header: Vec<u8>,
    pub inner_header: Vec<u8>,
    pub data: Vec<u8>,
    pub adf: Adf,
    pub reserve: String,
}

#[derive(Debug, Clone)]
pub struct Animal {
    pub pop: usize,
    pub group: usize,
    pub idx: usize,
    pub male: bool,
    pub weight: f64,
    pub score: f64,
    pub great_one: bool,
    pub seed: u32,
    pub fur: Option<String>,
    pub trophy: Trophy,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Stats {
    pub total: usize,
    pub males: usize,
    pub females: usize,
    pub diamonds: usize,
    pub great_ones: usize,
    pub rare_furs: usize,
    pub groups: usize,
}

pub fn file_name(reserve: &str) -> Result<String> {
    let r = data::reserve(reserve).ok_or_else(|| anyhow!("unknown reserve {reserve}"))?;
    Ok(format!("animal_population_{}", r.index))
}

impl PopFile {
    pub fn from_bytes(raw: &[u8], reserve: &str) -> Result<PopFile> {
        if raw.len() < 32 {
            bail!("population file too short");
        }
        let mut dec = Vec::new();
        ZlibDecoder::new(&raw[32..]).read_to_end(&mut dec).context("zlib decompress")?;
        if dec.len() < 5 {
            bail!("decompressed data too short");
        }
        let data = dec[5..].to_vec();
        let adf = Adf::parse(&data)?;
        Ok(PopFile {
            file_header: raw[..32].to_vec(),
            inner_header: dec[..5].to_vec(),
            data,
            adf,
            reserve: reserve.to_string(),
        })
    }

    pub fn load(save_dir: &Path, reserve: &str) -> Result<PopFile> {
        let p = save_dir.join(file_name(reserve)?);
        let raw = std::fs::read(&p).with_context(|| format!("read {}", p.display()))?;
        PopFile::from_bytes(&raw, reserve)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut dec = self.inner_header.clone();
        dec.extend_from_slice(&self.data);
        let mut fh = self.file_header.clone();
        let n = (dec.len() as u32).to_le_bytes();
        fh[8..12].copy_from_slice(&n);
        fh[24..28].copy_from_slice(&n);
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&dec)?;
        fh.extend_from_slice(&enc.finish()?);
        Ok(fh)
    }

    fn reparse(&mut self) -> Result<()> {
        self.adf = Adf::parse(&self.data)?;
        Ok(())
    }

    fn species_index(&self, species: &str) -> Result<usize> {
        let r = data::reserve(&self.reserve).ok_or_else(|| anyhow!("unknown reserve"))?;
        r.species.iter().position(|s| s == species).ok_or_else(|| anyhow!("{species} not in {}", self.reserve))
    }

    fn groups(&self, pop: usize) -> Result<&[Node]> {
        let root = self.adf.roots.first().ok_or_else(|| anyhow!("no instance"))?;
        let pops = root.get("Populations")?.items();
        let p = pops.get(pop).ok_or_else(|| anyhow!("population {pop} missing"))?;
        Ok(p.get("Groups")?.items())
    }

    fn animal_node(&self, pop: usize, group: usize, idx: usize) -> Result<&Node> {
        self.groups(pop)?[group].get("Animals")?.items().get(idx).ok_or_else(|| anyhow!("animal missing"))
    }

    fn great_one_node(n: &Node) -> Option<&Node> {
        n.field("IsGreatOne").or_else(|| n.field("FeatureModifiers").and_then(|f| f.field("Flags")))
    }

    pub fn animals(&self, species: &str) -> Result<Vec<Animal>> {
        let pop = self.species_index(species)?;
        let mut out = vec![];
        for (gi, g) in self.groups(pop)?.iter().enumerate() {
            for (ai, a) in g.get("Animals")?.items().iter().enumerate() {
                let male = a.get("Gender")?.uint() == 1;
                let great_one = Self::great_one_node(a).map(|n| n.uint() == 1).unwrap_or(false);
                let score = a.get("Score")?.num();
                let seed = a.get("VisualVariationSeed")?.uint() as u32;
                let gender = if male { "male" } else { "female" };
                out.push(Animal {
                    pop,
                    group: gi,
                    idx: ai,
                    male,
                    weight: a.get("Weight")?.num(),
                    score,
                    great_one,
                    seed,
                    fur: data::fur_for_seed(seed, species, gender, great_one),
                    trophy: data::trophy_for(species, score, great_one),
                });
            }
        }
        Ok(out)
    }

    pub fn stats(&self, species: &str) -> Result<Stats> {
        let pop = self.species_index(species)?;
        let mut st = Stats { groups: self.groups(pop)?.len(), ..Default::default() };
        let rm = data::rare_furs(species, "male");
        let rf = data::rare_furs(species, "female");
        for a in self.animals(species)? {
            st.total += 1;
            if a.male { st.males += 1 } else { st.females += 1 }
            st.great_ones += a.great_one as usize;
            st.diamonds += (a.trophy == Trophy::Diamond) as usize;
            if let Some(f) = &a.fur {
                if !a.great_one && (if a.male { &rm } else { &rf }).contains(f) {
                    st.rare_furs += 1;
                }
            }
        }
        Ok(st)
    }

    // ------------------------------------------------------------ edits
    /// Write gender / weight / score / great-one / seed of one animal.
    pub fn set_animal(&mut self, a: &Animal, male: bool, weight: f64, score: f64, great_one: bool, seed: u32) -> Result<()> {
        let n = self.animal_node(a.pop, a.group, a.idx)?.clone();
        adf::write_prim(&mut self.data, n.get("Gender")?, if male { 1.0 } else { 2.0 })?;
        adf::write_prim(&mut self.data, n.get("Weight")?, weight)?;
        adf::write_prim(&mut self.data, n.get("Score")?, score)?;
        if let Some(g) = Self::great_one_node(&n) {
            adf::write_prim(&mut self.data, g, if great_one { 1.0 } else { 0.0 })?;
        }
        adf::write_u32_exact(&mut self.data, n.get("VisualVariationSeed")?, seed)?;
        self.reparse()
    }

    fn randomize_new(&mut self, species: &str, pop: usize, group: usize, male: bool, rng: &mut impl Rng) -> Result<()> {
        let gender = if male { "male" } else { "female" };
        let gd = data::gender_data(species, gender, false).ok_or_else(|| anyhow!("no {gender} data for {species}"))?;
        let (w, s) = data::gen_weight_score(&gd.range(), None, true, rng);
        let seed = data::find_fur_seed(species, gender, false, None, rng).unwrap_or(0);
        let n = self.animal_node(pop, group, 0)?.clone();
        adf::write_prim(&mut self.data, n.get("Gender")?, if male { 1.0 } else { 2.0 })?;
        adf::write_prim(&mut self.data, n.get("Weight")?, w)?;
        adf::write_prim(&mut self.data, n.get("Score")?, s)?;
        if let Some(g) = Self::great_one_node(&n) {
            adf::write_prim(&mut self.data, g, 0.0)?;
        }
        if let Some(sc) = n.field("IsScripted") {
            adf::write_prim(&mut self.data, sc, 0.0)?;
        }
        adf::write_u32_exact(&mut self.data, n.get("VisualVariationSeed")?, seed)?;
        if let Some(id) = n.field("Id") {
            adf::write_prim(&mut self.data, id, 0.0)?;
        }
        self.reparse()
    }

    /// Add `count` animals (cloning the first animal of each group, round robin).
    pub fn add_animals(&mut self, species: &str, count: usize, male: bool, rng: &mut impl Rng) -> Result<usize> {
        let pop = self.species_index(species)?;
        let mut added = 0;
        let mut gi = 0usize;
        let mut full_rounds = 0;
        while added < count {
            let groups = self.groups(pop)?;
            if groups.is_empty() {
                break;
            }
            let g = gi % groups.len();
            gi += 1;
            let arr = groups[g].get("Animals")?.clone();
            if arr.items().is_empty() || arr.items().len() >= MAX_GROUP {
                full_rounds += 1;
                if full_rounds > groups.len() {
                    break; // every group is full or empty
                }
                continue;
            }
            full_rounds = 0;
            adf::insert_clone_front(&self.adf, &mut self.data, &arr, 0)?;
            self.reparse()?;
            self.randomize_new(species, pop, g, male, rng)?;
            added += 1;
        }
        Ok(added)
    }

    /// Remove `count` animals of a gender (never the last animal of a group,
    /// never Great Ones).
    pub fn remove_animals(&mut self, species: &str, count: usize, male: bool) -> Result<usize> {
        let pop = self.species_index(species)?;
        let mut removed = 0;
        let mut gi = 0usize;
        let mut misses = 0;
        while removed < count {
            let groups = self.groups(pop)?;
            if groups.is_empty() {
                break;
            }
            let g = gi % groups.len();
            gi += 1;
            let arr = groups[g].get("Animals")?.clone();
            let target = if arr.items().len() >= 2 {
                arr.items().iter().position(|a| {
                    a.field("Gender").map(|x| (x.uint() == 1) == male).unwrap_or(false)
                        && !Self::great_one_node(a).map(|n| n.uint() == 1).unwrap_or(false)
                })
            } else {
                None
            };
            match target {
                Some(i) => {
                    adf::remove_element(&self.adf, &mut self.data, &arr, i)?;
                    self.reparse()?;
                    removed += 1;
                    misses = 0;
                }
                None => {
                    misses += 1;
                    if misses > groups.len() {
                        break;
                    }
                }
            }
        }
        Ok(removed)
    }

    /// Set the number of males or females to `target`.
    pub fn set_count(&mut self, species: &str, male: bool, target: usize, rng: &mut impl Rng) -> Result<usize> {
        let st = self.stats(species)?;
        let cur = if male { st.males } else { st.females };
        if target > cur {
            self.add_animals(species, target - cur, male, rng)
        } else if target < cur {
            self.remove_animals(species, cur - target, male)
        } else {
            Ok(0)
        }
    }

    fn pick(&self, species: &str, filter: impl Fn(&Animal) -> bool, n: usize, rng: &mut impl Rng) -> Result<Vec<Animal>> {
        let mut v: Vec<Animal> = self.animals(species)?.into_iter().filter(|a| filter(a)).collect();
        v.shuffle(rng);
        v.truncate(n);
        Ok(v)
    }

    /// Turn up to `n` eligible animals into diamonds. Returns how many were changed.
    pub fn make_diamonds(&mut self, species: &str, n: usize, rare_fur: bool, rng: &mut impl Rng) -> Result<usize> {
        let dg = data::diamond_gender(species).ok_or_else(|| anyhow!("{species} has no diamond"))?;
        let r = data::safe_diamond(species).ok_or_else(|| anyhow!("no diamond data"))?;
        let chosen = self.pick(species, |a| !a.great_one && a.trophy != Trophy::Diamond
            && (dg == "both" || (dg == "male") == a.male), n, rng)?;
        for a in &chosen {
            let (w, s) = data::gen_weight_score(&r, None, true, rng);
            let gender = if a.male { "male" } else { "female" };
            let seed = if rare_fur {
                let rares = data::rare_furs(species, gender);
                match rares.choose(rng) {
                    Some(f) => data::find_fur_seed(species, gender, false, Some(f), rng).unwrap_or(a.seed),
                    None => a.seed,
                }
            } else {
                a.seed
            };
            self.set_animal(a, a.male, w, s, false, seed)?;
        }
        Ok(chosen.len())
    }

    pub fn make_great_ones(&mut self, species: &str, n: usize, rng: &mut impl Rng) -> Result<usize> {
        let gg = data::great_one_gender(species).ok_or_else(|| anyhow!("{species} has no Great One"))?;
        let chosen = self.pick(species, |a| !a.great_one && (gg == "both" || (gg == "male") == a.male), n, rng)?;
        for a in &chosen {
            let gender = if a.male { "male" } else { "female" };
            let gd = data::gender_data(species, gender, true).ok_or_else(|| anyhow!("no GO data"))?;
            let (w, s) = data::gen_weight_score(&gd.range(), None, true, rng);
            let seed = data::find_fur_seed(species, gender, true, None, rng).unwrap_or(a.seed);
            self.set_animal(a, a.male, w, s, true, seed)?;
        }
        Ok(chosen.len())
    }

    pub fn make_rare_furs(&mut self, species: &str, n: usize, rng: &mut impl Rng) -> Result<usize> {
        let chosen = self.pick(species, |a| !a.great_one, usize::MAX, rng)?;
        let mut done = 0;
        for a in chosen {
            if done >= n {
                break;
            }
            let gender = if a.male { "male" } else { "female" };
            let rares = data::rare_furs(species, gender);
            if rares.is_empty() || a.fur.as_ref().map(|f| rares.contains(f)).unwrap_or(false) {
                continue;
            }
            let f = rares.choose(rng).unwrap();
            if let Some(seed) = data::find_fur_seed(species, gender, false, Some(f), rng) {
                self.set_animal(&a, a.male, a.weight, a.score, false, seed)?;
                done += 1;
            }
        }
        Ok(done)
    }

    /// Convert `n` animals of the other gender (not diamonds / GOs) to `to_male`.
    pub fn convert_gender(&mut self, species: &str, n: usize, to_male: bool, rng: &mut impl Rng) -> Result<usize> {
        let chosen = self.pick(species, |a| a.male != to_male && !a.great_one && a.trophy != Trophy::Diamond, n, rng)?;
        let (from, to) = if to_male { ("female", "male") } else { ("male", "female") };
        let og = data::gender_data(species, from, false).ok_or_else(|| anyhow!("no data"))?;
        let ng = data::gender_data(species, to, false).ok_or_else(|| anyhow!("no data"))?;
        for a in &chosen {
            let span = og.weight_high - og.weight_low;
            let pct = if span > 0.0 { ((a.weight - og.weight_low) / span).clamp(0.0, 1.0) } else { 0.5 };
            let (w, s) = data::gen_weight_score(&ng.range(), Some(pct), false, rng);
            let seed = a.fur.as_ref()
                .filter(|f| data::furs(species, to, false).contains(f))
                .and_then(|f| data::find_fur_seed(species, to, false, Some(f), rng))
                .or_else(|| data::find_fur_seed(species, to, false, None, rng))
                .unwrap_or(a.seed);
            self.set_animal(a, to_male, w, s, false, seed)?;
        }
        Ok(chosen.len())
    }

    /// Set one animal's fur (keeps everything else).
    pub fn set_fur(&mut self, species: &str, a: &Animal, fur: &str, rng: &mut impl Rng) -> Result<()> {
        let gender = if a.male { "male" } else { "female" };
        let seed = data::find_fur_seed(species, gender, a.great_one, Some(fur), rng)
            .ok_or_else(|| anyhow!("no seed found for {fur}"))?;
        self.set_animal(a, a.male, a.weight, a.score, a.great_one, seed)
    }
}

// ---------------------------------------------------------------- save files

pub fn default_save_dirs() -> Vec<PathBuf> {
    let mut roots = vec![];
    if let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
        roots.push(home.join("Documents"));
        roots.push(home.join("OneDrive").join("Documents"));
        roots.push(home.join("OneDrive").join("文档"));
        roots.push(home.join("OneDrive").join("文件"));
    }
    let mut out = vec![];
    for r in roots {
        for store in ["", "Epic Games Store", "Microsoft Store"] {
            let base = if store.is_empty() {
                r.join("Avalanche Studios").join("COTW").join("Saves")
            } else {
                r.join("Avalanche Studios").join(store).join("COTW").join("Saves")
            };
            if let Ok(rd) = std::fs::read_dir(&base) {
                for e in rd.flatten() {
                    let p = e.path();
                    if p.is_dir() && e.file_name().to_string_lossy().chars().all(|c| c.is_ascii_digit()) {
                        out.push(p);
                    }
                }
            }
        }
    }
    out
}

pub fn backup_root() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("CotwMod").join("backups")
}

fn timestamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("{secs}")
}

/// Write the population back, keeping the very first original forever and a
/// timestamped copy of the previous file.
pub fn save_with_backup(pf: &PopFile, save_dir: &Path) -> Result<PathBuf> {
    let name = file_name(&pf.reserve)?;
    let target = save_dir.join(&name);
    let root = backup_root();
    let orig = root.join("original").join(&name);
    if target.exists() {
        if !orig.exists() {
            std::fs::create_dir_all(orig.parent().unwrap())?;
            std::fs::copy(&target, &orig)?;
        }
        let stamp = root.join(timestamp()).join(&name);
        std::fs::create_dir_all(stamp.parent().unwrap())?;
        std::fs::copy(&target, &stamp)?;
    }
    let bytes = pf.to_bytes()?;
    // validate before writing: must re-parse
    PopFile::from_bytes(&bytes, &pf.reserve).context("self-check of modified file failed")?;
    let tmp = target.with_extension("cotwmod_tmp");
    std::fs::write(&tmp, &bytes)?;
    std::fs::rename(&tmp, &target)?;
    Ok(orig)
}

pub fn restore_original(reserve: &str, save_dir: &Path) -> Result<()> {
    let name = file_name(reserve)?;
    let orig = backup_root().join("original").join(&name);
    if !orig.exists() {
        bail!("no original backup for {reserve}");
    }
    std::fs::copy(&orig, save_dir.join(&name))?;
    Ok(())
}
