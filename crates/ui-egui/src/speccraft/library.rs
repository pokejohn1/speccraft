//! The Master Library pane (Speccraft milestone M9). Added by the Speccraft fork; not part of the
//! original WordCraft.
//!
//! The pane searches a folder of masters by number, title, text and tags, previews a master's
//! outline without opening it, and adds it to the open project. Its index and searching are the
//! engine's `spec.library.*` commands; the UI commands here (`ui.spec.library*`) drive the pane.

use std::path::{Path, PathBuf};

use egui::{RichText, Ui};
use serde_json::{Value, json};

use crate::WordApp;
use crate::theme::{Tokens, regular, semibold};

/// What the Library pane keeps between frames.
#[derive(Default)]
pub struct LibraryUi {
    /// The pane is open.
    pub open: bool,
    /// The library folder.
    pub folder: Option<PathBuf>,
    /// The search box, and the filters.
    pub(crate) query: String,
    pub(crate) division: String,
    pub(crate) approved_only: bool,
    /// `spec.library.info` for the folder, and the last search with what it was for.
    pub(crate) info: Option<Value>,
    pub(crate) results: Option<(String, Value)>,
    /// The master being previewed: `spec.library.preview`, and its tags and note as typed.
    pub(crate) preview: Option<Value>,
    pub(crate) tags: String,
    pub(crate) note: String,
    /// A file dialog for choosing the folder is open.
    pub(crate) waiting: bool,
}

/// The `ui.spec.library*` commands.
pub(crate) fn command(app: &mut WordApp, id: &str, p: &Value) -> Result<Value, String> {
    let s = |k: &str| p.get(k).and_then(Value::as_str).map(str::to_string);
    match id {
        "ui.spec.library" => {
            let lib = &mut app.speccraft.library;
            lib.open = p.get("value").and_then(Value::as_bool).unwrap_or(!lib.open);
            Ok(json!({"open": lib.open}))
        }
        "ui.spec.libraryFolder" => match s("folder") {
            Some(f) => set_folder(app, PathBuf::from(f)),
            None => {
                app.speccraft.library.open = true;
                app.speccraft.library.waiting = true;
                app.dialog_pick = None;
                let _ = app
                    .ask_file(crate::file_dialogs::FileDialogRequest::Open { purpose: "document".into() }, crate::file_dialogs::AfterPick::ForDialog);
                poll(app);
                Ok(json!({"asked": true}))
            }
        },
        "ui.spec.libraryScan" => scan(app),
        "ui.spec.librarySearch" => {
            let lib = &mut app.speccraft.library;
            if let Some(q) = s("query") {
                lib.query = q;
            }
            if let Some(d) = s("division") {
                lib.division = d;
            }
            if let Some(a) = p.get("approved").and_then(Value::as_bool) {
                lib.approved_only = a;
            }
            lib.results = None;
            search(app)
        }
        "ui.spec.libraryPreview" => {
            let file = s("file").ok_or("`file` must name a master in the library")?;
            preview(app, &file)
        }
        "ui.spec.libraryTag" => {
            let folder = folder(app)?;
            let file = s("file").ok_or("`file` must name a master in the library")?;
            let mut args = json!({"folder": folder, "file": file});
            for k in ["tags", "approved", "note"] {
                if let Some(v) = p.get(k) {
                    args[k] = v.clone();
                }
            }
            let r = app.run("spec.library.tag", args)?;
            refresh(app)?;
            if app.speccraft.library.preview.as_ref().and_then(|p| p.get("file")).and_then(Value::as_str) == Some(file.as_str()) {
                preview(app, &file)?;
            }
            Ok(r)
        }
        "ui.spec.libraryAdd" => {
            let file = s("file").ok_or("`file` must name a master in the library")?;
            let folder = app.speccraft.library.folder.clone().ok_or("choose a library folder first")?;
            super::add_section(app, &folder.join(&file).to_string_lossy())
        }
        _ => Err(format!("unknown command `{id}`")),
    }
}

fn folder(app: &WordApp) -> Result<String, String> {
    app.speccraft.library.folder.as_ref().map(|f| f.to_string_lossy().into_owned()).ok_or_else(|| "choose a library folder first".to_string())
}

/// Finish the folder dialog: the folder of the master picked.
pub(crate) fn poll(app: &mut WordApp) {
    if !app.speccraft.library.waiting || app.file_dialog_open() || app.file_browser_open() {
        return;
    }
    app.speccraft.library.waiting = false;
    let Some(path) = app.dialog_pick.take() else { return };
    let Some(dir) = Path::new(&path).parent().map(Path::to_path_buf) else { return };
    if let Err(e) = set_folder(app, dir) {
        app.status(e);
    }
}

/// Use `dir` as the library: scan it, and remember it in the open project.
fn set_folder(app: &mut WordApp, dir: PathBuf) -> Result<Value, String> {
    if !dir.is_dir() {
        return Err(crate::i18n::fmt(tl!("{folder} is not a folder"), &[("folder", &dir.to_string_lossy())]));
    }
    let lib = &mut app.speccraft.library;
    lib.folder = Some(dir.clone());
    lib.open = true;
    lib.preview = None;
    let r = scan(app)?;
    if let Some(project) = app.speccraft.project.clone() {
        let info = app.run("spec.project.set", json!({"path": project.to_string_lossy(), "library": dir.to_string_lossy()}))?;
        app.speccraft.info = Some(info);
    }
    Ok(r)
}

/// The library a project remembers, used when the project opens (if no library is chosen yet).
pub(crate) fn from_project(app: &mut WordApp, info: &Value) {
    let Some(dir) = info.get("library").and_then(Value::as_str).filter(|d| !d.is_empty()).map(PathBuf::from) else { return };
    if app.speccraft.library.folder.as_ref() == Some(&dir) || !dir.is_dir() {
        return;
    }
    app.speccraft.library.folder = Some(dir);
    app.speccraft.library.preview = None;
    if let Err(e) = scan(app) {
        app.status(e);
    }
}

fn scan(app: &mut WordApp) -> Result<Value, String> {
    let folder = folder(app)?;
    let r = app.run("spec.library.scan", json!({"folder": folder}))?;
    let n = r.get("masters").and_then(Value::as_u64).unwrap_or(0);
    app.status(crate::i18n::fmt(tl!("{n} masters in the library"), &[("n", &n.to_string())]));
    refresh(app)?;
    Ok(r)
}

/// Read the index again (after a scan or a tag change) and forget the last search.
fn refresh(app: &mut WordApp) -> Result<(), String> {
    let folder = folder(app)?;
    let info = app.run("spec.library.info", json!({"folder": folder}))?;
    app.speccraft.library.info = Some(info);
    app.speccraft.library.results = None;
    Ok(())
}

/// What the current search is for, to know when it must run again.
fn search_key(lib: &LibraryUi) -> String {
    format!("{}\u{1}{}\u{1}{}", lib.query.trim(), lib.division, lib.approved_only)
}

fn search(app: &mut WordApp) -> Result<Value, String> {
    let folder = folder(app)?;
    let lib = &app.speccraft.library;
    let key = search_key(lib);
    if let Some((k, r)) = &lib.results
        && *k == key
    {
        return Ok(r.clone());
    }
    let mut args = json!({"folder": folder, "query": lib.query.trim()});
    if !lib.division.is_empty() {
        args["division"] = json!(lib.division);
    }
    if lib.approved_only {
        args["approved"] = json!(true);
    }
    let r = app.run("spec.library.search", args)?;
    app.speccraft.library.results = Some((key, r.clone()));
    Ok(r)
}

fn preview(app: &mut WordApp, file: &str) -> Result<Value, String> {
    let folder = folder(app)?;
    let r = app.run("spec.library.preview", json!({"folder": folder, "file": file}))?;
    let lib = &mut app.speccraft.library;
    lib.tags = r.get("tags").and_then(Value::as_array).map(|t| t.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default();
    lib.note = r.get("note").and_then(Value::as_str).unwrap_or("").to_string();
    lib.preview = Some(r.clone());
    Ok(r)
}

/// The section numbers the open project already has (as digits only).
fn in_project(app: &WordApp) -> Vec<String> {
    let digits = |s: &str| s.chars().filter(char::is_ascii_digit).collect::<String>();
    app.speccraft
        .info
        .as_ref()
        .and_then(|i| i.get("sections"))
        .and_then(Value::as_array)
        .map(|v| v.iter().filter_map(|s| s.get("number").and_then(Value::as_str)).map(digits).filter(|d| !d.is_empty()).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------
// The pane

/// The Master Library pane.
pub(crate) fn pane(app: &mut WordApp, ui: &mut Ui) {
    poll(app);
    let t = Tokens::get(ui.ctx());
    let mut action: Option<(&'static str, Value)> = None;
    let Some(dir) = app.speccraft.library.folder.clone() else {
        ui.label(RichText::new(tl!("No library folder is chosen.")).font(regular(13.0)));
        ui.add_space(6.0);
        ui.label(RichText::new(tl!("A library is a folder of master sections. Pick any master in it to use its folder.")).small().color(t.text_dim));
        ui.add_space(8.0);
        if ui.button(tl!("Choose Library Folder…")).clicked() {
            let _ = app.run("ui.spec.libraryFolder", json!({}));
        }
        return;
    };
    header(app, ui, &dir, &t, &mut action);
    ui.separator();
    let results = search(app).unwrap_or_else(|e| json!({"error": e}));
    let preview_h = if app.speccraft.library.preview.is_some() { (ui.available_height() * 0.5).clamp(200.0, 420.0) } else { 0.0 };
    let list_h = (ui.available_height() - preview_h).max(80.0);
    let taken = in_project(app);
    let has_project = app.speccraft.project.is_some();
    let current = app.speccraft.library.preview.as_ref().and_then(|p| p.get("file")).and_then(Value::as_str).map(str::to_string);
    egui::ScrollArea::vertical().id_salt("spec_library_results").max_height(list_h).auto_shrink([false, false]).show(ui, |ui| {
        result_list(ui, &t, &results, &taken, current.as_deref(), has_project, &mut action);
    });
    if let Some(p) = app.speccraft.library.preview.clone() {
        ui.separator();
        preview_panel(app, ui, &t, &p, &taken, &mut action);
    }
    if let Some((id, args)) = action
        && let Err(e) = app.run(id, args)
    {
        app.status(e);
    }
}

/// The folder line, search box and filters.
fn header(app: &mut WordApp, ui: &mut Ui, dir: &Path, t: &Tokens, action: &mut Option<(&'static str, Value)>) {
    let name = dir.file_name().map_or_else(|| dir.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned());
    let count = app.speccraft.library.info.as_ref().and_then(|i| i.get("count")).and_then(Value::as_u64).unwrap_or(0);
    ui.label(RichText::new(name).font(semibold(13.5))).on_hover_text(dir.to_string_lossy());
    ui.label(RichText::new(crate::i18n::fmt(tl!("{n} masters"), &[("n", &count.to_string())])).small().color(t.text_dim));
    ui.horizontal_wrapped(|ui| {
        if ui.small_button(tl!("Change Folder…")).clicked() {
            *action = Some(("ui.spec.libraryFolder", json!({})));
        }
        if ui.small_button(tl!("Rescan")).on_hover_text(tl!("Read new and changed masters")).clicked() {
            *action = Some(("ui.spec.libraryScan", json!({})));
        }
    });
    ui.add_space(4.0);
    let lib = &mut app.speccraft.library;
    ui.add(egui::TextEdit::singleline(&mut lib.query).hint_text(tl!("Number, title, words or tag")).desired_width(f32::INFINITY));
    let divisions: Vec<(String, String)> = lib
        .info
        .as_ref()
        .and_then(|i| i.get("divisions"))
        .and_then(Value::as_array)
        .map(|v| {
            v.iter()
                .map(|d| (d["division"].as_str().unwrap_or("").to_string(), d["name"].as_str().unwrap_or("").to_string()))
                .filter(|(d, _)| !d.is_empty())
                .collect()
        })
        .unwrap_or_default();
    ui.horizontal(|ui| {
        let shown =
            if lib.division.is_empty() { tl!("All divisions").to_string() } else { crate::i18n::fmt(tl!("Division {n}"), &[("n", &lib.division)]) };
        craft_ui::ComboBox::from_id_salt("spec_library_division").selected_text(shown).show_ui(ui, |ui| {
            if ui.selectable_label(lib.division.is_empty(), tl!("All divisions")).clicked() {
                lib.division.clear();
            }
            for (d, n) in &divisions {
                if ui.selectable_label(lib.division == *d, format!("{d}  {n}")).clicked() {
                    lib.division = d.clone();
                }
            }
        });
        ui.checkbox(&mut lib.approved_only, tl!("Approved only"));
    });
}

/// The search results, grouped by division.
fn result_list(
    ui: &mut Ui,
    t: &Tokens,
    results: &Value,
    taken: &[String],
    current: Option<&str>,
    has_project: bool,
    action: &mut Option<(&'static str, Value)>,
) {
    if let Some(e) = results.get("error").and_then(Value::as_str) {
        ui.label(RichText::new(e).small().color(t.red));
        return;
    }
    let list = results.get("results").and_then(Value::as_array).cloned().unwrap_or_default();
    if list.is_empty() {
        ui.label(RichText::new(tl!("No masters match.")).small().color(t.text_dim));
        return;
    }
    // Grouped by division in number order when browsing; best match first when searching.
    let mut last = None;
    for r in &list {
        let s = |k: &str| r.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let d = s("division");
        if last.as_ref() != Some(&d) {
            ui.add_space(4.0);
            let head = if d.is_empty() { tl!("Without a section number").to_string() } else { format!("{d}  {}", s("divisionName")) };
            ui.label(RichText::new(head).font(semibold(11.5)).color(t.text_dim));
            last = Some(d);
        }
        let file = s("file");
        let digits: String = s("number").chars().filter(char::is_ascii_digit).collect();
        let added = !digits.is_empty() && taken.contains(&digits);
        let name = if s("title").is_empty() { file.clone() } else { super::title_case(&s("title")) };
        let mut text = RichText::new(format!("{}  {}", s("number"), name)).font(regular(12.5));
        if !s("error").is_empty() {
            text = text.color(t.red);
        }
        ui.horizontal(|ui| {
            let resp = ui.selectable_label(current == Some(file.as_str()), text);
            let mut hover = file.clone();
            if !s("error").is_empty() {
                hover = format!("{hover}\n{}", s("error"));
            }
            if resp.on_hover_text(hover).clicked() {
                *action = Some(("ui.spec.libraryPreview", json!({"file": file})));
            }
            if r.get("approved").and_then(Value::as_bool) == Some(true) {
                ui.label(RichText::new("✓").color(t.green)).on_hover_text(tl!("Approved"));
            }
            if added {
                ui.label(RichText::new(tl!("in project")).small().color(t.text_dim));
            } else if has_project && s("error").is_empty() && ui.small_button("+").on_hover_text(tl!("Add to Project")).clicked() {
                *action = Some(("ui.spec.libraryAdd", json!({"file": file})));
            }
        });
        let tags: Vec<&str> = r.get("tags").and_then(Value::as_array).map(|v| v.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        if !tags.is_empty() {
            ui.label(RichText::new(tags.join(" · ")).small().color(t.accent));
        }
        if let Some(snip) = r.get("snippet").and_then(Value::as_str) {
            ui.label(RichText::new(format!("…{snip}…")).small().italics().color(t.text_dim));
        }
    }
}

/// The previewed master: its details, tags and outline.
fn preview_panel(app: &mut WordApp, ui: &mut Ui, t: &Tokens, p: &Value, taken: &[String], action: &mut Option<(&'static str, Value)>) {
    let s = |k: &str| p.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let file = s("file");
    ui.label(RichText::new(format!("{}  {}", s("number"), super::title_case(&s("title")))).font(semibold(13.0)));
    let mut sub = vec![file.clone()];
    if !s("version").is_empty() {
        sub.push(s("version"));
    }
    ui.label(RichText::new(sub.join(" · ")).small().color(t.text_dim));
    let digits: String = s("number").chars().filter(char::is_ascii_digit).collect();
    ui.horizontal_wrapped(|ui| {
        let can_add = app.speccraft.project.is_some() && !(!digits.is_empty() && taken.contains(&digits));
        let hint = if app.speccraft.project.is_none() { tl!("Open a project first") } else { tl!("Copy this master into the project") };
        if ui.add_enabled(can_add, egui::Button::new(tl!("Add to Project")).small()).on_hover_text(hint).on_disabled_hover_text(hint).clicked() {
            *action = Some(("ui.spec.libraryAdd", json!({"file": file})));
        }
        let mut approved = p.get("approved").and_then(Value::as_bool).unwrap_or(false);
        if ui.checkbox(&mut approved, tl!("Approved")).on_hover_text(tl!("Mark this master as the office's approved version")).changed() {
            *action = Some(("ui.spec.libraryTag", json!({"file": file, "approved": approved})));
        }
        if ui.small_button(tl!("Close")).on_hover_text(tl!("Close the preview")).clicked() {
            app.speccraft.library.preview = None;
        }
    });
    let lib = &mut app.speccraft.library;
    let tags = ui.add(egui::TextEdit::singleline(&mut lib.tags).hint_text(tl!("Tags, separated by commas")).desired_width(f32::INFINITY));
    let note = ui.add(egui::TextEdit::singleline(&mut lib.note).hint_text(tl!("Note")).desired_width(f32::INFINITY));
    if (tags.lost_focus() || note.lost_focus()) && action.is_none() {
        let list: Vec<String> = lib.tags.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
        let was_tags: Vec<String> =
            p.get("tags").and_then(Value::as_array).map(|v| v.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
        if list != was_tags || lib.note.trim() != s("note") {
            *action = Some(("ui.spec.libraryTag", json!({"file": file, "tags": list, "note": lib.note.trim()})));
        }
    }
    ui.add_space(4.0);
    let paras = p.get("paras").and_then(Value::as_array).cloned().unwrap_or_default();
    egui::ScrollArea::vertical().id_salt("spec_library_preview").auto_shrink([false, false]).show(ui, |ui| {
        for q in paras.iter().take(1_500) {
            let kind = q.get("k").and_then(Value::as_str).unwrap_or("");
            let text = q.get("t").and_then(Value::as_str).unwrap_or("");
            let level = q.get("l").and_then(Value::as_u64).unwrap_or(0) as f32;
            let (indent, rich) = match kind {
                "title" | "end" => (0.0, RichText::new(text).font(semibold(12.0))),
                "part" => (0.0, RichText::new(text).font(semibold(11.5))),
                "article" => (8.0, RichText::new(text).font(semibold(11.5))),
                "note" => (16.0, RichText::new(text).small().italics().color(t.text_dim)),
                _ => (16.0 + (level - 3.0).max(0.0) * 12.0, RichText::new(text).font(regular(11.5))),
            };
            ui.horizontal_top(|ui| {
                ui.add_space(indent);
                ui.add(egui::Label::new(rich).wrap());
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wordcraft_doc::{Block, Document, Paragraph};
    use wordcraft_engine::Session;

    use crate::Services;
    use crate::WordApp;

    fn master(number: &str, title: &str) -> Document {
        let p = |s: &str, t: &str| std::sync::Arc::new(Block::Para(Paragraph::with_text(t, Default::default()).styled(s)));
        let mut d = Document::new();
        d.body =
            vec![p("SCT", &format!("SECTION {number} - {title}")), p("PRT", "GENERAL"), p("PR1", "Provide the work."), p("EOS", "END OF SECTION")];
        d
    }

    #[test]
    fn the_library_finds_previews_tags_and_adds_masters() {
        let dir = std::env::temp_dir().join(format!("speccraft-ui-library-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let lib = dir.join("library");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::create_dir_all(dir.join("project")).unwrap();
        for (n, t, f) in [("09 2900", "GYPSUM BOARD", "092900.docx"), ("07 9200", "JOINT SEALANTS", "079200.docx")] {
            wordcraft_engine::io::save_path(&lib.join(f), &master(n, t)).unwrap();
        }
        let mut a = WordApp::new(Session::new(Document::new()), Services::default());
        let project = dir.join("project").join("Riverside.speccraft");
        a.run("ui.spec.newProject", json!({"path": project.to_string_lossy(), "name": "Riverside"})).unwrap();
        let r = a.run("ui.spec.libraryFolder", json!({"folder": lib.to_string_lossy()})).unwrap();
        assert_eq!(r["masters"], 2);
        // The project remembers the library.
        assert_eq!(a.speccraft.info.as_ref().unwrap()["library"], lib.to_string_lossy().as_ref());
        let r = a.run("ui.spec.librarySearch", json!({"query": "sealant"})).unwrap();
        assert_eq!(r["results"][0]["number"], "07 9200");
        let p = a.run("ui.spec.libraryPreview", json!({"file": "079200.docx"})).unwrap();
        assert_eq!(p["paras"][0]["k"], "title");
        a.run("ui.spec.libraryTag", json!({"file": "079200.docx", "tags": ["exterior"], "approved": true})).unwrap();
        assert_eq!(a.speccraft.library.tags, "exterior");
        let r = a.run("ui.spec.libraryAdd", json!({"file": "079200.docx"})).unwrap();
        assert_eq!(r["added"], "079200.docx");
        assert!(a.run("ui.spec.libraryAdd", json!({"file": "079200.docx"})).is_err(), "the project already has it");
        // A closed and reopened project brings its library back.
        a.speccraft.library = Default::default();
        a.run("ui.spec.openProject", json!({"path": project.to_string_lossy()})).unwrap();
        assert_eq!(a.speccraft.library.folder.as_deref(), Some(lib.as_path()));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
