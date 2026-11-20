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
}

impl Filter {
    fn label(self) -> &'static str {
        match self {
            Filter::All => "All paragraphs",
            Filter::Printing => "Printing",
            Filter::Off => "Off and refused",
            Filter::Notes => "Notes",
        }
    }
    fn keeps(self, state: &str) -> bool {
        match self {
            Filter::All => true,
            Filter::Printing => state == "on",
            Filter::Off => state == "off" || state == "refused",
            Filter::Notes => state == "note",
        }
    }
}

/// The pane's two views.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum View {
    #[default]
    Outline,
    Printed,
}

/// Rendered pages of the printed copy, for one document revision.
pub(crate) struct Printed {
    rev: u64,
    width: u32,
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
        "refused" => State::Refused,
        "note" => State::Note,
        _ => State::Off,
    }
}

/// Move the caret to the start of top-level body paragraph `block` and bring it into view.
fn go_to(app: &mut WordApp, block: u64) {
    let pos = wordcraft_doc::Pos::new(wordcraft_doc::StoryRef::Body, wordcraft_doc::Path(vec![block as u32]), 0);
    if app.run("caret.set", json!({"pos": wordcraft_engine::cmd::pos_json(&pos)})).is_ok() {
        app.canvas.scroll_to_caret = true;
    }
}

/// The Spec Outline pane.
pub(crate) fn pane(app: &mut WordApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        for (v, label) in [(View::Outline, "Outline"), (View::Printed, "Printed")] {
            if ui.selectable_label(app.speccraft.view == v, tl!(label)).clicked() {
                app.speccraft.view = v;
            }
        }
    });
    ui.separator();
    if app.speccraft.view == View::Printed {
        return printed(app, ui);
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
            for f in [Filter::All, Filter::Printing, Filter::Off, Filter::Notes] {
                ui.selectable_value(&mut app.speccraft.filter, f, tl!(f.label()));
            }
        });
    });
    ui.add(egui::TextEdit::singleline(&mut app.speccraft.search).hint_text(tl!("Search paragraphs")).desired_width(f32::INFINITY));
    ui.add_space(4.0);
    let caret = o.get("caret").and_then(Value::as_str).map(str::to_string);
    let needle = app.speccraft.search.trim().to_lowercase();
    let filter = app.speccraft.filter;
    let mut toggle: Option<String> = None;
    let mut goto: Option<u64> = None;
    let detail_h = 120.0;
    let list_h = (ui.available_height() - detail_h).max(120.0);
    egui::ScrollArea::vertical().id_salt("spec_outline_list").max_height(list_h).auto_shrink([false, false]).show(ui, |ui| {
        for it in &items {
            let s = |k: &str| it.get(k).and_then(Value::as_str).unwrap_or("");
            let state = s("state");
            if !filter.keeps(state) || (!needle.is_empty() && !s("text").to_lowercase().contains(&needle)) {
                continue;
            }
            let level = it.get("level").and_then(Value::as_u64).unwrap_or(1) as f32;
            let kind = s("kind");
            let here = caret.as_deref() == Some(s("id"));
            ui.horizontal(|ui| {
                ui.add_space(level.min(7.0) * 10.0);
                let st = state_of(state);
                if st == State::Note {
                    ui.label(RichText::new("//").small().color(t.text_dim)).on_hover_text(tl!(super::marks::describe(st)));
                } else {
                    let mut on = st == State::On;
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
                    State::On => text.color(t.text),
                    State::Note => text.italics().color(t.text_dim),
                    _ => text.color(t.text_disabled),
                };
                let resp = ui.add(egui::Button::selectable(here, text).frame_when_inactive(false).truncate());
                if resp.clicked() {
                    goto = it.get("block").and_then(Value::as_u64);
                }
            });
        }
    });
    ui.separator();
    details(app, ui, &items, caret.as_deref());
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

/// What the paragraph at the caret is.
fn details(app: &mut WordApp, ui: &mut Ui, items: &[Value], caret: Option<&str>) {
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
    let parent = it.get("parent").and_then(Value::as_str).and_then(|p| items.iter().find(|i| i["id"] == p));
    if let Some(p) = parent {
        let text: String = s(p, "text").chars().take(60).collect();
        let pstate = state_of(&s(p, "state"));
        let line = crate::i18n::fmt(tl!("Under: {text}"), &[("text", &text)]);
        let color = if pstate == State::On { t.text_dim } else { t.orange };
        ui.label(RichText::new(line).small().color(color));
    }
    let id = s(it, "id");
    let children = items.iter().filter(|i| i.get("parent").and_then(Value::as_str) == Some(id.as_str())).count();
    if children > 0 {
        ui.label(RichText::new(crate::i18n::fmt(tl!("{n} paragraphs under it"), &[("n", &children.to_string())])).small().color(t.text_dim));
    }
    ui.label(RichText::new(id).small().monospace().color(t.text_disabled));
    if state != State::Note && ui.small_button(tl!("Switch On or Off")).clicked() {
        let _ = app.run("spec.toggle", json!({}));
    }
}

/// The printed view: the pages as they will publish, rendered from the printable copy.
fn printed(app: &mut WordApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let width = (ui.available_width() - 8.0).max(80.0);
    let rev = app.session.rev();
    let stale = app.speccraft.printed.as_ref().is_none_or(|p| p.rev != rev || p.width != width as u32);
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
    let empty = |note: String| Printed { rev, width: width as u32, pages: Vec::new(), note: Some(note) };
    let mut copy = match output::printable_copy(&app.session.doc) {
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
    Printed { rev, width: width as u32, pages, note }
}
