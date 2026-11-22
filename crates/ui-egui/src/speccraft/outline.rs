//! The Spec Outline pane: the section's paragraphs as a tree with their state, a filter and a
//! search, details of the paragraph at the caret, and a Printed view that shows the pages exactly
//! as they will publish (only what prints, placeholders filled), read-only.

use egui::{Color32, RichText, TextureHandle, Ui};
use serde_json::{Value, json};
use wordcraft_engine::cmd::spec::sections::State;

use crate::WordApp;
use crate::theme::{Tokens, regular, semibold};

/// Which paragraphs the outline lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Filter {
    #[default]
    All,
    Printing,
    Off,
    Notes,
    /// Reworded or added in this project.
    Changed,
}

impl Filter {
    fn label(self) -> &'static str {
        match self {
            Filter::All => "All paragraphs",
            Filter::Printing => "Printing",
            Filter::Off => "Off and refused",
            Filter::Notes => "Notes",
            Filter::Changed => "Changed from master",
        }
    }
    fn keeps(self, state: &str, origin: &str) -> bool {
        match self {
            Filter::Changed => origin == "modified" || origin == "added",
            Filter::All => true,
            Filter::Printing => state == "on" || state == "linked",
            Filter::Off => state == "off" || state == "refused" || state == "overridden",
            Filter::Notes => state == "note",
        }
    }
}

/// The pane's two views.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum View {
    #[default]
    Outline,
    Fields,
    Rules,
    Checks,
    Updates,
    Printed,
    /// Review comments (M25) and the review round (M24).
    Comments,
    /// The paragraph at the caret: notes, project note, tags, why (M26).
    Paragraph,
    /// The section's global terms (M26).
    Terms,
    /// The section's reference standards (M26).
    Standards,
}

/// Rendered pages of the printed copy, for one document revision.
pub(crate) struct Printed {
    rev: u64,
    width: u32,
    /// The form shown (M16).
    form: wordcraft_engine::cmd::spec::forms::Form,
    pub pages: Vec<(TextureHandle, egui::Vec2)>,
    pub note: Option<String>,
}

/// `spec.outline` for the document on screen, cached per revision.
pub(crate) fn outline(app: &mut WordApp) -> Value {
    let key = (app.session.document_id(), app.session.rev());
    if let Some((k, v)) = &app.speccraft.outline_cache
        && *k == key
    {
        return v.clone();
    }
    let v = app.session.run("spec.outline", &json!({})).unwrap_or(Value::Null);
    app.speccraft.outline_cache = Some((key, v.clone()));
    v
}

fn state_of(name: &str) -> State {
    match name {
        "on" => State::On,
        "linked" => State::Linked,
        "overridden" => State::Overridden,
        "refused" => State::Refused,
        "note" => State::Note,
        _ => State::Off,
    }
}

/// Move the caret to the start of top-level body paragraph `block` and bring it into view.
pub(crate) fn go_to(app: &mut WordApp, block: u64) {
    let pos = wordcraft_doc::Pos::new(wordcraft_doc::StoryRef::Body, wordcraft_doc::Path(vec![block as u32]), 0);
    if app.run("caret.set", json!({"pos": wordcraft_engine::cmd::pos_json(&pos)})).is_ok() {
        app.canvas.scroll_to_caret = true;
    }
}

/// The Spec Outline pane.
pub(crate) fn pane(app: &mut WordApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal_wrapped(|ui| {
        for (v, label) in [
            (View::Outline, "Outline"),
            (View::Paragraph, "Paragraph"),
            (View::Fields, "Fill-ins"),
            (View::Terms, "Terms"),
            (View::Standards, "Standards"),
            (View::Rules, "Rules"),
            (View::Checks, "Checks"),
            (View::Updates, "Updates"),
            (View::Printed, "Printed"),
            (View::Comments, "Review"),
        ] {
            if ui.selectable_label(app.speccraft.view == v, tl!(label)).clicked() {
                app.speccraft.view = v;
            }
        }
    });
    ui.separator();
    if app.speccraft.view == View::Printed {
        return printed(app, ui);
    }
    if app.speccraft.view == View::Fields {
        return fill_ins(app, ui);
    }
    if app.speccraft.view == View::Rules {
        return super::rules_view::pane(app, ui);
    }
    if app.speccraft.view == View::Checks {
        return super::checks::section_view(app, ui);
    }
    if app.speccraft.view == View::Updates {
        return super::updates::view(app, ui);
    }
    if app.speccraft.view == View::Comments {
        super::manager::round(app, ui);
        return super::comments_view::view(app, ui);
    }
    if app.speccraft.view == View::Paragraph {
        return super::manager::paragraph(app, ui);
    }
    if app.speccraft.view == View::Terms {
        return super::manager::terms(app, ui);
    }
    if app.speccraft.view == View::Standards {
        return super::manager::standards(app, ui);
    }
    let o = outline(app);
    let items: Vec<Value> = o.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
    if items.is_empty() {
        ui.label(RichText::new(tl!("This document has no spec paragraphs yet.")).font(regular(13.0)));
        ui.add_space(6.0);
        if ui.button(tl!("Import Paragraphs")).on_hover_text(tl!("Read each paragraph's level from its style")).clicked()
            && let Err(e) = app.run("spec.import", json!({}))
        {
            app.status(e);
        }
        return;
    }
    let count = |k: &str| o.get("counts").and_then(|c| c.get(k)).and_then(Value::as_u64).unwrap_or(0);
    ui.label(
        RichText::new(crate::i18n::fmt(
            tl!("{on} printing · {off} off · {notes} notes"),
            &[("on", &count("on").to_string()), ("off", &(count("off") + count("refused")).to_string()), ("notes", &count("note").to_string())],
        ))
        .small()
        .color(t.text_dim),
    );
    ui.horizontal(|ui| {
        craft_ui::ComboBox::from_id_salt("spec_outline_filter").selected_text(tl!(app.speccraft.filter.label())).show_ui(ui, |ui| {
            for f in [Filter::All, Filter::Printing, Filter::Off, Filter::Notes, Filter::Changed] {
                ui.selectable_value(&mut app.speccraft.filter, f, tl!(f.label()));
            }
        });
    });
    ui.add(egui::TextEdit::singleline(&mut app.speccraft.search).hint_text(tl!("Search paragraphs")).desired_width(f32::INFINITY));
    ui.add_space(4.0);
    let caret = o.get("caret").and_then(Value::as_str).map(str::to_string);
    let origin_v = origins(app);
    let origin = origin_map(&origin_v);
    let removed: Vec<Value> = origin_v.get("removed").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut restore: Option<String> = None;
    let needle = app.speccraft.search.trim().to_lowercase();
    let filter = app.speccraft.filter;
    let mut toggle: Option<String> = None;
    let mut goto: Option<u64> = None;
    // Room for the details of the paragraph at the caret (its wording and buttons).
    let detail_h = 230.0;
    let list_h = (ui.available_height() - detail_h).max(120.0);
    egui::ScrollArea::vertical().id_salt("spec_outline_list").max_height(list_h).auto_shrink([false, false]).show(ui, |ui| {
        for it in &items {
            let s = |k: &str| it.get(k).and_then(Value::as_str).unwrap_or("");
            let state = s("state");
            let org = origin.get(s("id")).map_or("master", |(o, _)| o.as_str());
            if !filter.keeps(state, org) || (!needle.is_empty() && !s("text").to_lowercase().contains(&needle)) {
                continue;
            }
            let level = it.get("level").and_then(Value::as_u64).unwrap_or(1) as f32;
            let kind = s("kind");
            let here = caret.as_deref() == Some(s("id"));
            ui.horizontal(|ui| {
                // The sequence number, as in the page's column (M17).
                let seq = it.get("seq").and_then(Value::as_u64).unwrap_or(0) as usize;
                ui.label(RichText::new(super::marks::seq_label(seq)).monospace().small().color(t.text_dim));
                ui.add_space(level.min(7.0) * 10.0);
                let st = state_of(state);
                if st == State::Note {
                    ui.label(RichText::new("//").small().color(t.text_dim)).on_hover_text(tl!(super::marks::describe(st)));
                } else {
                    let mut on = matches!(st, State::On | State::Linked);
                    let resp = ui.checkbox(&mut on, "").on_hover_text(tl!(super::marks::describe(st)));
                    if st == State::Refused {
                        let c = resp.rect.left_center() + egui::vec2(9.0, 0.0);
                        ui.painter().circle_filled(c, 3.0, t.blue);
                    }
                    if resp.clicked() {
                        toggle = Some(s("id").to_string());
                    }
                }
                let mut text =
                    RichText::new(s("text")).font(if matches!(kind, "title" | "part" | "article" | "end") { semibold(12.0) } else { regular(12.0) });
                text = match st {
                    State::On | State::Linked => text.color(t.text),
                    State::Note => text.italics().color(t.text_dim),
                    _ => text.color(t.text_disabled),
                };
                match org {
                    "modified" => {
                        ui.label(RichText::new("M*").small().color(t.orange)).on_hover_text(tl!("Reworded in this project"));
                    }
                    "added" => {
                        ui.label(RichText::new("U").small().color(t.accent)).on_hover_text(tl!("Written in this project"));
                    }
                    _ => {}
                }
                let resp = ui.add(egui::Button::selectable(here, text).frame_when_inactive(false).truncate());
                if resp.clicked() {
                    goto = it.get("block").and_then(Value::as_u64);
                }
            });
        }
    });
    if !removed.is_empty() {
        ui.separator();
        egui::CollapsingHeader::new(
            RichText::new(crate::i18n::fmt(tl!("Deleted master paragraphs ({n})"), &[("n", &removed.len().to_string())])).small(),
        )
        .id_salt("spec_removed")
        .show(ui, |ui| {
            for r in &removed {
                let id = r.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                let text: String = r.get("text").and_then(Value::as_str).unwrap_or("").chars().take(60).collect();
                ui.horizontal(|ui| {
                    if ui.small_button(tl!("Restore")).clicked() {
                        restore = Some(id.clone());
                    }
                    ui.label(RichText::new(text).small().color(t.text_dim));
                });
            }
        });
    }
    ui.separator();
    details(app, ui, &items, caret.as_deref(), &origin);
    if let Some(id) = restore
        && let Err(e) = app.run("spec.restore", json!({"id": id}))
    {
        app.status(e);
    }
    if let Some(id) = toggle {
        match app.run("spec.toggle", json!({"id": id})) {
            Ok(r) if r["state"] == "refused" => app.status(tl!("Its parent is off: it will print once the parent is switched on")),
            Ok(_) => {}
            Err(e) => app.status(e),
        }
    }
    if let Some(b) = goto {
        go_to(app, b);
    }
}

/// `spec.origin` for the document on screen, cached per revision: each paragraph's origin by
/// id, and the deleted master paragraphs.
pub(crate) fn origins(app: &mut WordApp) -> Value {
    let key = (app.session.document_id(), app.session.rev());
    if let Some((k, v)) = &app.speccraft.origin_cache
        && *k == key
    {
        return v.clone();
    }
    let v = app.session.run("spec.origin", &json!({})).unwrap_or(Value::Null);
    app.speccraft.origin_cache = Some((key, v.clone()));
    v
}

/// The origin of each paragraph, by id.
fn origin_map(o: &Value) -> std::collections::HashMap<String, (String, String)> {
    o.get("items")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|i| {
                    let id = i.get("id")?.as_str()?.to_string();
                    let origin = i.get("origin")?.as_str()?.to_string();
                    let original = i.get("original").and_then(Value::as_str).unwrap_or("").to_string();
                    Some((id, (origin, original)))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `spec.fields.list` for the document on screen, cached per revision.
pub(crate) fn field_list(app: &mut WordApp) -> Value {
    let key = (app.session.document_id(), app.session.rev());
    if let Some((k, v)) = &app.speccraft.fields_cache
        && *k == key
    {
        return v.clone();
    }
    let v = app.session.run("spec.fields.list", &json!({})).unwrap_or(Value::Null);
    app.speccraft.fields_cache = Some((key, v.clone()));
    v
}

/// The Fill-ins view: every choice, blank and fill-in, with a control to settle it.
fn fill_ins(app: &mut WordApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let list = field_list(app);
    let all: Vec<Value> = list.get("fields").and_then(Value::as_array).cloned().unwrap_or_default();
    if all.is_empty() {
        ui.label(RichText::new(tl!("No fill-ins yet.")).font(regular(13.0)));
        ui.label(
            RichText::new(tl!("Find Fill-ins turns [choices], [optional text], ___ blanks and <Insert …> into fields.")).small().color(t.text_dim),
        );
        ui.add_space(6.0);
        if ui.button(tl!("Find Fill-ins")).clicked()
            && let Err(e) = app.run("spec.fields.detect", json!({}))
        {
            app.status(e);
        }
        return;
    }
    let open = all.iter().filter(|f| f["state"] == "open").count();
    let open_printing = list.get("openPrinting").and_then(Value::as_u64).unwrap_or(0);
    ui.label(
        RichText::new(crate::i18n::fmt(
            tl!("{n} fill-ins · {open} open ({printing} in printing paragraphs)"),
            &[("n", &all.len().to_string()), ("open", &open.to_string()), ("printing", &open_printing.to_string())],
        ))
        .small()
        .color(t.text_dim),
    );
    ui.horizontal(|ui| {
        ui.checkbox(&mut app.speccraft.open_only, tl!("Open only"));
        ui.checkbox(&mut app.speccraft.printing_only, tl!("Printing only"));
    });
    if ui.small_button(tl!("Find Fill-ins")).on_hover_text(tl!("Look again for choices and blanks not yet made into fields")).clicked()
        && let Err(e) = app.run("spec.fields.detect", json!({}))
    {
        app.status(e);
    }
    ui.separator();
    let mut action: Option<Value> = None;
    let mut need_action: Option<Value> = None;
    let mut choice_action: Option<(&'static str, Value)> = None;
    let mut goto: Option<u64> = None;
    egui::ScrollArea::vertical().id_salt("spec_fill_ins").auto_shrink([false, false]).show(ui, |ui| {
        for f in &all {
            let s = |k: &str| f.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            let state = s("state");
            let prints = f.get("prints").and_then(Value::as_bool).unwrap_or(false);
            if (app.speccraft.open_only && state != "open") || (app.speccraft.printing_only && !prints) {
                continue;
            }
            let id = s("id");
            ui.horizontal(|ui| {
                let color = match state.as_str() {
                    "open" => t.orange,
                    "omitted" => t.text_disabled,
                    _ => t.text,
                };
                let name = if prints { s("name") } else { format!("{} · {}", s("name"), tl!("paragraph off")) };
                if ui
                    .add(egui::Label::new(RichText::new(name).font(semibold(12.0)).color(color)).sense(egui::Sense::click()))
                    .on_hover_text(tl!("Go to it"))
                    .clicked()
                {
                    goto = f.get("block").and_then(Value::as_u64);
                }
                if state != "open" && ui.small_button("↺").on_hover_text(tl!("Back to the master's wording")).clicked() {
                    action = Some(json!({"id": id, "reset": true}));
                }
                // W1: whether it must be filled in before issue.
                let need = s("need");
                let label = |n: &str| match n {
                    "required" => tl!("Required"),
                    "optional" => tl!("Optional"),
                    _ => tl!("Unmarked"),
                };
                craft_ui::ComboBox::from_id_salt(("spec_field_need", &id))
                    .selected_text(RichText::new(label(&need)).small().color(if need == "required" { t.red } else { t.text_dim }))
                    .width(90.0)
                    .show_ui(ui, |ui| {
                        for (n, hint) in [
                            ("required", tl!("Left open, it is an error: Publish stops")),
                            ("optional", tl!("Left open, it prints as written and isn't reported")),
                            ("default", tl!("Left open, it is a warning")),
                        ] {
                            if ui.selectable_label(need == n || (need.is_empty() && n == "default"), label(n)).on_hover_text(hint).clicked() {
                                need_action = Some(json!({"id": id, "need": n}));
                            }
                        }
                    });
            });
            ui.label(RichText::new(s("original")).small().color(t.text_dim));
            match s("kind").as_str() {
                "choice" | "optional" => {
                    if let Some(c) = super::choices::Choice::from_json(f) {
                        let fill = app.speccraft.drafts.entry(id.clone()).or_default();
                        egui::Frame::new().inner_margin(egui::Margin::symmetric(6, 2)).show(ui, |ui| {
                            if let Some(a) = super::choices::list(ui, &t, &c, fill) {
                                choice_action = Some(a);
                            }
                        });
                    }
                }
                _ => {
                    let draft = app.speccraft.drafts.entry(id.clone()).or_insert_with(|| if state == "open" { String::new() } else { s("value") });
                    let hint = if s("kind") == "date" { tl!("e.g. December 1, 2026") } else { tl!("Type the value") };
                    let resp = ui.add(egui::TextEdit::singleline(draft).hint_text(hint).desired_width(f32::INFINITY));
                    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if (enter || resp.lost_focus()) && !draft.trim().is_empty() && draft.trim() != s("value") {
                        action = Some(json!({"id": id, "value": draft.trim()}));
                    }
                }
            }
            ui.add_space(6.0);
        }
    });
    if let Some((cmd, params)) = choice_action {
        if let Some(id) = params.get("id").and_then(Value::as_str)
            && params.get("fill").is_some()
        {
            app.speccraft.drafts.remove(id);
        }
        if let Err(e) = app.run(cmd, params) {
            app.status(e);
        }
    }
    if let Some(params) = need_action
        && let Err(e) = app.run("spec.fields.need", params)
    {
        app.status(e);
    }
    if let Some(params) = action {
        if let Some(id) = params.get("id").and_then(Value::as_str) {
            app.speccraft.drafts.remove(id);
        }
        if let Err(e) = app.run("spec.fields.set", params) {
            app.status(e);
        }
    }
    if let Some(b) = goto {
        go_to(app, b);
    }
}

/// What the paragraph at the caret is.
fn details(app: &mut WordApp, ui: &mut Ui, items: &[Value], caret: Option<&str>, origin: &std::collections::HashMap<String, (String, String)>) {
    let t = Tokens::get(ui.ctx());
    let Some(it) = caret.and_then(|c| items.iter().find(|i| i["id"] == c)) else {
        ui.label(RichText::new(tl!("Put the caret in a spec paragraph to see it here.")).small().color(t.text_dim));
        return;
    };
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let kind = match s(it, "kind").as_str() {
        "title" => tl!("Section title"),
        "part" => tl!("Part"),
        "article" => tl!("Article"),
        "para" => tl!("Paragraph"),
        "end" => tl!("End of section"),
        "note" => tl!("Author note"),
        _ => tl!("Paragraph without a spec style"),
    };
    let level = it.get("level").and_then(Value::as_u64).map(|l| crate::i18n::fmt(tl!(", level {n}"), &[("n", &l.to_string())])).unwrap_or_default();
    ui.label(RichText::new(format!("{kind}{level}")).font(semibold(12.5)));
    let state = state_of(&s(it, "state"));
    ui.label(RichText::new(tl!(super::marks::describe(state))).small());
    // Why, in the rules' words (the engine's sentences, as for an agent).
    let why_key = (app.session.document_id(), app.session.rev(), s(it, "id"));
    let reasons = match &app.speccraft.why_cache {
        Some((k, v)) if *k == why_key => v.clone(),
        _ => {
            let v: Vec<String> = app
                .session
                .run("spec.why", &json!({"id": s(it, "id")}))
                .ok()
                .and_then(|r| r.get("reasons").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()))
                .unwrap_or_default();
            app.speccraft.why_cache = Some((why_key, v.clone()));
            v
        }
    };
    if reasons.iter().any(|r| r.contains("Rule ") || r.contains("rule")) {
        for r in &reasons {
            ui.label(RichText::new(format!("• {r}")).small().color(t.text_dim));
        }
    }
    let parent = it.get("parent").and_then(Value::as_str).and_then(|p| items.iter().find(|i| i["id"] == p));
    if let Some(p) = parent {
        let text: String = s(p, "text").chars().take(60).collect();
        let pstate = state_of(&s(p, "state"));
        let line = crate::i18n::fmt(tl!("Under: {text}"), &[("text", &text)]);
        let color = if matches!(pstate, State::On | State::Linked) { t.text_dim } else { t.orange };
        ui.label(RichText::new(line).small().color(color));
    }
    let id = s(it, "id");
    let children = items.iter().filter(|i| i.get("parent").and_then(Value::as_str) == Some(id.as_str())).count();
    if children > 0 {
        ui.label(RichText::new(crate::i18n::fmt(tl!("{n} paragraphs under it"), &[("n", &children.to_string())])).small().color(t.text_dim));
    }
    let (org, original) = origin.get(&id).cloned().unwrap_or_else(|| ("master".into(), String::new()));
    let org_text = match org.as_str() {
        "modified" => tl!("Reworded in this project (M*)"),
        "added" => tl!("Written in this project (U)"),
        _ => tl!("Master wording"),
    };
    ui.label(RichText::new(format!("{org_text} · {id}")).small().color(t.text_disabled));
    if org == "modified" && !original.is_empty() {
        ui.label(RichText::new(tl!("The master says:")).small().color(t.text_dim));
        ui.label(RichText::new(&original).small().italics());
    }
    let mut cmd: Option<(&str, Value)> = None;
    ui.horizontal_wrapped(|ui| {
        if state != State::Note && ui.small_button(tl!("Switch On or Off")).clicked() {
            cmd = Some(("spec.toggle", json!({})));
        }
        if org == "modified" && ui.small_button(tl!("Restore Master Wording")).clicked() {
            cmd = Some(("spec.restore", json!({})));
        }
        if ui.small_button(tl!("Duplicate")).clicked() {
            cmd = Some(("spec.duplicate", json!({})));
        }
        if ui.small_button(tl!("Add After")).clicked() {
            cmd = Some(("spec.addParagraph", json!({})));
        }
        if ui.small_button(tl!("Add Under")).clicked() {
            cmd = Some(("spec.addParagraph", json!({"child": true})));
        }
        if org == "added" && ui.small_button(tl!("Remove")).clicked() {
            cmd = Some(("spec.removeParagraph", json!({})));
        }
    });
    if !matches!(s(it, "kind").as_str(), "title" | "end" | "note") {
        short_form(app, ui, &t, &id);
    }
    if let Some((id, params)) = cmd
        && let Err(e) = app.run(id, params)
    {
        app.status(e);
    }
}

/// Full, short or outline form (M16), with the outline's depth.
pub(crate) fn form_choice(ui: &mut Ui, salt: &str, form: &mut wordcraft_engine::cmd::spec::forms::Form) {
    use wordcraft_engine::cmd::spec::forms::Form;
    let label = |f: Form| match f {
        Form::Full => tl!("Full form"),
        Form::Short => tl!("Short form"),
        Form::Outline(_) => tl!("Outline"),
    };
    ui.horizontal(|ui| {
        craft_ui::ComboBox::from_id_salt(salt).selected_text(label(*form)).show_ui(ui, |ui| {
            let depth = if let Form::Outline(d) = *form { d } else { 1 };
            for f in [Form::Full, Form::Short, Form::Outline(depth)] {
                if ui.selectable_label(form.name() == f.name(), label(f)).clicked() {
                    *form = f;
                }
            }
        });
        if let Form::Outline(d) = form {
            ui.label(RichText::new(tl!("levels")).small());
            ui.add(egui::DragValue::new(d).range(0..=6)).on_hover_text(tl!("Paragraph levels under each Article (0: headings only)"));
        }
    });
}

/// The paragraph's place in the short form (M16): in or left out, and its short wording.
fn short_form(app: &mut WordApp, ui: &mut Ui, t: &Tokens, id: &str) {
    let key = (app.session.document_id(), app.session.rev());
    if app.speccraft.short_cache.as_ref().is_none_or(|(k, _)| *k != key) {
        let v = app.session.run("spec.short.list", &json!({})).unwrap_or(Value::Null);
        app.speccraft.short_cache = Some((key, v));
    }
    let list = app.speccraft.short_cache.as_ref().map(|(_, v)| v.clone()).unwrap_or(Value::Null);
    let mark = list.get("marks").and_then(Value::as_array).and_then(|m| m.iter().find(|x| x.get("id").and_then(Value::as_str) == Some(id))).cloned();
    let leave = mark.as_ref().and_then(|m| m.get("leave")).and_then(Value::as_bool).unwrap_or(false);
    let text = mark.as_ref().and_then(|m| m.get("text")).and_then(Value::as_str).unwrap_or("").to_string();
    let stale = mark.as_ref().and_then(|m| m.get("stale")).and_then(Value::as_bool).unwrap_or(false);
    let mut cmd: Option<Value> = None;
    ui.add_space(2.0);
    let mut inside = !leave;
    if ui
        .checkbox(&mut inside, tl!("In the short form"))
        .on_hover_text(tl!("Left out, the paragraph and what is under it are not in the short or outline form"))
        .changed()
    {
        cmd = Some(json!({"id": id, "leave": !inside}));
    }
    if inside {
        if app.speccraft.short_draft.as_ref().is_none_or(|(d, _)| d != id) {
            app.speccraft.short_draft = Some((id.to_string(), text.clone()));
        }
        if let Some((_, draft)) = app.speccraft.short_draft.as_mut() {
            let resp = ui.add(egui::TextEdit::singleline(draft).hint_text(tl!("Short wording (blank: the full text)")).desired_width(f32::INFINITY));
            if resp.lost_focus() && draft.trim() != text {
                cmd = Some(json!({"id": id, "text": draft.trim()}));
            }
        }
        if stale {
            ui.label(RichText::new(tl!("The full wording changed since this short wording was written.")).small().color(t.orange));
        }
    }
    if let Some(params) = cmd
        && let Err(e) = app.run("spec.short.set", params)
    {
        app.status(e);
    }
}

/// The printed view: the pages as they will publish, rendered from the printable copy.
fn printed(app: &mut WordApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let width = (ui.available_width() - 8.0).max(80.0);
    let rev = app.session.rev();
    form_choice(ui, "spec_printed_form", &mut app.speccraft.print_form);
    let form = app.speccraft.print_form;
    let stale = app.speccraft.printed.as_ref().is_none_or(|p| p.rev != rev || p.width != width as u32 || p.form != form);
    if stale {
        let p = render_printed(app, ui.ctx(), width);
        app.speccraft.printed = Some(p);
    }
    let Some(p) = &app.speccraft.printed else { return };
    if let Some(note) = &p.note {
        ui.label(RichText::new(note).small().color(t.text_dim));
    }
    let n = p.pages.len();
    ui.label(RichText::new(crate::i18n::fmt(tl!("{n} pages, as they will publish"), &[("n", &n.to_string())])).small().color(t.text_dim));
    let pages = p.pages.clone();
    egui::ScrollArea::vertical().id_salt("spec_printed").auto_shrink([false, false]).show(ui, |ui| {
        for (tex, size) in &pages {
            let (rect, _) = ui.allocate_exact_size(*size, egui::Sense::hover());
            ui.painter().rect_filled(rect.translate(egui::vec2(2.0, 2.0)), 0.0, Color32::from_black_alpha(40));
            ui.painter().image(tex.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            ui.add_space(8.0);
        }
    });
}

/// Lay out and render the printable copy (placeholders filled from the project beside the
/// section) at `width` points per page.
fn render_printed(app: &WordApp, ctx: &egui::Context, width: f32) -> Printed {
    use wordcraft_engine::cmd::spec::{output, project};
    let rev = app.session.rev();
    let form = app.speccraft.print_form;
    let empty = |note: String| Printed { rev, width: width as u32, form, pages: Vec::new(), note: Some(note) };
    let mut copy = match output::printable_copy_as(&app.session.doc, form) {
        Ok((copy, _)) => copy,
        // The engine's reason is for agents; the pane says it in the interface's language.
        Err(_) => return empty(tl!("This document has no spec paragraphs yet.").to_string()),
    };
    let mut note = None;
    if let Ok(Some(path)) = project::project_for(app.session.path.as_deref(), None)
        && let Ok(f) = project::load(&path)
    {
        project::fill_placeholders(&mut copy, &project::placeholder_values(&f));
    } else {
        note = Some(tl!("No project beside this section: placeholders are shown as written.").to_string());
    }
    let opts = wordcraft_layout::LayoutOptions { view: wordcraft_layout::ViewMode::Print, ..Default::default() };
    let lay = wordcraft_layout::layout(&copy, &mut wordcraft_layout::LayoutCache::new(), &opts);
    let ppp = ctx.pixels_per_point();
    let max_side = ctx.input(|i| i.max_texture_side) as f32;
    let mut pages = Vec::new();
    for (i, pg) in lay.pages.iter().enumerate().take(200) {
        let scale = (width / pg.w.max(1.0) * ppp).clamp(0.05, 2.0).min(max_side / pg.w.max(pg.h).max(1.0));
        let img = wordcraft_render::render_page(&copy, pg, scale, &Default::default());
        let color = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.to_straight());
        let tex = ctx.load_texture(format!("spec_printed_{i}"), color, egui::TextureOptions::LINEAR);
        pages.push((tex, egui::vec2(width, width * pg.h / pg.w.max(1.0))));
    }
    Printed { rev, width: width as u32, form, pages, note }
}
