//! Sections pane: the project's header and footer (M21), print options (M22) and reference
//! standards (M20), and keeping the section on screen in line with the project (M20 sync).

use egui::{RichText, Ui};
use serde_json::{Value, json};

use crate::WordApp;
use crate::theme::Tokens;

/// The header and footer being edited: (header left, header right, footer left, footer right),
/// one line per text line.
#[derive(Clone, Debug, Default)]
pub(crate) struct FurnitureForm {
    pub lines: [String; 4],
}

/// Bring the section on screen into line with the open project (terms, references, units,
/// Auto-Select New, header and footer). Quiet: a section outside the project is left alone.
pub(crate) fn sync_open(app: &mut WordApp) {
    let Some(project) = app.speccraft.project.clone() else { return };
    let Some(path) = app.session.path.clone() else { return };
    if path.parent() != project.parent() {
        return;
    }
    if let Err(e) = app.run("spec.project.sync", json!({"path": project.to_string_lossy()})) {
        log::warn!("speccraft: couldn't bring the section up to date with the project: {e}");
    }
}

/// Change project settings, then refresh the pane's copy and the section on screen.
pub(crate) fn change(app: &mut WordApp, changes: Value) -> Result<(), String> {
    let path = super::project_path(app)?;
    let mut args = changes;
    args["path"] = json!(path);
    app.run("spec.project.settings", args)?;
    let info = app.run("spec.project.info", json!({"path": path}))?;
    app.speccraft.info = Some(info);
    sync_open(app);
    Ok(())
}

/// The parts of the pane.
pub(crate) fn show(app: &mut WordApp, ui: &mut Ui, info: &Value) {
    attachments(app, ui, info);
    furniture(app, ui, info);
    print_options(app, ui, info);
    standards(app, ui);
}

fn lines_of(v: &Value, side: &str) -> String {
    v.get(side).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n")).unwrap_or_default()
}

/// Attached PDFs (M23): which section each goes with; remove one.
fn attachments(app: &mut WordApp, ui: &mut Ui, info: &Value) {
    let t = Tokens::get(ui.ctx());
    let list = info.get("attachments").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut action: Option<Value> = None;
    let head = crate::i18n::fmt(tl!("Attached PDFs ({n})"), &[("n", &list.len().to_string())]);
    egui::CollapsingHeader::new(RichText::new(head).small()).id_salt("spec_attachments").default_open(false).show(ui, |ui| {
        if list.is_empty() {
            ui.label(RichText::new(tl!("None. Right-click a section › Attach PDF… to add a form, appendix or schedule.")).small().color(t.text_dim));
        }
        let folder = app.speccraft.project.as_ref().and_then(|p| p.parent().map(std::path::Path::to_path_buf)).unwrap_or_default();
        for a in &list {
            let s = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            let file = s("file");
            let name = std::path::Path::new(&file).file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let missing = !folder.join(&file).is_file();
            let section = info
                .get("sections")
                .and_then(Value::as_array)
                .and_then(|x| x.iter().find(|r| r.get("file").and_then(Value::as_str) == Some(s("section").as_str())))
                .and_then(|r| r.get("number").and_then(Value::as_str))
                .unwrap_or("")
                .to_string();
            ui.horizontal_wrapped(|ui| {
                let color = if missing { t.red } else { t.text };
                ui.label(RichText::new(&name).small().strong().color(color));
                let pages = a.get("pages").and_then(Value::as_u64).unwrap_or(0).to_string();
                let detail = crate::i18n::fmt(tl!("{section}, {pages} pages"), &[("section", &section), ("pages", &pages)]);
                ui.label(RichText::new(detail).small().color(t.text_dim));
                if missing {
                    ui.label(RichText::new(tl!("missing")).small().color(t.red));
                }
                let r = ui.small_button(tl!("Remove")).on_hover_text(tl!("Take it off the section; the PDF stays in the Attachments folder"));
                if r.clicked() {
                    action = Some(json!({"file": file}));
                }
            });
            let title = s("title");
            if !title.is_empty() {
                ui.label(RichText::new(title).small().color(t.text_dim));
            }
        }
    });
    if let Some(args) = action
        && let Err(e) = app.run("ui.spec.detach", args)
    {
        app.status(e);
    }
}

/// Header and Footer: lines on the left and right, with {fields}.
fn furniture(app: &mut WordApp, ui: &mut Ui, info: &Value) {
    let t = Tokens::get(ui.ctx());
    let header = egui::CollapsingHeader::new(RichText::new(tl!("Header and Footer")).small()).id_salt("spec_furniture").default_open(false);
    let shown = header.show(ui, |ui| {
        if app.speccraft.furniture_form.is_none() {
            let (h, f) = (info.get("header").cloned().unwrap_or(Value::Null), info.get("footer").cloned().unwrap_or(Value::Null));
            app.speccraft.furniture_form = Some(FurnitureForm { lines: [lines_of(&h, "left"), lines_of(&h, "right"), lines_of(&f, "left"), lines_of(&f, "right")] });
        }
        ui.label(
            RichText::new(tl!("Every section gets these instead of its own. Fields: {Section Name} {Section ID} {Page} {Pages} and any project value, e.g. {Project ID}."))
                .small()
                .color(t.text_dim),
        );
        let Some(form) = app.speccraft.furniture_form.as_mut() else { return };
        let [hl, hr, fl, fr] = &mut form.lines;
        for (label, l, r) in [(tl!("Header"), &mut *hl, &mut *hr), (tl!("Footer"), &mut *fl, &mut *fr)] {
            ui.label(RichText::new(label).small().strong());
            ui.horizontal(|ui| {
                let w = (ui.available_width() / 2.0 - 6.0).max(80.0);
                ui.add(egui::TextEdit::multiline(l).hint_text(tl!("Left")).desired_rows(2).desired_width(w));
                ui.add(egui::TextEdit::multiline(r).hint_text(tl!("Right")).desired_rows(2).desired_width(w));
            });
        }
        let mut save = false;
        let mut sample = false;
        ui.horizontal(|ui| {
            save = ui.small_button(tl!("Save")).clicked();
            sample = ui.small_button(tl!("Use a Sample Footer")).on_hover_text(tl!("Project ID, revision and date on the left; section name, number and pages on the right")).clicked();
        });
        if sample {
            *fl = "Project I.D. {Project ID}\nRev. {Rev No}, {Deliverable Date}".into();
            *fr = "{Section Name}\n{Section ID} - {Page} of {Pages}".into();
        }
        if save {
            let side = |s: &str| -> Value { json!(s.lines().map(str::trim_end).filter(|l| !l.is_empty()).collect::<Vec<_>>()) };
            let block = |l: &str, r: &str| if l.trim().is_empty() && r.trim().is_empty() { Value::Null } else { json!({"left": side(l), "right": side(r)}) };
            let changes = json!({"header": block(hl, hr), "footer": block(fl, fr)});
            app.speccraft.furniture_form = None;
            match change(app, changes) {
                Ok(()) => app.status(tl!("Header and footer saved; sections take them when opened and published")),
                Err(e) => app.status(e),
            }
        }
    });
    if shown.body_returned.is_none() {
        app.speccraft.furniture_form = None;
    }
}

type Changes = serde_json::Map<String, Value>;

/// A print option shown as a check box.
fn check(ui: &mut Ui, print: &Value, changes: &mut Changes, key: &str, default: bool, label: &str, hint: &str) {
    let mut v = print.get(key).and_then(Value::as_bool).unwrap_or(default);
    if ui.checkbox(&mut v, RichText::new(tl!(label)).small()).on_hover_text(tl!(hint)).changed() {
        changes.insert(key.into(), json!(v));
    }
}

/// A print option chosen from a list.
fn choose(ui: &mut Ui, print: &Value, changes: &mut Changes, key: &str, label: &str, opts: &[(&str, &str)]) {
    let now = print.get(key).and_then(Value::as_str).or_else(|| opts.first().map(|(k, _)| *k)).unwrap_or("").to_string();
    ui.horizontal(|ui| {
        ui.label(RichText::new(tl!(label)).small());
        let shown = opts.iter().find(|(k, _)| *k == now).map_or(now.clone(), |(_, l)| tl!(l).to_string());
        craft_ui::ComboBox::from_id_salt(("spec_print", key)).selected_text(shown).show_ui(ui, |ui| {
            for (k, l) in opts {
                if ui.selectable_label(now == *k, tl!(l)).clicked() {
                    changes.insert(key.into(), json!(k));
                }
            }
        });
    });
}

/// Print Options: saved to the project as they change.
fn print_options(app: &mut WordApp, ui: &mut Ui, info: &Value) {
    let t = Tokens::get(ui.ctx());
    let print = info.get("print").cloned().unwrap_or(Value::Null);
    let mut changes = Changes::new();
    let force = std::mem::take(&mut app.speccraft.show_print_options).then_some(true);
    egui::CollapsingHeader::new(RichText::new(tl!("Print Options")).small()).id_salt("spec_print_options").default_open(false).open(force).show(
        ui,
        |ui| {
            let c = &mut changes;
            check(ui, &print, c, "draft", false, "Draft", "Author notes print too, under a watermark; files are marked (Draft)");
            check(
                ui,
                &print,
                c,
                "showAllChoices",
                false,
                "Show all choice options",
                "Every choice prints all its options, the chosen ones underlined, for review",
            );
            check(
                ui,
                &print,
                c,
                "keepWithNext",
                true,
                "Keep headings with what follows",
                "Titles, Parts and Articles never end a page; END OF SECTION is never alone",
            );
            check(
                ui,
                &print,
                c,
                "widowControl",
                true,
                "Widow and orphan control",
                "No single line of a paragraph alone at the top or bottom of a page",
            );
            check(ui, &print, c, "hyperlinks", true, "Keep hyperlinks", "Off: links print as plain text");
            check(ui, &print, c, "contents", true, "Manual: table of contents", "The project manual starts with a table of contents");
            check(
                ui,
                &print,
                c,
                "oddPages",
                false,
                "Manual: each section on an odd page",
                "A blank page goes before a section that would start on an even page",
            );
            if print.get("oddPages").and_then(Value::as_bool).unwrap_or(false) {
                ui.indent("spec_blank", |ui| {
                    check(ui, &print, c, "blankPageFurniture", true, "Header and footer on blank pages", "Off: blank pages have no header or footer");
                    let mut text = print.get("blankPageText").and_then(Value::as_str).unwrap_or("").to_string();
                    let resp = ui.add(egui::TextEdit::singleline(&mut text).desired_width(ui.available_width() - 8.0));
                    if resp.lost_focus() {
                        c.insert("blankPageText".into(), json!(text));
                    }
                });
            }
            choose(ui, &print, c, "bookmarks", "PDF bookmarks", &[("sections", "Sections"), ("parts", "Sections and Parts"), ("none", "None")]);
            choose(
                ui,
                &print,
                c,
                "endOfSection",
                "END OF SECTION",
                &[("asis", "As written"), ("id", "With the number"), ("plain", "Without the number")],
            );
            choose(
                ui,
                &print,
                c,
                "fileNames",
                "File names",
                &[("file", "The section file's"), ("number", "Number"), ("numberTitle", "Number and title")],
            );
            choose(ui, &print, c, "pageNumbers", "Manual page numbers", &[("restart", "From 1 in each section"), ("continue", "Running on")]);
            choose(ui, &print, c, "markup", "Review markup", &[("none", "None"), ("all", "All sections"), ("tracking", "Sections tracking changes")]);
            choose(
                ui,
                &print,
                c,
                "attachments",
                "Attached PDFs",
                &[("after", "After their section"), ("end", "At the end of the manual"), ("none", "Left out")],
            );
            ui.label(RichText::new(tl!("Publish and Project Manual use these.")).small().color(t.text_dim));
        },
    );
    if !changes.is_empty()
        && let Err(e) = change(app, json!({"print": changes}))
    {
        app.status(e);
    }
}

/// Reference Standards: what the sections cite, with the project's titles and editions.
fn standards(app: &mut WordApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let mut save: Option<Value> = None;
    let mut refresh = false;
    egui::CollapsingHeader::new(RichText::new(tl!("Reference Standards")).small()).id_salt("spec_standards").default_open(false).show(ui, |ui| {
        refresh = ui.small_button(tl!("Read the Sections")).on_hover_text(tl!("List the standards the included sections cite")).clicked();
        let Some(report) = app.speccraft.standards.clone() else {
            ui.label(RichText::new(tl!("Not read yet.")).small().color(t.text_dim));
            return;
        };
        let rows = report.get("standards").and_then(Value::as_array).cloned().unwrap_or_default();
        if rows.is_empty() {
            ui.label(RichText::new(tl!("No standards cited.")).small().color(t.text_dim));
        }
        for r in &rows {
            let s = |k: &str| r.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            let key = s("key");
            let sections: Vec<String> = r
                .get("sections")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default();
            let cited = !sections.is_empty();
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&key).small().strong().color(if cited { t.green } else { t.text_disabled }));
                let where_ = if cited { sections.join(", ") } else { tl!("not cited in a printing paragraph").to_string() };
                ui.label(RichText::new(where_).small().color(t.text_dim));
            });
            let draft = app.speccraft.standard_drafts.entry(key.clone()).or_insert_with(|| (s("title"), s("edition")));
            ui.horizontal(|ui| {
                let w = (ui.available_width() - 70.0).max(80.0);
                let a = ui.add(egui::TextEdit::singleline(&mut draft.0).hint_text(tl!("Title")).desired_width(w));
                let b = ui.add(egui::TextEdit::singleline(&mut draft.1).hint_text(tl!("Edition")).desired_width(60.0));
                if (a.lost_focus() || b.lost_focus()) && (draft.0 != s("title") || draft.1 != s("edition")) {
                    save = Some(json!({"key": key, "title": draft.0, "edition": draft.1}));
                }
            });
        }
    });
    if refresh || save.is_some() {
        let Ok(path) = super::project_path(app) else { return };
        if let Some(mut args) = save {
            args["path"] = json!(path);
            if let Err(e) = app.run("spec.project.setStandard", args) {
                app.status(e);
            }
        }
        if refresh {
            // Sections on screen are read from their files: save them first.
            if let Err(e) = super::save_if_changed(app) {
                app.status(e);
            }
            app.speccraft.standard_drafts.clear();
        }
        match app.run("spec.project.standards", json!({"path": path})) {
            Ok(r) => app.speccraft.standards = Some(r),
            Err(e) => app.status(e),
        }
    }
}
