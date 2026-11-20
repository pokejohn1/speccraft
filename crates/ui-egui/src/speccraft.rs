//! The Specs ribbon tab and the Sections pane (Speccraft milestone M4). Added by the Speccraft
//! fork; not part of the original WordCraft.
//!
//! The Sections pane shows the open project's sections by division. Clicking one saves the
//! section on screen (if it changed) and shows the other in the same window, so the author works
//! in one program. Everything goes through commands: the engine's `spec.project.*` for the
//! project file, and the UI commands here (`ui.spec.*`) for the pane, so the control channel, MCP
//! and tests can drive it like the mouse does.

use std::path::{Path, PathBuf};

use egui::{RichText, Ui};
use serde_json::{Value, json};

use crate::WordApp;
use crate::file_dialogs::{AfterPick, FileDialogRequest};
use crate::theme::{Tokens, regular, semibold};
use crate::widgets::{big, big_toggle, group};

/// What the Specs tab and Sections pane keep between frames.
#[derive(Default)]
pub struct SpecUi {
    /// The Sections pane is open.
    pub pane_open: bool,
    /// The open project's `.speccraft` file.
    pub project: Option<PathBuf>,
    /// The project as `spec.project.info` last returned it.
    pub info: Option<Value>,
    /// The last `spec.project.validate` result.
    pub findings: Option<Value>,
    /// A file dialog asked for by the pane, waiting for its answer.
    waiting: Option<Pick>,
}

/// What a file dialog the pane asked for is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pick {
    OpenProject,
    NewProject,
    AddMaster,
}

// ---------------------------------------------------------------------------------------------
// Commands

/// The `ui.spec.*` commands (see [`crate::app`]'s `ui_command`).
pub(crate) fn command(app: &mut WordApp, id: &str, p: &Value) -> Result<Value, String> {
    let s = |k: &str| p.get(k).and_then(Value::as_str).map(str::to_string);
    match id {
        "ui.spec.sectionsPane" => {
            app.speccraft.pane_open = p.get("value").and_then(Value::as_bool).unwrap_or(!app.speccraft.pane_open);
            Ok(json!({"open": app.speccraft.pane_open}))
        }
        "ui.spec.openProject" => match s("path") {
            Some(path) => open_project(app, PathBuf::from(path)),
            None => ask(app, Pick::OpenProject),
        },
        "ui.spec.newProject" => match s("path") {
            Some(path) => new_project(app, PathBuf::from(path), s("name")),
            None => ask(app, Pick::NewProject),
        },
        "ui.spec.closeProject" => {
            app.speccraft.project = None;
            app.speccraft.info = None;
            app.speccraft.findings = None;
            Ok(json!({"closed": true}))
        }
        "ui.spec.addSection" => match s("master") {
            Some(master) => add_section(app, &master),
            None => ask(app, Pick::AddMaster),
        },
        "ui.spec.openSection" => {
            let file = s("file").ok_or("`file` must name a section file of the project")?;
            open_section(app, &file)
        }
        "ui.spec.check" => {
            let path = project_path(app)?;
            let r = app.run("spec.project.validate", json!({"path": path}))?;
            app.speccraft.findings = Some(r.clone());
            app.speccraft.pane_open = true;
            Ok(r)
        }
        "ui.spec.publish" => {
            let path = project_path(app)?;
            save_if_changed(app)?;
            let r = app.run("spec.project.publish", json!({"path": path}))?;
            let n = r.get("sections").and_then(Value::as_array).map_or(0, Vec::len);
            let failed = r.get("failed").and_then(Value::as_u64).unwrap_or(0);
            let msg = if failed == 0 {
                crate::i18n::fmt(tl!("Published {n} sections"), &[("n", &n.to_string())])
            } else {
                crate::i18n::fmt(tl!("Published with {failed} failures"), &[("failed", &failed.to_string())])
            };
            app.status(msg);
            Ok(r)
        }
        "ui.spec.include" | "ui.spec.removeSection" | "ui.spec.move" => {
            let path = project_path(app)?;
            let file = s("file").ok_or("`file` must name a section file of the project")?;
            let r = match id {
                "ui.spec.include" => {
                    let on = p.get("included").and_then(Value::as_bool).ok_or("`included` must be true or false")?;
                    app.run("spec.project.include", json!({"path": path, "file": file, "included": on}))?
                }
                "ui.spec.removeSection" => app.run("spec.project.removeSection", json!({"path": path, "file": file}))?,
                _ => {
                    let by = p.get("by").and_then(Value::as_i64).unwrap_or(1);
                    let files = moved(app.speccraft.info.as_ref(), &file, by).ok_or("that section can't move further")?;
                    app.run("spec.project.order", json!({"path": path, "files": files}))?
                }
            };
            app.speccraft.info = Some(r.clone());
            Ok(r)
        }
        _ => Err(format!("unknown command `{id}`")),
    }
}

/// The open project's file, as a string for the engine commands.
fn project_path(app: &WordApp) -> Result<String, String> {
    app.speccraft.project.as_ref().map(|p| p.to_string_lossy().into_owned()).ok_or_else(|| "open a project first".to_string())
}

/// Ask the host for a file; the answer is picked up by [`poll`].
fn ask(app: &mut WordApp, pick: Pick) -> Result<Value, String> {
    let req = match pick {
        Pick::OpenProject => FileDialogRequest::Open { purpose: "speccraft".into() },
        Pick::AddMaster => FileDialogRequest::Open { purpose: "document".into() },
        Pick::NewProject => FileDialogRequest::Save { name: "Project.speccraft".into(), dir: None },
    };
    app.speccraft.pane_open = true;
    app.dialog_pick = None;
    app.speccraft.waiting = Some(pick);
    let _ = app.ask_file(req, AfterPick::ForDialog);
    poll(app);
    Ok(json!({"asked": true}))
}

/// Finish a file dialog the pane asked for, once its answer is in.
fn poll(app: &mut WordApp) {
    let Some(pick) = app.speccraft.waiting else { return };
    if app.file_dialog_open() || app.file_browser_open() {
        return;
    }
    app.speccraft.waiting = None;
    let Some(path) = app.dialog_pick.take() else { return };
    let r = match pick {
        Pick::OpenProject => open_project(app, PathBuf::from(path)),
        Pick::NewProject => new_project(app, PathBuf::from(path), None),
        Pick::AddMaster => add_section(app, &path),
    };
    if let Err(e) = r {
        app.status(e);
    }
}

fn open_project(app: &mut WordApp, path: PathBuf) -> Result<Value, String> {
    let info = app.run("spec.project.info", json!({"path": path.to_string_lossy()}))?;
    app.speccraft.project = Some(path);
    app.speccraft.info = Some(info.clone());
    app.speccraft.findings = None;
    app.speccraft.pane_open = true;
    Ok(info)
}

fn new_project(app: &mut WordApp, mut path: PathBuf, name: Option<String>) -> Result<Value, String> {
    if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("speccraft")) {
        path.set_extension("speccraft");
    }
    let name = name.unwrap_or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
    app.run("spec.project.new", json!({"path": path.to_string_lossy(), "name": name}))?;
    open_project(app, path)
}

fn add_section(app: &mut WordApp, master: &str) -> Result<Value, String> {
    let path = project_path(app)?;
    let r = app.run("spec.project.addSection", json!({"path": path, "master": master}))?;
    app.speccraft.info = Some(r.clone());
    if let Some(file) = r.get("added").and_then(Value::as_str).map(str::to_string) {
        open_section(app, &file)?;
    }
    Ok(r)
}

/// Save the document on screen if it has a file and unsaved changes.
fn save_if_changed(app: &mut WordApp) -> Result<(), String> {
    if app.session.dirty && app.session.path.is_some() {
        app.run("file.save", json!({}))?;
    }
    Ok(())
}

/// Show section `file` of the project in this window: the section on screen is saved first if
/// it changed. An untitled document with changes is left alone (nothing is lost).
fn open_section(app: &mut WordApp, file: &str) -> Result<Value, String> {
    let project = app.speccraft.project.clone().ok_or("open a project first")?;
    let folder = project.parent().map(Path::to_path_buf).unwrap_or_default();
    let known = app
        .speccraft
        .info
        .as_ref()
        .and_then(|i| i.get("sections"))
        .and_then(Value::as_array)
        .is_some_and(|s| s.iter().any(|x| x.get("file").and_then(Value::as_str).is_some_and(|f| f.eq_ignore_ascii_case(file))));
    if !known {
        return Err(format!("{file} is not a section of this project"));
    }
    let path = folder.join(file);
    if app.session.path.as_deref() == Some(path.as_path()) {
        return Ok(json!({"path": path.to_string_lossy(), "alreadyOpen": true}));
    }
    save_if_changed(app)?;
    if app.session.dirty {
        return Err(tl!("Save or close the open document first").to_string());
    }
    let doc = wordcraft_engine::io::open_path(&path)?;
    app.replace_startup_document(doc);
    app.session.path = Some(path.clone());
    app.status(crate::i18n::fmt(tl!("Opened {file}"), &[("file", file)]));
    Ok(json!({"path": path.to_string_lossy(), "alreadyOpen": false}))
}

/// The project's files in order with `file` moved `by` places, or `None` if it can't move.
fn moved(info: Option<&Value>, file: &str, by: i64) -> Option<Vec<String>> {
    let mut files: Vec<String> =
        info?.get("sections")?.as_array()?.iter().filter_map(|s| s.get("file").and_then(Value::as_str).map(str::to_string)).collect();
    let i = files.iter().position(|f| f.eq_ignore_ascii_case(file))?;
    let j = i64::try_from(i).ok()?.checked_add(by)?;
    let j = usize::try_from(j).ok().filter(|j| *j < files.len())?;
    let f = files.remove(i);
    files.insert(j, f);
    Some(files)
}

// ---------------------------------------------------------------------------------------------
// The Specs tab

/// The Specs ribbon tab.
pub(crate) fn ribbon(app: &mut WordApp, ui: &mut Ui) {
    poll(app);
    let has_project = app.speccraft.project.is_some();
    group(ui, "Project", None, app, |ui, app| {
        big(ui, app, "open", "Open\nProject…", "ui.spec.openProject", json!({}), false);
        big(ui, app, "plus", "New\nProject…", "ui.spec.newProject", json!({}), false);
        let open = app.speccraft.pane_open;
        big_toggle(ui, app, "navPane", "Sections\nPane", "ui.spec.sectionsPane", open);
        ui.add_enabled_ui(has_project, |ui| {
            big(ui, app, "file", "Add\nSection…", "ui.spec.addSection", json!({}), false);
            big(ui, app, "checkErrors", "Check\nProject", "ui.spec.check", json!({}), false);
            big(ui, app, "export", "Publish\nProject", "ui.spec.publish", json!({}), false);
        });
    });
    group(ui, "Section", None, app, |ui, app| {
        big(ui, app, "multilevel", "Import\nParagraphs", "spec.import", json!({}), false);
        big(ui, app, "export", "Export\nPDF", "spec.exportPdf", json!({}), false);
    });
}

// ---------------------------------------------------------------------------------------------
// The Sections pane

/// A section row of the pane, from `spec.project.info`.
struct Row {
    file: String,
    number: String,
    title: String,
    included: bool,
    exists: bool,
}

fn rows(info: &Value) -> Vec<Row> {
    let Some(sections) = info.get("sections").and_then(Value::as_array) else { return Vec::new() };
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    sections
        .iter()
        .map(|v| Row {
            file: s(v, "file"),
            number: s(v, "number"),
            title: s(v, "title"),
            included: v.get("included").and_then(Value::as_bool).unwrap_or(true),
            exists: v.get("exists").and_then(Value::as_bool).unwrap_or(false),
        })
        .collect()
}

/// The division of a section number: its first two digits ("09 2900" → "09").
fn division(number: &str) -> String {
    number.chars().filter(char::is_ascii_digit).take(2).collect()
}

/// Title case for an all-capitals section title ("GYPSUM BOARD" → "Gypsum Board").
fn title_case(t: &str) -> String {
    if t.chars().any(char::is_lowercase) {
        return t.to_string();
    }
    t.split(' ')
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_string() + &c.as_str().to_lowercase()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The Sections pane.
pub(crate) fn pane(app: &mut WordApp, ui: &mut Ui) {
    poll(app);
    let t = Tokens::get(ui.ctx());
    let Some(info) = app.speccraft.info.clone() else {
        ui.label(RichText::new(tl!("No project is open.")).font(regular(13.0)));
        ui.add_space(6.0);
        ui.label(RichText::new(tl!("A project lists the section files in one folder.")).small().color(t.text_dim));
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button(tl!("Open Project…")).clicked() {
                let _ = app.run("ui.spec.openProject", json!({}));
            }
            if ui.button(tl!("New Project…")).clicked() {
                let _ = app.run("ui.spec.newProject", json!({}));
            }
        });
        return;
    };
    let field = |k: &str| info.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    ui.label(RichText::new(field("name")).font(semibold(14.0)));
    let sub: Vec<String> = ["number", "issue", "date"].iter().map(|k| field(k)).filter(|s| !s.is_empty()).collect();
    if !sub.is_empty() {
        ui.label(RichText::new(sub.join(" · ")).small().color(t.text_dim));
    }
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        if ui.small_button(tl!("Add Section…")).clicked() {
            let _ = app.run("ui.spec.addSection", json!({}));
        }
        if ui.small_button(tl!("Check")).clicked() {
            let _ = app.run("ui.spec.check", json!({}));
        }
        if ui.small_button(tl!("Publish")).clicked() {
            let _ = app.run("ui.spec.publish", json!({}));
        }
    });
    ui.separator();
    let current = app.session.path.clone();
    let folder = app.speccraft.project.as_ref().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_default();
    let rows = rows(&info);
    let findings = app.speccraft.findings.clone();
    let mut action: Option<(&'static str, Value)> = None;
    egui::ScrollArea::vertical().id_salt("spec_sections").auto_shrink([false, true]).show(ui, |ui| {
        if rows.is_empty() {
            ui.label(RichText::new(tl!("No sections yet. Add one from a master.")).small().color(t.text_dim));
        }
        let mut last_division = None;
        for r in &rows {
            let d = division(&r.number);
            if last_division.as_ref() != Some(&d) {
                ui.add_space(4.0);
                let head = if d.is_empty() { tl!("Other sections").to_string() } else { crate::i18n::fmt(tl!("Division {n}"), &[("n", &d)]) };
                ui.label(RichText::new(head).font(semibold(11.5)).color(t.text_dim));
                last_division = Some(d);
            }
            let shown = current.as_deref() == Some(folder.join(&r.file).as_path());
            let name = if r.title.is_empty() { r.file.clone() } else { title_case(&r.title) };
            let mut text = RichText::new(format!("{}  {}", r.number, name)).font(regular(12.5));
            if !r.exists {
                text = text.color(t.red);
            } else if !r.included {
                text = text.color(t.text_disabled).strikethrough();
            }
            let resp = ui.add(egui::Button::selectable(shown, text).frame_when_inactive(false)).on_hover_text(&r.file);
            if resp.clicked() && r.exists {
                action = Some(("ui.spec.openSection", json!({"file": r.file})));
            }
            resp.context_menu(|ui| {
                let label = if r.included { tl!("Leave Out of Project") } else { tl!("Include in Project") };
                if ui.button(label).clicked() {
                    action = Some(("ui.spec.include", json!({"file": r.file, "included": !r.included})));
                    ui.close();
                }
                if ui.button(tl!("Move Up")).clicked() {
                    action = Some(("ui.spec.move", json!({"file": r.file, "by": -1})));
                    ui.close();
                }
                if ui.button(tl!("Move Down")).clicked() {
                    action = Some(("ui.spec.move", json!({"file": r.file, "by": 1})));
                    ui.close();
                }
                ui.separator();
                if ui.button(tl!("Remove from Project")).on_hover_text(tl!("The file itself is kept")).clicked() {
                    action = Some(("ui.spec.removeSection", json!({"file": r.file})));
                    ui.close();
                }
            });
        }
        if let Some(f) = &findings {
            ui.add_space(8.0);
            ui.separator();
            let n = |k: &str| f.get(k).and_then(Value::as_u64).unwrap_or(0).to_string();
            let summary = crate::i18n::fmt(
                tl!("Check: {e} errors, {w} warnings, {n} notices"),
                &[("e", &n("errors")), ("w", &n("warnings")), ("n", &n("notices"))],
            );
            ui.label(RichText::new(summary).font(semibold(12.0)));
            for x in f.get("findings").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default() {
                let sev = x.get("severity").and_then(Value::as_str).unwrap_or("");
                let color = match sev {
                    "error" => t.red,
                    "warning" => t.orange,
                    _ => t.text_dim,
                };
                let file = x.get("file").and_then(Value::as_str);
                let msg = x.get("message").and_then(Value::as_str).unwrap_or("");
                let line = match file {
                    Some(file) => format!("{file}: {msg}"),
                    None => msg.to_string(),
                };
                let resp = ui.add(egui::Label::new(RichText::new(line).small().color(color)).wrap().sense(egui::Sense::click()));
                if let Some(file) = file
                    && resp.clicked()
                {
                    action = Some(("ui.spec.openSection", json!({"file": file})));
                }
            }
        }
    });
    if let Some((id, params)) = action
        && let Err(e) = app.run(id, params)
    {
        app.status(e);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wordcraft_engine::Session;

    use super::*;
    use crate::Services;

    fn app() -> WordApp {
        WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default())
    }

    /// A master section with a title, a Part and an end of section.
    fn master(number: &str, title: &str) -> wordcraft_doc::Document {
        use std::sync::Arc;
        let para = |style: &str, text: &str| {
            Arc::new(wordcraft_doc::Block::Para(wordcraft_doc::Paragraph::with_text(text, Default::default()).styled(style)))
        };
        let mut d = wordcraft_doc::Document::new();
        d.body = vec![para("SCT", &format!("SECTION {number} - {title}")), para("PRT", "GENERAL"), para("EOS", &format!("END OF SECTION {number}"))];
        d
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("speccraft-ui-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn sections_open_in_the_same_window_and_changes_are_saved_first() {
        let dir = scratch("switch");
        let project = dir.join("Riverside.speccraft");
        let mut a = app();
        a.run("ui.spec.newProject", json!({"path": project.to_string_lossy(), "name": "Riverside Library"})).unwrap();
        assert!(a.speccraft.pane_open);
        for (n, t, f) in [("09 2900", "GYPSUM BOARD", "m-gypsum.docx"), ("07 9200", "JOINT SEALANTS", "m-sealants.docx")] {
            let m = dir.join("masters");
            std::fs::create_dir_all(&m).unwrap();
            wordcraft_engine::io::save_path(&m.join(f), &master(n, t)).unwrap();
            a.run("ui.spec.addSection", json!({"master": m.join(f).to_string_lossy()})).unwrap();
        }
        // Adding a section shows it.
        assert_eq!(a.session.path.as_deref(), Some(dir.join("m-sealants.docx").as_path()));
        let docs = a.session.documents().len();

        // A change in the section on screen is saved before another is shown, in the same window.
        a.run("text.insert", json!({"text": "Edited. "})).unwrap();
        assert!(a.session.dirty);
        a.run("ui.spec.openSection", json!({"file": "m-gypsum.docx"})).unwrap();
        assert_eq!(a.session.path.as_deref(), Some(dir.join("m-gypsum.docx").as_path()));
        assert_eq!(a.session.documents().len(), docs, "no new window or document");
        let saved = wordcraft_engine::io::open_path(&dir.join("m-sealants.docx")).unwrap();
        assert!(saved.plain_text(wordcraft_doc::StoryRef::Body).contains("Edited. "));

        // Order, leave out, check.
        let r = a.run("ui.spec.move", json!({"file": "m-gypsum.docx", "by": -1})).unwrap();
        assert_eq!(r["sections"][0]["file"], "m-gypsum.docx");
        assert!(a.run("ui.spec.move", json!({"file": "m-gypsum.docx", "by": -1})).is_err());
        a.run("ui.spec.include", json!({"file": "m-sealants.docx", "included": false})).unwrap();
        let r = a.run("ui.spec.check", json!({})).unwrap();
        assert!(r.get("findings").is_some());
        assert!(a.run("ui.spec.openSection", json!({"file": "not-in-project.docx"})).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_pane_and_tab_draw_with_and_without_a_project() {
        let dir = scratch("draw");
        let mut a = app();
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        let draw = |a: &mut WordApp| {
            let out = ctx.run_ui(egui::RawInput::default(), |ui| {
                egui::Panel::left("spec_test").show(ui, |ui| pane(a, ui));
                egui::CentralPanel::default().show(ui, |ui| ui.horizontal(|ui| ribbon(a, ui)));
            });
            out.drop_without_applying_deltas();
        };
        draw(&mut a);
        let project = dir.join("P.speccraft");
        a.run("ui.spec.newProject", json!({"path": project.to_string_lossy()})).unwrap();
        assert_eq!(a.speccraft.info.as_ref().unwrap()["name"], "P");
        a.run("ui.spec.check", json!({})).unwrap();
        draw(&mut a);
        a.run("ui.spec.closeProject", json!({})).unwrap();
        assert!(a.speccraft.project.is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn titles_and_divisions() {
        assert_eq!(title_case("GYPSUM BOARD"), "Gypsum Board");
        assert_eq!(title_case("Gypsum board"), "Gypsum board");
        assert_eq!(division("09 2900"), "09");
        assert_eq!(division(""), "");
        let info = json!({"sections": [{"file": "a.docx"}, {"file": "b.docx"}, {"file": "c.docx"}]});
        assert_eq!(moved(Some(&info), "c.docx", -2).unwrap(), ["c.docx", "a.docx", "b.docx"]);
        assert_eq!(moved(Some(&info), "a.docx", -1), None);
        assert_eq!(moved(Some(&info), "a.docx", i64::MAX), None);
    }
}
