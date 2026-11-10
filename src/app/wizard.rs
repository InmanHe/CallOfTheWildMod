//! "Quick start" page: one checklist and one button that walks through the
//! guided setup (see `cotwmod::guide`). Prompts are also drawn on the in-game
//! overlay, so the user never has to leave the game except to type a distance.

use super::{hotkeys, tr, App};
use cotwmod::gamedata::gd;
use cotwmod::guide::{Step, Task};
use eframe::egui::{self, Color32, RichText};

const OK: Color32 = Color32::from_rgb(80, 200, 120);

fn zero_levels(app: &App) -> usize {
    let g = gd();
    g.weapons.get(&app.s.weapon).and_then(|w| g.tunings.get(&w.tuning)).map(|t| t.zeros.len()).unwrap_or(1)
}

pub fn start(app: &mut App, tasks: &[Task]) {
    let Some(p) = app.shared.lock().unwrap().process.clone() else { return };
    app.wizard.zero_levels = zero_levels(app);
    app.wizard.fov_hint = app.s.camera.fov_deg;
    app.wizard.start(p, tasks);
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let lang = app.s.lang;
    let t = |zh, en| tr(lang, zh, en);
    let attached = app.live.attached;
    let mark = |ui: &mut egui::Ui, ok: bool, text: String| {
        ui.horizontal(|ui| {
            ui.colored_label(if ok { OK } else { Color32::GRAY }, if ok { "✔" } else { "○" });
            ui.label(text);
        });
    };
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.heading(t("快速开始", "Quick start"));
        ui.label(t("游戏设为“无边框窗口”，进入保护区并手持武器，然后按下面的顺序做一次即可。之后每次只需打开 CotwMod 再开游戏。",
            "Set the game to borderless window, enter a reserve holding a weapon, then do the steps below once. Afterwards just start CotwMod and the game."));
        ui.add_space(6.0);
        mark(ui, attached, t("1. 游戏已连接", "1. Game attached").to_string());
        let data = if lang == super::settings::Lang::Zh { app.data_status.0.clone() } else { app.data_status.1.clone() };
        mark(ui, app.load_job.is_none() && !gd().weapons.is_empty(), format!("{} {data}", t("2. 武器弹道数据：", "2. Ballistic data:")));
        ui.horizontal(|ui| {
            ui.colored_label(if app.s.overlay_on { OK } else { Color32::GRAY }, if app.s.overlay_on { "✔" } else { "○" });
            if ui.checkbox(&mut app.s.overlay_on, t("3. 显示游戏内落点瞄具（Alt+F6 开关）", "3. Show the in-game impact sight (Alt+F6 toggles)")).changed() {
                app.mark_dirty();
            }
        });
        ui.add_space(6.0);
        ui.label(RichText::new(t("4. 自动读取游戏数值（每项只需设置一次）", "4. Automatic game values (set up once)")).strong());
        let nz = zero_levels(app);
        let busy = app.wizard.active();
        let mut todo: Vec<Task> = vec![];
        let mut run_one: Option<Task> = None;
        egui::Grid::new("guide_tasks").num_columns(3).spacing([10.0, 4.0]).show(ui, |ui| {
            for task in Task::ALL {
                let bound = app.s.bindings.contains_key(&task.field());
                let na = task == Task::Zero && nz < 2;
                let (zh, en) = task.name();
                ui.colored_label(if bound { OK } else { Color32::GRAY }, if bound { "✔" } else { "○" });
                let why = match task {
                    Task::Weapon => t("换武器后瞄具自动切换", "sight follows the weapon you hold"),
                    Task::Zero => if na { t("当前武器没有可调归零，换成带归零的武器再设置", "this weapon has no adjustable zero; hold one that has") } else { t("调归零后落点自动更新", "impact follows the zero level") },
                    Task::Pitch => t("上下坡射击自动修正", "uphill / downhill correction"),
                    Task::Fov => t("开镜放大时落点位置仍然准确", "keeps the mark right when scoped in"),
                    Task::Distance => t("测距后距离自动填入（没有就用 Alt+PgUp/PgDn 手动调）", "rangefinder distance fills in (otherwise Alt+PgUp/PgDn)"),
                };
                ui.label(format!("{} — {why}", t(zh, en)));
                if ui.add_enabled(attached && !busy && !na, egui::Button::new(if bound { t("重新设置", "Redo") } else { t("设置", "Set up") }).small()).clicked() {
                    run_one = Some(task);
                }
                ui.end_row();
                if !bound && !na {
                    todo.push(task);
                }
            }
        });
        ui.add_space(4.0);
        if !busy {
            let label = if todo.is_empty() { t("全部已完成 ✔", "All done ✔").to_string() } else { format!("{} ({})", t("一键设置未完成的项目", "Set up everything that is missing"), todo.len()) };
            if ui.add_enabled(attached && !todo.is_empty(), egui::Button::new(RichText::new(label).size(16.0))).clicked() {
                start(app, &todo);
            }
            if !attached {
                ui.label(RichText::new(t("（先启动游戏；连接不上时用管理员身份运行 CotwMod）", "(start the game first; run CotwMod as administrator if it will not attach)")).small());
            }
        }
        if let Some(task) = run_one {
            start(app, &[task]);
        }

        match app.wizard.step.clone() {
            Step::Failed(e) => {
                ui.colored_label(ui.visuals().warn_fg_color, format!("{} ({}) — {e}", t("这一项没有成功，请再试一次", "This step did not succeed, please try again"), t(app.wizard.task.name().0, app.wizard.task.name().1)));
            }
            Step::Done => {
                ui.colored_label(OK, t("设置完成，已保存。游戏更新后如果失效，重新设置一次即可。", "Setup finished and saved. If a game update breaks it, just run it again."));
            }
            _ => {}
        }
        if let Some((zh, en)) = app.wizard.prompt() {
            ui.add_space(6.0);
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.colored_label(Color32::from_rgb(215, 120, 0), RichText::new(format!(">> {}", if lang == super::settings::Lang::Zh { zh } else { en })).size(15.0));
                ui.label(RichText::new(t("同样的提示会显示在游戏画面上方，不需要切回这个窗口。", "The same prompt is shown at the top of the game screen; no need to switch back here.")).small());
                if matches!(app.wizard.step, Step::Scanning | Step::PointerScan) {
                    ui.add(egui::ProgressBar::new(app.wizard.progress.fraction()).show_percentage());
                }
                let process = app.shared.lock().unwrap().process.clone();
                ui.horizontal(|ui| {
                    if app.wizard.needs_value() {
                        ui.label(t("距离 m", "Distance m"));
                        let r = ui.add(egui::TextEdit::singleline(&mut app.guide_value).desired_width(70.0));
                        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if ui.button(t("确定", "OK")).clicked() || enter {
                            if let Ok(v) = app.guide_value.trim().parse::<f64>() {
                                app.wizard.confirm(process.clone(), Some(v));
                                app.guide_value.clear();
                            }
                        }
                    } else if app.wizard.step == Step::WaitUser && ui.button(t("我做好了（等同 Alt+F12）", "Done (same as Alt+F12)")).clicked() {
                        app.wizard.confirm(process.clone(), None);
                    }
                    if ui.button(t("取消", "Cancel")).clicked() {
                        app.wizard.cancel();
                    }
                });
            });
        }

        ui.add_space(8.0);
        ui.separator();
        ui.label(RichText::new(t("5. 风", "5. Wind")).strong());
        ui.label(t("风向和风力请对照游戏 HUD 用热键设置：Alt+←/→ 转风向，Alt+↑/↓ 调风力。弓箭在常用距离内受风影响很小。",
            "Set wind from the game HUD with hotkeys: Alt+←/→ direction, Alt+↑/↓ strength. Arrows are barely affected at normal bow ranges."));
        ui.add_space(4.0);
        ui.label(RichText::new(t("6. 打得不准时：自动校准", "6. If shots land off: auto-calibration")).strong());
        ui.label(t("Alt+F11 进入校准 → 开一枪 → Alt+方向键把红框移到弹着点 → Alt+Insert 记录。记录两枪以上后自动修正所有武器的弹道。",
            "Alt+F11 enters calibration → fire → Alt+arrows move the red box onto the hit → Alt+Insert records. From two shots on, ballistics are corrected for all weapons."));
        ui.add_space(4.0);
        ui.collapsing(t("全部热键", "All hotkeys"), |ui| {
            for (_, key, act) in hotkeys::KEYS {
                ui.label(format!("{key}  —  {}", t(act.describe().0, act.describe().1)));
            }
        });
        ui.label(RichText::new(t("种群数量 / 稀有度在“种群编辑”页；动物可视距离在“内存扫描”页。仅限单人模式。",
            "Population / rarity: Population tab. Animal view distance: Memory tab. Single-player only.")).small());
    });
}
