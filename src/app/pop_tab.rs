use super::settings::{Lang, Settings};
use super::{tr, App};
use cotwmod::data::{self, Trophy};
use cotwmod::population::{self, Animal, PopFile, Stats};
use eframe::egui::{self, Color32, RichText};
use egui_extras::{Column, TableBuilder};
use rand::rngs::StdRng;
use rand::SeedableRng;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Default)]
struct Edit {
    males: usize,
    females: usize,
    diamonds: usize,
    great_ones: usize,
    rares: usize,
    to_males: usize,
    to_females: usize,
    diamond_rare: bool,
}

pub struct PopState {
    detected: Vec<PathBuf>,
    reserve: String,
    file: Option<PopFile>,
    stats: Vec<(String, Stats)>,
    edits: HashMap<String, Edit>,
    selected: Option<String>,
    animals: Vec<Animal>,
    unsaved: bool,
    rng: StdRng,
}

impl PopState {
    pub fn new(_s: &Settings) -> Self {
        PopState {
            detected: population::default_save_dirs(),
            reserve: data::data().reserves[0].key.clone(),
            file: None,
            stats: vec![],
            edits: HashMap::new(),
            selected: None,
            animals: vec![],
            unsaved: false,
            rng: StdRng::from_entropy(),
        }
    }

    fn refresh(&mut self) {
        let Some(f) = &self.file else { return };
        self.stats.clear();
        for sp in &data::reserve(&self.reserve).unwrap().species {
            if data::species(sp).is_none() {
                continue;
            }
            if let Ok(st) = f.stats(sp) {
                if st.groups == 0 {
                    continue;
                }
                let e = self.edits.entry(sp.clone()).or_default();
                e.males = st.males;
                e.females = st.females;
                e.diamonds = 0;
                e.great_ones = 0;
                e.rares = 0;
                e.to_males = 0;
                e.to_females = 0;
                self.stats.push((sp.clone(), st));
            }
        }
        self.animals = match &self.selected {
            Some(sp) => f.animals(sp).unwrap_or_default(),
            None => vec![],
        };
    }
}

fn trophy_label(t: Trophy, lang: Lang) -> RichText {
    let (zh, en, c) = match t {
        Trophy::None => ("-", "-", Color32::GRAY),
        Trophy::Bronze => ("铜", "Bronze", Color32::from_rgb(205, 127, 50)),
        Trophy::Silver => ("银", "Silver", Color32::from_rgb(192, 192, 192)),
        Trophy::Gold => ("金", "Gold", Color32::from_rgb(255, 215, 0)),
        Trophy::Diamond => ("钻石", "Diamond", Color32::from_rgb(120, 220, 255)),
        Trophy::GreatOne => ("巨兽", "Great One", Color32::from_rgb(255, 120, 255)),
    };
    RichText::new(tr(lang, zh, en)).color(c)
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let lang = app.s.lang;
    let t = |zh, en| tr(lang, zh, en);

    // ---------------------------------------------------------- toolbar
    ui.horizontal(|ui| {
        ui.label(t("存档文件夹", "Save folder"));
        ui.add(egui::TextEdit::singleline(&mut app.s.save_dir).desired_width(420.0)
            .hint_text("Documents\\Avalanche Studios\\COTW\\Saves\\<id>"));
        if !app.pop.detected.is_empty() {
            egui::ComboBox::from_id_salt("detected").selected_text(t("自动检测…", "Detected…")).show_ui(ui, |ui| {
                for d in app.pop.detected.clone() {
                    if ui.selectable_label(false, d.display().to_string()).clicked() {
                        app.s.save_dir = d.display().to_string();
                        app.mark_dirty();
                    }
                }
            });
        }
        if ui.button(t("重新检测", "Rescan")).clicked() {
            app.pop.detected = population::default_save_dirs();
            if app.s.save_dir.is_empty() {
                if let Some(d) = app.pop.detected.first() {
                    app.s.save_dir = d.display().to_string();
                }
            }
        }
    });
    if app.s.save_dir.is_empty() {
        if let Some(d) = app.pop.detected.first() {
            app.s.save_dir = d.display().to_string();
        }
    }
    ui.horizontal(|ui| {
        ui.label(t("保护区", "Reserve"));
        let cur = data::reserve(&app.pop.reserve).map(|r| if lang == Lang::Zh { r.zh.clone() } else { r.en.clone() }).unwrap_or_default();
        let mut changed = false;
        egui::ComboBox::from_id_salt("reserve").width(300.0).selected_text(cur).show_ui(ui, |ui| {
            for r in &data::data().reserves {
                let name = if lang == Lang::Zh { &r.zh } else { &r.en };
                if ui.selectable_value(&mut app.pop.reserve, r.key.clone(), format!("{name}  ({})", r.key)).changed() {
                    changed = true;
                }
            }
        });
        let load = ui.button(t("读取存档", "Load")).clicked();
        if changed || load {
            let discarded = app.pop.unsaved && changed;
            app.pop.selected = None;
            match PopFile::load(&PathBuf::from(&app.s.save_dir), &app.pop.reserve) {
                Ok(f) => {
                    app.pop.file = Some(f);
                    app.pop.unsaved = false;
                    app.pop.refresh();
                    app.status = format!("{} {}{}", t("已读取", "Loaded"), population::file_name(&app.pop.reserve).unwrap_or_default(),
                        if discarded { t("（上一个保护区未保存的修改已丢弃）", " (unsaved changes of the previous reserve were discarded)") } else { "" });
                }
                Err(e) => {
                    app.pop.file = None;
                    app.pop.stats.clear();
                    app.status = format!("{}: {e:#}", t("读取失败", "Load failed"));
                }
            }
        }
        ui.separator();
        let can_save = app.pop.file.is_some() && app.pop.unsaved;
        if ui.add_enabled(can_save, egui::Button::new(RichText::new(t("💾 保存到游戏", "💾 Save to game")).strong())).clicked() {
            if app.live.attached {
                app.status = t("请先关闭游戏！游戏退出时会覆盖种群文件。", "Close the game first! It overwrites population files on exit.").into();
            } else if let Some(f) = &app.pop.file {
                match population::save_with_backup(f, &PathBuf::from(&app.s.save_dir)) {
                    Ok(orig) => {
                        app.pop.unsaved = false;
                        app.status = format!("{} {}", t("已保存。原始备份：", "Saved. Original backup:"), orig.display());
                    }
                    Err(e) => app.status = format!("{}: {e:#}", t("保存失败", "Save failed")),
                }
            }
        }
        if ui.button(t("还原原始存档", "Restore original")).clicked() {
            match population::restore_original(&app.pop.reserve, &PathBuf::from(&app.s.save_dir)) {
                Ok(()) => {
                    app.status = t("已还原为第一次修改前的原始文件", "Restored the original file").into();
                    app.pop.file = PopFile::load(&PathBuf::from(&app.s.save_dir), &app.pop.reserve).ok();
                    app.pop.unsaved = false;
                    app.pop.refresh();
                }
                Err(e) => app.status = format!("{e:#}"),
            }
        }
        if app.pop.unsaved {
            ui.colored_label(Color32::YELLOW, t("● 有未保存的修改", "● unsaved changes"));
        }
    });
    if app.live.attached {
        ui.colored_label(Color32::from_rgb(255, 170, 60), t("⚠ 游戏正在运行：修改种群前请先退出游戏（回到桌面）。", "⚠ The game is running: quit to desktop before editing populations."));
    }
    ui.separator();

    if app.pop.file.is_none() {
        ui.label(t("选择存档文件夹和保护区，然后点击“读取存档”。修改只在点击“保存到游戏”后写入，每次保存都会自动备份。",
            "Pick the save folder and a reserve, then Load. Nothing is written until you press Save; every save is backed up."));
        return;
    }

    // ---------------------------------------------------------- species table
    let mut action: Option<(String, Edit)> = None;
    let mut select: Option<String> = None;
    egui::ScrollArea::vertical().id_salt("species").max_height(ui.available_height() * 0.5).show(ui, |ui| {
        egui::Grid::new("species_grid").striped(true).spacing([10.0, 4.0]).show(ui, |ui| {
            for h in [t("物种", "Species"), t("总数", "Total"), t("公", "M"), t("母", "F"), t("钻石", "Diam."), t("巨兽", "GO"), t("稀有毛色", "Rare"),
                t("公 目标", "Males →"), t("母 目标", "Females →"), t("母→公", "F→M"), t("公→母", "M→F"), t("+稀有", "+Rare"), t("+钻石", "+Diamond"), t("+巨兽", "+Great One"), ""] {
                ui.strong(h);
            }
            ui.end_row();
            for (sp, st) in app.pop.stats.clone() {
                let name = data::species_name(&sp, lang == Lang::Zh);
                if ui.selectable_label(app.pop.selected.as_deref() == Some(&sp), name).clicked() {
                    select = Some(sp.clone());
                }
                ui.label(st.total.to_string());
                ui.label(st.males.to_string());
                ui.label(st.females.to_string());
                ui.label(st.diamonds.to_string());
                ui.label(st.great_ones.to_string());
                ui.label(st.rare_furs.to_string());
                let e = app.pop.edits.entry(sp.clone()).or_default();
                let cap = st.groups * population::MAX_GROUP;
                ui.add(egui::DragValue::new(&mut e.males).range(0..=cap));
                ui.add(egui::DragValue::new(&mut e.females).range(0..=cap));
                ui.add(egui::DragValue::new(&mut e.to_males).range(0..=st.females));
                ui.add(egui::DragValue::new(&mut e.to_females).range(0..=st.males));
                ui.add(egui::DragValue::new(&mut e.rares).range(0..=st.total));
                ui.horizontal(|ui| {
                    ui.add_enabled(data::diamond_gender(&sp).is_some(), egui::DragValue::new(&mut e.diamonds).range(0..=st.total));
                    ui.checkbox(&mut e.diamond_rare, t("稀有色", "rare fur"));
                });
                ui.add_enabled(data::great_one_gender(&sp).is_some(), egui::DragValue::new(&mut e.great_ones).range(0..=st.total));
                if ui.button(t("应用", "Apply")).clicked() {
                    action = Some((sp.clone(), e.clone()));
                }
                ui.end_row();
            }
        });
    });
    if let Some(sp) = select {
        app.pop.selected = Some(sp);
        app.pop.refresh();
    }
    if let Some((sp, e)) = action {
        let st = app.pop.stats.iter().find(|x| x.0 == sp).map(|x| x.1.clone()).unwrap_or_default();
        let rng = &mut app.pop.rng;
        let f = app.pop.file.as_mut().unwrap();
        let mut msg = vec![];
        let r: anyhow::Result<()> = (|| {
            if e.males != st.males {
                let n = f.set_count(&sp, true, e.males, rng)?;
                msg.push(format!("{} {n}", tr(lang, "公数量变化", "males changed")));
            }
            if e.females != st.females {
                let n = f.set_count(&sp, false, e.females, rng)?;
                msg.push(format!("{} {n}", tr(lang, "母数量变化", "females changed")));
            }
            if e.to_males > 0 {
                msg.push(format!("{} {}", tr(lang, "母→公", "F→M"), f.convert_gender(&sp, e.to_males, true, rng)?));
            }
            if e.to_females > 0 {
                msg.push(format!("{} {}", tr(lang, "公→母", "M→F"), f.convert_gender(&sp, e.to_females, false, rng)?));
            }
            if e.rares > 0 {
                msg.push(format!("{} {}", tr(lang, "稀有毛色", "rare furs"), f.make_rare_furs(&sp, e.rares, rng)?));
            }
            if e.diamonds > 0 {
                msg.push(format!("{} {}", tr(lang, "钻石", "diamonds"), f.make_diamonds(&sp, e.diamonds, e.diamond_rare, rng)?));
            }
            if e.great_ones > 0 {
                msg.push(format!("{} {}", tr(lang, "巨兽", "Great Ones"), f.make_great_ones(&sp, e.great_ones, rng)?));
            }
            Ok(())
        })();
        app.pop.unsaved = true;
        app.status = match r {
            Ok(()) => format!("{}: {}", data::species_name(&sp, lang == Lang::Zh), if msg.is_empty() { tr(lang, "无变化", "no change").to_string() } else { msg.join(", ") }),
            Err(err) => format!("{}: {err:#}", tr(lang, "出错", "Error")),
        };
        app.pop.refresh();
    }

    // ---------------------------------------------------------- animal list
    ui.separator();
    let Some(sp) = app.pop.selected.clone() else {
        ui.label(t("点击物种名称查看并逐只编辑动物。", "Click a species name to list and edit individual animals."));
        return;
    };
    ui.strong(format!("{} — {} {}", data::species_name(&sp, lang == Lang::Zh), app.pop.animals.len(), t("只", "animals")));
    let has_go = data::great_one_gender(&sp);
    let dg = data::diamond_gender(&sp);
    let mut op: Option<(usize, &'static str, Option<String>)> = None;
    let animals = app.pop.animals.clone();
    TableBuilder::new(ui)
        .id_salt("animals")
        .striped(true)
        .column(Column::exact(50.0))
        .column(Column::exact(40.0))
        .column(Column::exact(50.0))
        .column(Column::exact(70.0))
        .column(Column::exact(70.0))
        .column(Column::exact(70.0))
        .column(Column::exact(170.0))
        .column(Column::remainder())
        .header(20.0, |mut h| {
            for x in ["#", t("群", "Grp"), t("性别", "Sex"), t("体重", "Weight"), t("评分", "Score"), t("等级", "Trophy"), t("毛色", "Fur"), t("操作", "Actions")] {
                h.col(|ui| {
                    ui.strong(x);
                });
            }
        })
        .body(|body| {
            body.rows(22.0, animals.len(), |mut row| {
                let i = row.index();
                let a = &animals[i];
                let gender = if a.male { "male" } else { "female" };
                row.col(|ui| { ui.label(i.to_string()); });
                row.col(|ui| { ui.label(a.group.to_string()); });
                row.col(|ui| { ui.label(if a.male { t("♂ 公", "♂ M") } else { t("♀ 母", "♀ F") }); });
                row.col(|ui| { ui.label(format!("{:.1}", a.weight)); });
                row.col(|ui| { ui.label(format!("{:.1}", a.score)); });
                row.col(|ui| { ui.label(trophy_label(a.trophy, lang)); });
                row.col(|ui| {
                    let cur = a.fur.as_deref().map(|f| data::fur_name(f, lang == Lang::Zh)).unwrap_or_else(|| "?".into());
                    egui::ComboBox::from_id_salt(("fur", i)).width(150.0).selected_text(cur).show_ui(ui, |ui| {
                        for f in data::furs(&sp, gender, a.great_one) {
                            if ui.selectable_label(a.fur.as_deref() == Some(&f), data::fur_name(&f, lang == Lang::Zh)).clicked() {
                                op = Some((i, "fur", Some(f)));
                            }
                        }
                    });
                });
                row.col(|ui| {
                    ui.horizontal(|ui| {
                        let can_d = !a.great_one && dg.map(|g| g == "both" || g == gender).unwrap_or(false);
                        if ui.add_enabled(can_d, egui::Button::new(t("→钻石", "→Diamond")).small()).clicked() {
                            op = Some((i, "diamond", None));
                        }
                        let can_go = !a.great_one && has_go.map(|g| g == "both" || g == gender).unwrap_or(false);
                        if ui.add_enabled(can_go, egui::Button::new(t("→巨兽", "→Great One")).small()).clicked() {
                            op = Some((i, "go", None));
                        }
                        if ui.add_enabled(!a.great_one, egui::Button::new(t("换性别", "Swap sex")).small()).clicked() {
                            op = Some((i, "sex", None));
                        }
                    });
                });
            });
        });
    if let Some((i, what, fur)) = op {
        let a = animals[i].clone();
        let rng = &mut app.pop.rng;
        let f = app.pop.file.as_mut().unwrap();
        let gender = if a.male { "male" } else { "female" };
        let r: anyhow::Result<()> = match what {
            "fur" => f.set_fur(&sp, &a, fur.as_deref().unwrap(), rng),
            "diamond" => {
                let rr = data::safe_diamond(&sp).unwrap();
                let (w, s) = data::gen_weight_score(&rr, None, true, rng);
                f.set_animal(&a, a.male, w, s, false, a.seed)
            }
            "go" => {
                let gd = data::gender_data(&sp, gender, true).unwrap();
                let (w, s) = data::gen_weight_score(&gd.range(), None, true, rng);
                let seed = data::find_fur_seed(&sp, gender, true, None, rng).unwrap_or(a.seed);
                f.set_animal(&a, a.male, w, s, true, seed)
            }
            _ => {
                // swap sex: re-roll stats in the same percentile for the other sex
                let to = if a.male { "female" } else { "male" };
                let og = data::gender_data(&sp, gender, false).unwrap();
                let ng = data::gender_data(&sp, to, false).unwrap();
                let pct = ((a.weight - og.weight_low) / (og.weight_high - og.weight_low).max(1e-9)).clamp(0.0, 1.0);
                let (w, s) = data::gen_weight_score(&ng.range(), Some(pct), false, rng);
                let seed = data::find_fur_seed(&sp, to, false, a.fur.as_deref().filter(|x| data::furs(&sp, to, false).iter().any(|y| y == x)), rng)
                    .or_else(|| data::find_fur_seed(&sp, to, false, None, rng)).unwrap_or(a.seed);
                f.set_animal(&a, !a.male, w, s, false, seed)
            }
        };
        if let Err(e) = r {
            app.status = format!("{e:#}");
        }
        app.pop.unsaved = true;
        app.pop.refresh();
    }
}
