//! Spec Outline pane views added in M26, so what SpecLink keeps in its two side tabs is in one
//! place: Paragraph (the paragraph at the caret: notes from the master, the author's project
//! note, tags, why it is on or off), Terms (the section's global terms, edited for the project),
//! Standards (what the section cites) and the review round's changes (shown in Review).

use egui::{RichText, Ui};
use serde_json::{Value, json};

use crate::WordApp;
use crate::theme::Tokens;

/// The section's outline rows, for finding a paragraph's block and sequence number.
fn outline_rows(app: &mut WordApp) -> Vec<Value> {
    app.session.run("spec.outline", &json!({})).ok().and_then(|o| o.get("items").and_then(Value::as_array).cloned()).unwrap_or_default()
}

/// Put the caret at the start of paragraph `id`.
pub(crate) fn go_to(app: &mut WordApp, id: &str) {
    let rows = outline_rows(app);
    let Some(block) = rows.iter().find(|r| r.get("id").and_then(Value::as_str) == Some(id)).and_then(|r| r.get("block")).and_then(Value::as_u64)
    else {
        return;
    };
    let pos = json!({"story": "body", "path": [block], "off": 0});
    if app.run("caret.set", json!({"pos": pos})).is_ok() {
        app.canvas.scroll_to_caret = true;
    }
}

/// Paragraph: everything about the paragraph at the caret.
pub(crate) fn paragraph(app: &mut WordApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let info = match app.session.run("spec.paragraph.info", &json!({})) {
        Ok(i) => i,
        Err(_) => {
            ui.label(RichText::new(tl!("Put the caret in a spec paragraph to see it here.")).small().color(t.text_dim));
            return;
        }
    };
    let id = info.get("id").and_then(Value::as_str).unwrap_or("").to_string();
    let seq = info.get("seq").and_then(Value::as_u64).unwrap_or(0);
    ui.label(RichText::new(format!("{seq:04} · {id}")).monospace().small().color(t.text_dim));
    egui::ScrollArea::vertical().id_salt("spec_paragraph_view").auto_shrink([false, false]).show(ui, |ui| {
        let notes: Vec<String> = info
            .get("masterNotes")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default();
        ui.label(RichText::new(tl!("Notes from the master")).small().strong());
        if notes.is_empty() {
            ui.label(RichText::new(tl!("None.")).small().color(t.text_dim));
        }
        for n in &notes {
            ui.label(RichText::new(n).small().italics().color(t.blue));
        }
        ui.add_space(6.0);
        ui.label(RichText::new(tl!("Project note")).small().strong())
            .on_hover_text(tl!("Yours, for this project: not in the text, printed only in drafts"));
        let saved_note = info.get("projectNote").and_then(Value::as_str).unwrap_or("").to_string();
        let saved_tags: Vec<String> =
            info.get("tags").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
        // Drafts follow the paragraph at the caret.
        if app.speccraft.para_form.as_ref().map(|(i, _, _)| i.as_str()) != Some(id.as_str()) {
            app.speccraft.para_form = Some((id.clone(), saved_note.clone(), saved_tags.join(", ")));
        }
        let mut save_note = None;
        let mut save_tags = None;
        if let Some((_, note, tags)) = app.speccraft.para_form.as_mut() {
            let r = ui.add(egui::TextEdit::multiline(note).desired_rows(3).desired_width(f32::INFINITY).hint_text(tl!("A note for this project")));
            if r.lost_focus() && *note != saved_note {
                save_note = Some(note.clone());
            }
            ui.add_space(6.0);
            ui.label(RichText::new(tl!("Tags")).small().strong()).on_hover_text(tl!("Words to find paragraphs by, separated by commas"));
            let r = ui.add(egui::TextEdit::singleline(tags).desired_width(f32::INFINITY).hint_text(tl!("e.g. coordination, owner review")));
            if r.lost_focus() && *tags != saved_tags.join(", ") {
                save_tags = Some(tags.split(',').map(str::trim).filter(|x| !x.is_empty()).map(str::to_string).collect::<Vec<_>>());
            }
        }
        ui.add_space(6.0);
        ui.label(RichText::new(tl!("Why it is on or off")).small().strong());
        for r in info.get("reasons").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
            ui.label(RichText::new(format!("• {r}")).small().color(t.text_dim));
        }
        if let Some(text) = save_note
            && let Err(e) = app.run("spec.paragraph.note", json!({"id": id, "text": text}))
        {
            app.status(e);
        }
        if let Some(list) = save_tags
            && let Err(e) = app.run("spec.paragraph.tags", json!({"id": id, "tags": list}))
        {
            app.status(e);
        }
        // Paragraphs with tags, to jump between them.
        if let Ok(all) = app.session.run("spec.tags.list", &json!({}))
            && let Some(map) = all.get("tags").and_then(Value::as_object)
            && !map.is_empty()
        {
            ui.add_space(8.0);
            ui.separator();
            ui.label(RichText::new(tl!("Tagged paragraphs")).small().strong());
            let rows = outline_rows(app);
            let mut go = None;
            for (tag, ids) in map {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(tag).small().color(t.accent));
                    for pid in ids.as_array().into_iter().flatten().filter_map(Value::as_str) {
                        let seq = rows.iter().position(|r| r.get("id").and_then(Value::as_str) == Some(pid)).unwrap_or(0);
                        if ui.link(RichText::new(format!("{seq:04}")).small().monospace()).clicked() {
                            go = Some(pid.to_string());
                        }
                    }
                });
            }
            if let Some(pid) = go {
                go_to(app, &pid);
            }
        }
    });
}

/// Terms: the section's global terms; a value changed here changes it for the whole project.
pub(crate) fn terms(app: &mut WordApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let list = app.session.run("spec.terms.list", &json!({})).unwrap_or(json!({}));
    let rows: Vec<Value> = list.get("terms").and_then(Value::as_array).cloned().unwrap_or_default();
    let values = app.speccraft.info.as_ref().and_then(|i| i.get("values")).cloned().unwrap_or(json!({}));
    ui.label(
        RichText::new(tl!("Global terms in this section. A value set here is the project's: every section shows it.")).small().color(t.text_dim),
    );
    if app.speccraft.project.is_none() {
        ui.label(RichText::new(tl!("Open the section's project to set values.")).small().color(t.orange));
    }
    ui.add(egui::TextEdit::singleline(&mut app.speccraft.term_search).hint_text(tl!("Search terms")).desired_width(f32::INFINITY));
    // Each name once, with how often it is used.
    let mut names: Vec<(String, usize, String)> = Vec::new();
    for r in &rows {
        let name = r.get("name").and_then(Value::as_str).unwrap_or("").to_string();
        let original = r.get("original").and_then(Value::as_str).unwrap_or("").to_string();
        match names.iter_mut().find(|(n, _, _)| n.eq_ignore_ascii_case(&name)) {
            Some(x) => x.1 += 1,
            None => names.push((name, 1, original)),
        }
    }
    // The project's other values too: they are terms as soon as a section uses them.
    if let Some(m) = values.as_object() {
        for k in m.keys() {
            if !names.iter().any(|(n, _, _)| n.eq_ignore_ascii_case(k)) {
                names.push((k.clone(), 0, String::new()));
            }
        }
    }
    let needle = app.speccraft.term_search.trim().to_lowercase();
    let mut save: Option<(String, String)> = None;
    egui::ScrollArea::vertical().id_salt("spec_terms_view").auto_shrink([false, false]).show(ui, |ui| {
        if names.is_empty() {
            ui.label(
                RichText::new(tl!("No global terms: the master has no <Insert …> placeholders with a project value.")).small().color(t.text_dim),
            );
        }
        for (name, count, _) in names.iter().filter(|(n, _, _)| needle.is_empty() || n.to_lowercase().contains(&needle)) {
            let current = values
                .as_object()
                .and_then(|m| m.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)))
                .and_then(|(_, v)| v.as_str())
                .unwrap_or("")
                .to_string();
            ui.horizontal(|ui| {
                ui.label(RichText::new(name).small().color(TERM));
                if *count > 0 {
                    ui.label(RichText::new(format!("×{count}")).small().color(t.text_dim));
                }
            });
            let draft = app.speccraft.term_drafts.entry(name.clone()).or_insert_with(|| current.clone());
            let r = ui.add(egui::TextEdit::singleline(draft).desired_width(f32::INFINITY).hint_text(tl!("No value: the placeholder shows")));
            if r.lost_focus() && *draft != current {
                save = Some((name.clone(), draft.clone()));
            }
        }
    });
    if let Some((name, value)) = save {
        let Some(project) = app.speccraft.project.clone() else { return };
        let v = if value.trim().is_empty() { Value::Null } else { json!(value.trim()) };
        let r = app.run("spec.project.set", json!({"path": project.to_string_lossy(), "values": { name: v }}));
        match r {
            Ok(info) => {
                app.speccraft.info = Some(info);
                app.speccraft.term_drafts.clear();
                super::project_settings::sync_open(app);
            }
            Err(e) => app.status(e),
        }
    }
}

/// The purple of global terms, as on the page.
const TERM: egui::Color32 = egui::Color32::from_rgb(132, 72, 178);

/// Standards: what the section cites, with the project's titles and editions.
pub(crate) fn standards(app: &mut WordApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let list = app.session.run("spec.standards.list", &json!({})).unwrap_or(json!({}));
    let rows: Vec<Value> = list.get("standards").and_then(Value::as_array).cloned().unwrap_or_default();
    let known = app.speccraft.info.as_ref().and_then(|i| i.get("standards")).cloned().unwrap_or(json!({}));
    let all = outline_rows(app);
    let mut go = None;
    egui::ScrollArea::vertical().id_salt("spec_standards_view").auto_shrink([false, false]).show(ui, |ui| {
        if rows.is_empty() {
            ui.label(RichText::new(tl!("This section cites no reference standards.")).small().color(t.text_dim));
        }
        for r in &rows {
            let key = r.get("key").and_then(Value::as_str).unwrap_or("");
            let prints = r.get("prints").and_then(Value::as_bool).unwrap_or(false);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(key).small().strong().color(if prints { t.green } else { t.text_disabled }));
                for pid in r.get("paragraphs").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                    let seq = all.iter().position(|x| x.get("id").and_then(Value::as_str) == Some(pid)).unwrap_or(0);
                    if ui.link(RichText::new(format!("{seq:04}")).small().monospace()).clicked() {
                        go = Some(pid.to_string());
                    }
                }
            });
            let info = known.get(key);
            let title = info.and_then(|i| i.get("title")).and_then(Value::as_str).unwrap_or("");
            let edition = info.and_then(|i| i.get("edition")).and_then(Value::as_str).unwrap_or("");
            if !title.is_empty() || !edition.is_empty() {
                ui.label(RichText::new(format!("{title} {edition}")).small().color(t.text_dim));
            }
        }
        ui.add_space(6.0);
        ui.label(RichText::new(tl!("Titles and editions: Sections pane › Reference Standards.")).small().color(t.text_dim));
    });
    if let Some(pid) = go {
        go_to(app, &pid);
    }
}

/// This review round (M24): when it began, paragraphs switched since, wording changes.
pub(crate) fn round(app: &mut WordApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let c = app.session.run("spec.review.changes", &json!({})).unwrap_or(json!({}));
    let Some(since) = c.get("since").and_then(Value::as_str) else {
        ui.label(RichText::new(tl!("No review round. Specs › Track Changes starts one.")).small().color(t.text_dim));
        return;
    };
    let tracking = c.get("tracking").and_then(Value::as_bool).unwrap_or(false);
    let head = crate::i18n::fmt(
        if tracking { tl!("Review round since {date}, tracking") } else { tl!("Review round since {date}, not tracking") },
        &[("date", since)],
    );
    ui.label(RichText::new(head).small().strong());
    let all = outline_rows(app);
    let mut go = None;
    for (key, label) in [("switchedOn", tl!("Switched on")), ("switchedOff", tl!("Switched off"))] {
        let ids: Vec<String> =
            c.get(key).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
        if ids.is_empty() {
            continue;
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(label).small().color(t.blue));
            for pid in &ids {
                let seq = all.iter().position(|x| x.get("id").and_then(Value::as_str) == Some(pid.as_str())).unwrap_or(0);
                if ui.link(RichText::new(format!("{seq:04}")).small().monospace()).clicked() {
                    go = Some(pid.clone());
                }
            }
        });
    }
    let n = c.get("revisions").and_then(Value::as_u64).unwrap_or(0).to_string();
    ui.label(RichText::new(crate::i18n::fmt(tl!("{n} wording changes tracked"), &[("n", &n)])).small().color(t.text_dim));
    if let Some(pid) = go {
        go_to(app, &pid);
    }
    ui.separator();
}
