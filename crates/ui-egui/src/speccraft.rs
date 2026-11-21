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

pub(crate) mod checks;
pub(crate) mod library;
pub(crate) mod marks;
pub(crate) mod outline;
pub(crate) mod overview;
pub(crate) mod project_rules;
pub(crate) mod rules_view;
pub(crate) mod tabs;
pub(crate) mod updates;

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
    /// The Spec Outline pane is open.
    pub outline_open: bool,
    /// The page's spec marks are hidden (Specs › Show Marks off).
    pub hide_marks: bool,
    /// The Spec Outline pane's filter, search text and view.
    pub(crate) filter: outline::Filter,
    pub(crate) search: String,
    pub(crate) view: outline::View,
    /// Caches: the page's marks, `spec.outline`, and the Printed view's pages.
    pub(crate) marks_cache: Option<marks::MarksCache>,
    pub(crate) outline_cache: Option<((u64, u64), Value)>,
    pub(crate) fields_cache: Option<((u64, u64), Value)>,
    pub(crate) origin_cache: Option<((u64, u64), Value)>,
    pub(crate) rules_cache: Option<((u64, u64), Value)>,
    pub(crate) why_cache: Option<((u64, u64, String), Vec<String>)>,
    /// The Rules view's new-rule form.
    pub(crate) rule_form: rules_view::RuleForm,
    /// The Fill-ins view: filters, and text typed but not yet set, by field id.
    pub(crate) open_only: bool,
    pub(crate) printing_only: bool,
    pub(crate) drafts: std::collections::HashMap<String, String>,
    pub(crate) printed: Option<outline::Printed>,
    /// The project information being edited in the Sections pane.
    info_form: Option<InfoForm>,
    /// The Checks view's `spec.check`, per document revision (M10).
    pub(crate) check_cache: Option<((u64, u64), Value)>,
    /// Check Project's accepted findings are listed too.
    pub(crate) show_accepted: bool,
    /// The last Publish or Project Manual stopped at errors: which (the report offers to go
    /// ahead anyway).
    pub(crate) publish_blocked: Option<&'static str>,
    /// The form the Printed view shows, and the form Publish and Project Manual write (M16).
    pub(crate) print_form: wordcraft_engine::cmd::spec::forms::Form,
    pub(crate) publish_form: wordcraft_engine::cmd::spec::forms::Form,
    /// `spec.short.list`, per revision; the short wording being typed (M16).
    pub(crate) short_cache: Option<((u64, u64), Value)>,
    pub(crate) short_draft: Option<(String, String)>,
    /// What Publish and Project Manual write: `pdf` (default), `docx` or `both` (M13).
    pub(crate) publish_format: String,
    /// Open section tabs, the one used last at the end (M15).
    pub(crate) tab_order: Vec<u64>,
    /// The add form for rules between sections (W3).
    pub(crate) project_rule_form: project_rules::ProjectRuleForm,
    /// The Project Overview pane (M12).
    pub overview: overview::OverviewUi,
    /// The Updates view (M11).
    pub(crate) update: updates::UpdateUi,
    /// The Master Library pane (M9).
    pub library: library::LibraryUi,
}

/// Project information as typed in the Sections pane, before it is saved.
#[derive(Clone, Default)]
struct InfoForm {
    name: String,
    number: String,
    issue: String,
    date: String,
    /// Named values: (name, value).
    values: Vec<(String, String)>,
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
    if id.starts_with("ui.spec.library") {
        return library::command(app, id, p);
    }
    if id == "ui.spec.addProjectRule" || id == "ui.spec.removeProjectRule" {
        return project_rules::command(app, id, p);
    }
    if id == "ui.spec.overview" {
        return overview::command(app, p);
    }
    if id.starts_with("ui.spec.update") {
        return updates::command(app, id, p);
    }
    match id {
        "ui.spec.sectionsPane" => {
            app.speccraft.pane_open = p.get("value").and_then(Value::as_bool).unwrap_or(!app.speccraft.pane_open);
            Ok(json!({"open": app.speccraft.pane_open}))
        }
        "ui.spec.outlinePane" => {
            app.speccraft.outline_open = p.get("value").and_then(Value::as_bool).unwrap_or(!app.speccraft.outline_open);
            if let Some(v) = s("view") {
                app.speccraft.view = match v.as_str() {
                    "printed" => outline::View::Printed,
                    "fields" => outline::View::Fields,
                    "rules" => outline::View::Rules,
                    "checks" => outline::View::Checks,
                    "updates" => outline::View::Updates,
                    _ => outline::View::Outline,
                };
            }
            if p.get("form").is_some() {
                app.speccraft.print_form = wordcraft_engine::cmd::spec::forms::Form::from_params(p).map_err(|e| e.to_string())?;
            }
            Ok(json!({"open": app.speccraft.outline_open}))
        }
        "ui.spec.marks" => {
            let show = p.get("value").and_then(Value::as_bool).unwrap_or(app.speccraft.hide_marks);
            app.speccraft.hide_marks = !show;
            Ok(json!({"shown": show}))
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
        "ui.spec.tab" => {
            let tab = p.get("id").and_then(Value::as_u64).ok_or("`id` must be an open tab")?;
            tabs::switch(app, tab)?;
            Ok(json!({"id": tab}))
        }
        "ui.spec.closeTab" => {
            let tab = p.get("id").and_then(Value::as_u64).unwrap_or_else(|| app.session.document_id());
            tabs::close(app, tab)?;
            Ok(json!({"closed": tab, "tabs": app.session.documents().len()}))
        }
        "ui.spec.followReference" => {
            let (number, file) =
                tabs::reference_at_caret(app).ok_or_else(|| tl!("No section is named at the caret (as in \"Section 07 9200\")").to_string())?;
            let file = file.ok_or_else(|| crate::i18n::fmt(tl!("Section {n} is not in this project"), &[("n", &number)]))?;
            open_section(app, &file)
        }
        "ui.spec.goTo" => {
            let file = s("file").ok_or("`file` must name a section file of the project")?;
            let shown = app.session.path.as_deref().and_then(Path::file_name).is_some_and(|n| n.to_string_lossy() == file.as_str());
            if !shown {
                open_section(app, &file)?;
            }
            if let Some(para) = s("para") {
                checks::go_to_para(app, &para)?;
            }
            if let Some(view) = s("view") {
                app.run("ui.spec.outlinePane", json!({"value": true, "view": view}))?;
            }
            Ok(json!({"file": file}))
        }
        "ui.spec.acknowledge" => {
            let path = project_path(app)?;
            let key = p.get("key").and_then(Value::as_str).ok_or("`key` must be a finding's key")?;
            let mut args = json!({"path": path, "key": key, "undo": p.get("undo").and_then(Value::as_bool).unwrap_or(false)});
            if let Some(n) = s("note") {
                args["note"] = json!(n);
            }
            let r = app.run("spec.project.acknowledge", args)?;
            let check = app.run("spec.project.validate", json!({"path": path}))?;
            app.speccraft.findings = Some(check);
            Ok(r)
        }
        "ui.spec.check" => {
            let path = project_path(app)?;
            // It reads the section files: open sections with changes are saved first.
            save_if_changed(app)?;
            let r = app.run("spec.project.validate", json!({"path": path}))?;
            app.speccraft.publish_blocked = None;
            app.speccraft.findings = Some(r.clone());
            app.speccraft.pane_open = true;
            Ok(r)
        }
        "ui.spec.publishFormat" => {
            let f = s("format").filter(|f| matches!(f.as_str(), "pdf" | "docx" | "both")).ok_or("`format` must be \"pdf\", \"docx\" or \"both\"")?;
            app.speccraft.publish_format = f.clone();
            Ok(json!({"format": f}))
        }
        "ui.spec.publish" | "ui.spec.manual" => {
            let path = project_path(app)?;
            save_if_changed(app)?;
            let force = p.get("force").and_then(Value::as_bool).unwrap_or(false);
            let format = s("format")
                .unwrap_or_else(|| if app.speccraft.publish_format.is_empty() { "pdf".into() } else { app.speccraft.publish_format.clone() });
            let (engine, which) =
                if id == "ui.spec.manual" { ("spec.project.manual", "ui.spec.manual") } else { ("spec.project.publish", "ui.spec.publish") };
            let form = app.speccraft.publish_form;
            let depth = if let wordcraft_engine::cmd::spec::forms::Form::Outline(d) = form { d } else { 1 };
            let form_name = s("form").unwrap_or_else(|| form.name().to_string());
            let args =
                json!({"path": path, "force": force, "format": format, "form": form_name, "depth": p.get("depth").cloned().unwrap_or(json!(depth))});
            let r = match app.run(engine, args) {
                Ok(r) => r,
                Err(e) => {
                    // Stopped by errors: show them, with a way to go ahead anyway beside them.
                    if let Ok(check) = app.run("spec.project.validate", json!({"path": path})) {
                        let errors = check.get("errors").and_then(Value::as_u64).unwrap_or(0) > 0;
                        app.speccraft.publish_blocked = errors.then_some(which);
                        app.speccraft.findings = Some(check);
                        app.speccraft.pane_open = true;
                    }
                    return Err(e);
                }
            };
            app.speccraft.publish_blocked = None;
            if id == "ui.spec.manual" {
                let pages = r.get("pages").and_then(Value::as_u64).unwrap_or(0).to_string();
                let n = r.get("sections").and_then(Value::as_array).map_or(0, Vec::len).to_string();
                app.status(crate::i18n::fmt(
                    tl!("Project manual published: {n} sections, {pages} pages, in the Published folder"),
                    &[("n", &n), ("pages", &pages)],
                ));
                return Ok(r);
            }
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
    library::from_project(app, &info);
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
    // Sections waiting in other tabs too (M15): the project's checks and outputs read the files.
    tabs::save_sections(app)
}

/// Show section `file` of the project in a tab of its own (M15), or its tab if it is open.
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
    // In a tab of its own (M15): the section on screen keeps its changes in its tab.
    let was_open = tabs::open(app, &path)?;
    if !was_open {
        app.status(crate::i18n::fmt(tl!("Opened {file}"), &[("file", file)]));
    }
    Ok(json!({"path": path.to_string_lossy(), "alreadyOpen": was_open}))
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
    library::poll(app);
    let has_project = app.speccraft.project.is_some();
    group(ui, "Project", None, app, |ui, app| {
        big(ui, app, "open", "Open\nProject…", "ui.spec.openProject", json!({}), false);
        big(ui, app, "plus", "New\nProject…", "ui.spec.newProject", json!({}), false);
        let open = app.speccraft.pane_open;
        big_toggle(ui, app, "navPane", "Sections\nPane", "ui.spec.sectionsPane", open);
        let lib = app.speccraft.library.open;
        big_toggle(ui, app, "search", "Master\nLibrary", "ui.spec.library", lib);
        ui.add_enabled_ui(has_project, |ui| {
            big(ui, app, "file", "Add\nSection…", "ui.spec.addSection", json!({}), false);
            big(ui, app, "checkErrors", "Check\nProject", "ui.spec.check", json!({}), false);
            big(ui, app, "export", "Publish\nProject", "ui.spec.publish", json!({}), false);
            big(ui, app, "coverPage", "Project\nManual", "ui.spec.manual", json!({}), false);
            let shown = app.speccraft.overview.open;
            big(ui, app, "tableOfFigures", "Project\nOverview", "ui.spec.overview", json!({"value": !shown}), shown);
        });
    });
    group(ui, "Section", None, app, |ui, app| {
        big(ui, app, "multilevel", "Import\nParagraphs", "spec.import", json!({}), false);
        big(ui, app, "check", "Switch\nOn / Off", "spec.toggle", json!({}), false);
        let open = app.speccraft.outline_open;
        big_toggle(ui, app, "outline", "Outline\nPane", "ui.spec.outlinePane", open);
        let shown = !app.speccraft.hide_marks;
        big_toggle(ui, app, "pilcrow", "Show\nMarks", "ui.spec.marks", shown);
        big(ui, app, "ccComboBox", "Fill-ins", "ui.spec.outlinePane", json!({"value": true, "view": "fields"}), false);
        big(ui, app, "link", "Rules", "ui.spec.outlinePane", json!({"value": true, "view": "rules"}), false);
        big(ui, app, "checkErrors", "Check\nSection", "ui.spec.outlinePane", json!({"value": true, "view": "checks"}), false);
        big(ui, app, "compare", "Master\nUpdates", "ui.spec.outlinePane", json!({"value": true, "view": "updates"}), false);
        big(ui, app, "readMode", "Printed\nView", "ui.spec.outlinePane", json!({"value": true, "view": "printed"}), false);
        big(ui, app, "export", "Export\nPDF", "spec.exportPdf", json!({}), false);
        big(ui, app, "link", "Go to\nSection", "ui.spec.followReference", json!({}), false);
        big(ui, app, "saveAs", "Export\nWord", "spec.exportDocx", json!({}), false);
    });
    group(ui, "Edits", None, app, |ui, app| {
        big(ui, app, "plus", "Add\nParagraph", "spec.addParagraph", json!({}), false);
        big(ui, app, "copy", "Duplicate", "spec.duplicate", json!({}), false);
        big(ui, app, "recover", "Restore\nWording", "spec.restore", json!({}), false);
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
    project_info(app, ui, &info);
    project_rules::show(app, ui, &info);
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        if ui.small_button(tl!("Add Section…")).clicked() {
            let _ = app.run("ui.spec.addSection", json!({}));
        }
        if ui.small_button(tl!("Check")).clicked() {
            let _ = app.run("ui.spec.check", json!({}));
        }
        if ui.small_button(tl!("Publish")).on_hover_text(tl!("Each included section as its own file")).clicked() {
            let _ = app.run("ui.spec.publish", json!({}));
        }
        if ui.small_button(tl!("Manual")).on_hover_text(tl!("All included sections as one project manual, with a table of contents")).clicked() {
            let _ = app.run("ui.spec.manual", json!({}));
        }
        let fmt = if app.speccraft.publish_format.is_empty() { "pdf".to_string() } else { app.speccraft.publish_format.clone() };
        let label = |f: &str| match f {
            "docx" => tl!("Word"),
            "both" => tl!("PDF and Word"),
            _ => tl!("PDF"),
        };
        outline::form_choice(ui, "spec_publish_form", &mut app.speccraft.publish_form);
        craft_ui::ComboBox::from_id_salt("spec_publish_format").selected_text(label(&fmt)).show_ui(ui, |ui| {
            for f in ["pdf", "docx", "both"] {
                if ui.selectable_label(fmt == f, label(f)).clicked() {
                    app.speccraft.publish_format = f.to_string();
                }
            }
        });
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
            checks::project_report(app, ui, f, &mut action);
        }
    });
    if let Some((id, params)) = action
        && let Err(e) = app.run(id, params)
    {
        app.status(e);
    }
}

/// Project information and named values, editable in place. Values fill placeholders such as
/// `<Insert Architect Name>` when sections publish.
fn project_info(app: &mut WordApp, ui: &mut Ui, info: &Value) {
    let t = Tokens::get(ui.ctx());
    let header = egui::CollapsingHeader::new(RichText::new(tl!("Project Information")).small()).id_salt("spec_project_info").default_open(false);
    let shown = header.show(ui, |ui| {
        if app.speccraft.info_form.is_none() {
            let f = |k: &str| info.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            let values = info
                .get("values")
                .and_then(Value::as_object)
                .map(|m| m.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string())).collect())
                .unwrap_or_default();
            app.speccraft.info_form = Some(InfoForm { name: f("name"), number: f("number"), issue: f("issue"), date: f("date"), values });
        }
        let Some(form) = app.speccraft.info_form.as_mut() else { return };
        egui::Grid::new("spec_info_grid").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
            for (label, field) in [("Name", &mut form.name), ("Number", &mut form.number), ("Issue", &mut form.issue), ("Date", &mut form.date)] {
                ui.label(RichText::new(tl!(label)).small());
                ui.add(egui::TextEdit::singleline(field).desired_width(160.0));
                ui.end_row();
            }
        });
        ui.add_space(4.0);
        ui.label(RichText::new(tl!("Values for placeholders such as <Insert Architect Name>")).small().color(t.text_dim));
        let mut remove = None;
        for (i, (k, v)) in form.values.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(k).hint_text(tl!("Name")).desired_width(90.0));
                ui.add(egui::TextEdit::singleline(v).hint_text(tl!("Value")).desired_width(90.0));
                if ui.small_button("×").on_hover_text(tl!("Remove")).clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            form.values.remove(i);
        }
        let mut save = false;
        ui.horizontal(|ui| {
            if ui.small_button(tl!("Add Value")).clicked() {
                form.values.push((String::new(), String::new()));
            }
            save = ui.small_button(tl!("Save")).clicked();
        });
        if save {
            let form = form.clone();
            if let Err(e) = save_info(app, info, &form) {
                app.status(e);
            }
        }
    });
    if shown.body_returned.is_none() {
        // Closed: show the saved values next time it opens.
        app.speccraft.info_form = None;
    }
}

/// Save the project information form: fields, and named values (dropped names removed).
fn save_info(app: &mut WordApp, info: &Value, form: &InfoForm) -> Result<Value, String> {
    let path = project_path(app)?;
    let mut values = serde_json::Map::new();
    if let Some(old) = info.get("values").and_then(Value::as_object) {
        for k in old.keys() {
            values.insert(k.clone(), Value::Null);
        }
    }
    for (k, v) in &form.values {
        if !k.trim().is_empty() {
            values.insert(k.trim().to_string(), json!(v));
        }
    }
    let r = app.run(
        "spec.project.set",
        json!({"path": path, "name": form.name, "number": form.number, "issue": form.issue, "date": form.date, "values": values}),
    )?;
    app.speccraft.info = Some(r.clone());
    app.speccraft.info_form = None;
    app.status(tl!("Project information saved"));
    Ok(r)
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
    fn sections_open_in_tabs_and_changes_are_saved_before_checking() {
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

        // Both sections are open in tabs (M15): showing the other one keeps the change in its tab.
        a.run("text.insert", json!({"text": "Edited. "})).unwrap();
        assert!(a.session.dirty);
        a.run("ui.spec.openSection", json!({"file": "m-gypsum.docx"})).unwrap();
        assert_eq!(a.session.path.as_deref(), Some(dir.join("m-gypsum.docx").as_path()));
        assert_eq!(a.session.documents().len(), docs, "its tab was open: no new document");
        let saved = wordcraft_engine::io::open_path(&dir.join("m-sealants.docx")).unwrap();
        assert!(!saved.plain_text(wordcraft_doc::StoryRef::Body).contains("Edited. "), "waiting in its tab");
        // Checking the project saves the open sections first.
        a.run("ui.spec.check", json!({})).unwrap();
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

    /// A section with a Part, an Article, a paragraph under it and a note, imported.
    fn imported_app() -> WordApp {
        use std::sync::Arc;
        let para = |style: &str, text: &str| {
            Arc::new(wordcraft_doc::Block::Para(wordcraft_doc::Paragraph::with_text(text, Default::default()).styled(style)))
        };
        let mut d = wordcraft_doc::Document::new();
        d.body = vec![
            para("SCT", "SECTION 09 2900 - GYPSUM BOARD"),
            para("PRT", "GENERAL"),
            para("ART", "SUMMARY"),
            para("CMT", "Retain this article."),
            para("PR1", "Section includes gypsum board."),
            para("EOS", "END OF SECTION 09 2900"),
        ];
        let mut a = WordApp::new(Session::new(d), Services::default());
        a.run("spec.import", json!({})).unwrap();
        a
    }

    #[test]
    fn the_outline_pane_lists_filters_and_switches_paragraphs() {
        let mut a = imported_app();
        a.run("ui.spec.outlinePane", json!({"value": true})).unwrap();
        assert!(crate::panes::open(&a).contains(&crate::panes::Pane::SpecOutline));
        let o = outline::outline(&mut a);
        assert_eq!(o["items"].as_array().unwrap().len(), 6);
        // Switching the paragraph under an off Article marks it refused, then the chain brings it on.
        assert_eq!(a.run("spec.toggle", json!({"id": "p0005"})).unwrap()["state"], "refused");
        a.run("spec.activate", json!({"ids": ["p0002", "p0003"]})).unwrap();
        let o = outline::outline(&mut a);
        let state = |id: &str| o["items"].as_array().unwrap().iter().find(|i| i["id"] == id).unwrap()["state"].clone();
        assert_eq!(state("p0005"), "on");
        assert_eq!(state("p0004"), "note");

        // The pane draws in both views, filtered and searched; the Printed view renders pages.
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        for (view, filter, search) in [
            (outline::View::Outline, outline::Filter::All, ""),
            (outline::View::Outline, outline::Filter::Notes, "retain"),
            (outline::View::Printed, outline::Filter::All, ""),
        ] {
            a.speccraft.view = view;
            a.speccraft.filter = filter;
            a.speccraft.search = search.into();
            let out = ctx.run_ui(egui::RawInput::default(), |ui| {
                egui::Panel::right("outline_test").default_size(320.0).show(ui, |ui| outline::pane(&mut a, ui));
            });
            out.drop_without_applying_deltas();
        }
        let printed = a.speccraft.printed.as_ref().unwrap();
        assert_eq!(printed.pages.len(), 1, "{:?}", printed.note);
    }

    #[test]
    fn marks_are_found_for_every_paragraph_and_can_be_hidden() {
        let mut a = imported_app();
        let layout = a.session.layout();
        let marks = marks::marked(&mut a, &layout);
        assert_eq!(marks.len(), 6);
        assert!(marks.iter().all(|m| !m.pieces.is_empty()), "each paragraph is on a page");
        // Cached until the document changes.
        assert!(std::sync::Arc::ptr_eq(&marks, &marks::marked(&mut a, &layout)));
        a.run("spec.toggle", json!({"id": "p0002"})).unwrap();
        let layout = a.session.layout();
        let again = marks::marked(&mut a, &layout);
        assert!(!std::sync::Arc::ptr_eq(&marks, &again));
        assert_eq!(again.iter().find(|m| m.id == "p0002").unwrap().state, wordcraft_engine::cmd::spec::sections::State::On);
        assert_eq!(a.run("ui.spec.marks", json!({"value": false})).unwrap()["shown"], false);
        assert!(a.speccraft.hide_marks);
        assert_eq!(a.run("ui.spec.marks", json!({})).unwrap()["shown"], true);
    }

    #[test]
    fn project_information_is_saved_with_its_values() {
        let dir = scratch("info");
        let mut a = app();
        let project = dir.join("P.speccraft");
        a.run("ui.spec.newProject", json!({"path": project.to_string_lossy()})).unwrap();
        let info = a.speccraft.info.clone().unwrap();
        let form = InfoForm {
            name: "Riverside Library".into(),
            number: "2026-14".into(),
            issue: "Issued for Bid".into(),
            date: "December 1, 2026".into(),
            values: vec![("Architect Name".into(), "Studio North".into())],
        };
        save_info(&mut a, &info, &form).unwrap();
        let info = a.speccraft.info.clone().unwrap();
        assert_eq!((info["name"].clone(), info["values"]["Architect Name"].clone()), (json!("Riverside Library"), json!("Studio North")));
        // A value dropped from the form is removed from the project.
        let form = InfoForm { values: vec![], ..form };
        save_info(&mut a, &info, &form).unwrap();
        assert_eq!(a.speccraft.info.as_ref().unwrap()["values"], json!({}));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_fill_ins_view_lists_and_the_page_marks_fields() {
        let mut a = imported_app();
        // Give the paragraph a choice and a blank, then find them.
        let at = wordcraft_doc::Pos::new(wordcraft_doc::StoryRef::Body, wordcraft_doc::Path(vec![4]), 0);
        a.run("caret.set", json!({"pos": wordcraft_engine::cmd::pos_json(&at)})).unwrap();
        a.run("text.insert", json!({"text": "Thickness [1/2 inch] [5/8 inch], gap ___ mm. "})).unwrap();
        let r = a.run("spec.fields.detect", json!({})).unwrap();
        assert_eq!(r["added"], 2, "{r}");
        a.run("ui.spec.outlinePane", json!({"value": true, "view": "fields"})).unwrap();
        assert_eq!(a.speccraft.view, outline::View::Fields);
        assert_eq!(outline::field_list(&mut a)["count"], 2);
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        for open_only in [false, true] {
            a.speccraft.open_only = open_only;
            let out = ctx.run_ui(egui::RawInput::default(), |ui| {
                egui::Panel::right("fill_ins_test").default_size(320.0).show(ui, |ui| outline::pane(&mut a, ui));
            });
            out.drop_without_applying_deltas();
        }
        let layout = a.session.layout();
        marks::marked(&mut a, &layout);
        let fields = a.speccraft.marks_cache.as_ref().unwrap().fields.clone();
        assert_eq!(fields.len(), 2);
        assert!(fields.iter().all(|f| !f.pieces.is_empty()));
        // Setting a choice shows in the list.
        a.run("spec.fields.set", json!({"id": "f0001", "option": 1})).unwrap();
        assert_eq!(outline::field_list(&mut a)["fields"][0]["value"], "5/8 inch");
    }

    #[test]
    fn edits_show_their_origin_in_the_outline_and_on_the_page() {
        let mut a = imported_app();
        // Reword p0005, add a paragraph after it.
        let at = wordcraft_doc::Pos::new(wordcraft_doc::StoryRef::Body, wordcraft_doc::Path(vec![4]), wordcraft_doc::para::OBJ.len_utf8());
        a.run("caret.set", json!({"pos": wordcraft_engine::cmd::pos_json(&at)})).unwrap();
        a.run("text.insert", json!({"text": "This "})).unwrap();
        a.run("spec.addParagraph", json!({"after": "p0005", "text": "Provide corner bead."})).unwrap();
        let o = outline::origins(&mut a);
        assert_eq!(o["counts"], json!({"added": 1, "master": 5, "modified": 1}));
        let layout = a.session.layout();
        let marks = marks::marked(&mut a, &layout);
        use wordcraft_engine::cmd::spec::edits::Origin;
        assert_eq!(marks.iter().find(|m| m.id == "p0005").unwrap().origin, Origin::Modified);
        assert_eq!(marks.iter().find(|m| m.id == "u0001").unwrap().origin, Origin::Added);
        // The outline draws filtered to changes, with the caret's details.
        a.speccraft.filter = outline::Filter::Changed;
        a.run("ui.spec.outlinePane", json!({"value": true, "view": "outline"})).unwrap();
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        let out = ctx.run_ui(egui::RawInput::default(), |ui| {
            egui::Panel::right("edits_test").default_size(320.0).show(ui, |ui| outline::pane(&mut a, ui));
        });
        out.drop_without_applying_deltas();
        // Restore through the command the panel's button runs.
        a.run("spec.restore", json!({"id": "p0005"})).unwrap();
        assert_eq!(outline::origins(&mut a)["counts"], json!({"added": 1, "master": 6}));
    }

    #[test]
    fn rules_show_on_the_page_and_in_the_rules_view() {
        let mut a = imported_app();
        // SUMMARY (p0003) on requires "Section includes" (p0005); a recommendation on GENERAL.
        a.run("spec.rules.add", json!({"kind": "require", "if": [{"on": "p0003"}], "target": "p0005"})).unwrap();
        a.run("spec.rules.add", json!({"kind": "recommend", "if": [{"on": "p0002"}], "target": "p0003"})).unwrap();
        a.run("spec.activate", json!({"id": "p0002"})).unwrap();
        let layout = a.session.layout();
        let marks = marks::marked(&mut a, &layout);
        use wordcraft_engine::cmd::spec::rules::Standing;
        assert_eq!(marks.iter().find(|m| m.id == "p0003").unwrap().advice, Some(Standing::Suggested));
        a.run("spec.activate", json!({"id": "p0003"})).unwrap();
        let layout = a.session.layout();
        let marks = marks::marked(&mut a, &layout);
        use wordcraft_engine::cmd::spec::sections::State;
        assert_eq!(marks.iter().find(|m| m.id == "p0005").unwrap().state, State::Linked);
        a.run("spec.toggle", json!({"id": "p0005"})).unwrap();
        let layout = a.session.layout();
        let marks = marks::marked(&mut a, &layout);
        assert_eq!(marks.iter().find(|m| m.id == "p0005").unwrap().state, State::Overridden);
        // The Rules view and the outline details draw.
        a.run("ui.spec.outlinePane", json!({"value": true, "view": "rules"})).unwrap();
        assert_eq!(rules_view::rule_list(&mut a)["count"], 2);
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        for view in [outline::View::Rules, outline::View::Outline] {
            a.speccraft.view = view;
            let out = ctx.run_ui(egui::RawInput::default(), |ui| {
                egui::Panel::right("rules_test").default_size(320.0).show(ui, |ui| outline::pane(&mut a, ui));
            });
            out.drop_without_applying_deltas();
        }
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
