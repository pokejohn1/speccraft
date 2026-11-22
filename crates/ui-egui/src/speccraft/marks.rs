//! Spec marks on the page: a small mark in the left margin of each section paragraph, coloured
//! by its state, and a wash over paragraphs that won't print, so the page reads like SpecLink's
//! sequence-number column without changing the document. Clicking a mark switches its paragraph.
//!
//! Drawn over the page image, like Find's reading highlight, so nothing is added to the document
//! and nothing reaches the PDF.

use std::sync::Arc;

use egui::{Color32, Painter, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;
use wordcraft_engine::cmd::spec::edits::{self, Origin};
use wordcraft_engine::cmd::spec::fields::{self, Field, FieldKind, FieldState};
use wordcraft_engine::cmd::spec::rules::{self, Standing};
use wordcraft_engine::cmd::spec::sections::{self, State};
use wordcraft_engine::cmd::spec::terms;
use wordcraft_layout::DocLayout;

use crate::WordApp;
use crate::theme::Tokens;

/// One section paragraph as the marks need it: id, state, and its pieces on the pages.
#[derive(Clone)]
pub(crate) struct Marked {
    pub id: String,
    /// Sequence number: the paragraph's place in the section, the title 0 (M17).
    pub seq: usize,
    pub state: State,
    /// Turned on by a rule (whether or not the author switched it on too).
    pub linked: bool,
    /// Reworded (`modified`) or written (`added`) in this project.
    pub origin: Origin,
    /// What the section's rules say about it now: a recommendation not followed, or a conflict.
    pub advice: Option<Standing>,
    /// Switched on or off since the review round began (M24).
    pub switched: bool,
    pub pieces: Vec<(usize, wordcraft_geom::Rect)>,
}

/// A fill-in as the page shows it: its state and where its text is.
#[derive(Clone)]
pub(crate) struct MarkedField {
    pub field: Field,
    pub pieces: Vec<(usize, wordcraft_geom::Rect)>,
}

/// A fill-in's colour on the page (M19): measurements green, choices blue, blanks red.
fn field_color(t: &Tokens, f: &Field) -> Color32 {
    if f.measure {
        t.green
    } else if matches!(f.kind, FieldKind::Choice | FieldKind::Optional) {
        t.blue
    } else {
        t.red
    }
}

/// A global term, section reference or reference standard on the page (M20).
#[derive(Clone)]
pub(crate) struct MarkedText {
    pub kind: TextKind,
    /// A reference's section number; a term's name; a standard's key.
    pub name: String,
    pub pieces: Vec<(usize, wordcraft_geom::Rect)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextKind {
    Term,
    Ref,
    Standard,
}

/// The marks of a document revision laid out one way.
pub(crate) struct MarksCache {
    rev: u64,
    layout: Arc<DocLayout>,
    pub items: Arc<Vec<Marked>>,
    pub fields: Arc<Vec<MarkedField>>,
    pub texts: Arc<Vec<MarkedText>>,
}

/// Global terms' colour (M20): purple, readable on the page in both themes.
const TERM: Color32 = Color32::from_rgb(132, 72, 178);

/// The section paragraphs of the document on screen and where they are, cached per revision
/// and layout.
pub(crate) fn marked(app: &mut WordApp, layout: &Arc<DocLayout>) -> Arc<Vec<Marked>> {
    let rev = app.session.rev();
    if let Some(c) = &app.speccraft.marks_cache
        && c.rev == rev
        && Arc::ptr_eq(&c.layout, layout)
    {
        return c.items.clone();
    }
    let doc = &app.session.doc;
    let hint = app.session.page_hint;
    let (origins, _) = edits::origins(doc);
    let mut advice: std::collections::HashMap<String, Standing> = std::collections::HashMap::new();
    for (r, st) in rules::standings(doc) {
        if matches!(st, Standing::Suggested | Standing::Conflict) {
            let worse = advice.get(&r.target).is_none_or(|old| *old != Standing::Conflict);
            if worse {
                advice.insert(r.target.clone(), st);
            }
        }
    }
    let origin_of: std::collections::HashMap<String, Origin> = origins.into_iter().map(|(e, o)| (e.id, o)).collect();
    let switched: std::collections::HashSet<String> =
        wordcraft_engine::cmd::spec::review::switched(doc).map(|(on, off)| on.into_iter().chain(off).collect()).unwrap_or_default();
    let items: Vec<Marked> = sections::states(doc)
        .into_iter()
        .enumerate()
        .map(|(seq, (e, state))| {
            let pieces = layout.selection_rects(doc, &e.range.start, &e.range.end, hint);
            let origin = origin_of.get(&e.id).copied().unwrap_or(Origin::Master);
            let advice = advice.get(&e.id).copied();
            let switched = switched.contains(&e.id);
            Marked { id: e.id, seq, state, linked: e.linked, origin, advice, switched, pieces }
        })
        .collect();
    let marked_fields: Vec<MarkedField> = fields::fields(doc)
        .into_iter()
        .map(|f| {
            let pieces = layout.selection_rects(doc, &f.range.content_start(), &f.range.end, hint);
            MarkedField { field: f, pieces }
        })
        .collect();
    // Terms and references (inline controls), and standards found in the text (M20).
    let mut texts: Vec<MarkedText> = terms::inline(doc)
        .into_iter()
        .map(|x| MarkedText {
            kind: if x.kind == terms::InlineKind::Term { TextKind::Term } else { TextKind::Ref },
            name: x.name.clone(),
            pieces: layout.selection_rects(doc, &x.range.content_start(), &x.range.end, hint),
        })
        .collect();
    for (b, block) in doc.body.iter().enumerate() {
        let Some(p) = block.as_para() else { continue };
        for st in terms::find_standards(&p.text) {
            let at = |off: usize| wordcraft_doc::Pos::new(wordcraft_doc::StoryRef::Body, wordcraft_doc::Path::top(b), off);
            texts.push(MarkedText {
                kind: TextKind::Standard,
                name: st.key(),
                pieces: layout.selection_rects(doc, &at(st.range.start), &at(st.range.end), hint),
            });
        }
    }
    let items = Arc::new(items);
    app.speccraft.marks_cache =
        Some(MarksCache { rev, layout: layout.clone(), items: items.clone(), fields: Arc::new(marked_fields), texts: Arc::new(texts) });
    items
}

/// The fill-ins of the document on screen (after [`marked`] for the same layout).
fn marked_fields(app: &WordApp) -> Arc<Vec<MarkedField>> {
    app.speccraft.marks_cache.as_ref().map(|c| c.fields.clone()).unwrap_or_default()
}

/// How a state's sequence number is drawn: box fill, box outline and number colour. As in the
/// sequence column authors know: grey when off, filled when the author switched it on, dark
/// (with a green bar) when a rule did, red when left out, faint and dotted for notes.
fn colors(t: &Tokens, state: State) -> (Color32, Color32, Color32) {
    match state {
        State::On => (t.accent, t.accent, t.on_accent),
        State::Linked => (Color32::TRANSPARENT, Color32::TRANSPARENT, t.text),
        State::Overridden => (Color32::TRANSPARENT, Color32::TRANSPARENT, t.red),
        State::Off | State::Refused => (Color32::TRANSPARENT, Color32::TRANSPARENT, t.text_disabled),
        State::Note => (Color32::TRANSPARENT, t.text_disabled, t.text_disabled),
    }
}

/// What a state means, for the mark's tooltip.
pub(crate) fn describe(state: State) -> &'static str {
    match state {
        State::On => "On: prints. Click to switch off. Right-click for more.",
        State::Linked => "On because a rule turned it on: prints. Click to switch it off anyway. Right-click for more.",
        State::Overridden => "Left out: no rule turns it on. Click to switch it on, or right-click to include it again.",
        State::Off => "Off: doesn't print. Click to switch on. Right-click for more.",
        State::Refused => "Chosen, but its parent is off: it prints once the parent is switched on. Click to clear.",
        State::Note => "Author note: never prints.",
    }
}

/// A paragraph's sequence number as the column shows it: four digits.
pub(crate) fn seq_label(seq: usize) -> String {
    format!("{:04}", seq.min(9999))
}

/// Paint the marks over the pages (`rects`: each page's rectangle on screen). Called by the
/// canvas after the reading highlight; does nothing while the page shows a preview copy or the
/// marks are hidden.
pub(crate) fn paint(app: &mut WordApp, ui: &mut Ui, painter: &Painter, rects: &[Rect], layout: &Arc<DocLayout>, fallback_scale: f32) {
    if app.speccraft.hide_marks || app.session.view.read_mode || crate::canvas::preview_sel(app).is_some() {
        return;
    }
    let items = marked(app, layout);
    if items.is_empty() {
        return;
    }
    let t = Tokens::get(ui.ctx());
    let clip = ui.clip_rect();
    paint_fields(app, ui, painter, rects, layout, fallback_scale);
    let mut clicked: Option<String> = None;
    let mut action: Option<(&'static str, serde_json::Value)> = None;
    for m in items.iter() {
        // The paragraph's extent on each page it is on.
        let mut by_page: Vec<(usize, Rect)> = Vec::new();
        for (pi, r) in &m.pieces {
            let Some(pr) = rects.get(*pi) else { continue };
            let scale = layout.pages.get(*pi).map_or(fallback_scale, |page| crate::canvas::page_screen_scale(*pr, page, fallback_scale));
            let sr = Rect::from_min_size(pos2(pr.min.x + r.x * scale, pr.min.y + r.y * scale), vec2(r.w * scale, r.h * scale));
            match by_page.iter_mut().find(|(p, _)| p == pi) {
                Some((_, u)) => *u = u.union(sr),
                None => by_page.push((*pi, sr)),
            }
        }
        for (k, (pi, area)) in by_page.iter().enumerate() {
            if !area.intersects(clip) {
                continue;
            }
            // A wash over what won't print, so it reads as greyed.
            match m.state {
                State::Off | State::Refused | State::Overridden => {
                    let wash = if t.dark { Color32::from_black_alpha(130) } else { Color32::from_white_alpha(150) };
                    painter.rect_filled(area.expand(1.0), 0.0, wash);
                }
                State::Note => {
                    let bar = Rect::from_min_max(pos2(area.min.x - 6.0, area.min.y), pos2(area.min.x - 4.0, area.max.y));
                    painter.rect_filled(bar, 0.0, t.text_dim.gamma_multiply(0.5));
                }
                State::On | State::Linked => {}
            }
            // The sequence column sits in the page's left margin, all numbers in one straight
            // column (W2, M17), on the paragraph's first line; the outline shows the levels.
            // Origin (M* / U) before it, the rule bar after it.
            let Some(pr) = rects.get(*pi) else { continue };
            if k > 0 {
                continue;
            }
            let scale = layout.pages.get(*pi).map_or(fallback_scale, |page| crate::canvas::page_screen_scale(*pr, page, fallback_scale));
            let h = (10.0 * scale).clamp(8.0, 18.0);
            let w = (22.0 * scale).clamp(18.0, 40.0);
            let font = egui::FontId::monospace((6.5 * scale).clamp(6.0, 12.0));
            let right = pr.min.x + 46.0 * scale;
            let top = area.min.y + ((area.height().min(h * 1.6) - h) * 0.5).max(0.0);
            let mark = Rect::from_min_size(pos2(right - w, top), vec2(w, h));
            let (fill, stroke, ink) = colors(&t, m.state);
            painter.rect_filled(mark, 2.0, fill);
            if m.state == State::Note {
                dotted(painter, mark, Stroke::new(1.0, stroke));
            }
            painter.text(mark.center(), egui::Align2::CENTER_CENTER, seq_label(m.seq), font.clone(), ink);
            if m.state == State::Refused {
                painter.circle_filled(pos2(mark.min.x + 2.5, mark.center().y), (h * 0.16).max(1.5), t.blue);
            }
            if m.state == State::Overridden {
                let c = pos2(mark.min.x - h * 0.45, mark.center().y);
                let r = h * 0.28;
                painter.circle_filled(c, r + 1.0, t.red);
                let k = Rect::from_center_size(c, vec2(r, r));
                painter.line_segment([k.left_top(), k.right_bottom()], Stroke::new(1.2, t.on_accent));
                painter.line_segment([k.right_top(), k.left_bottom()], Stroke::new(1.2, t.on_accent));
            }
            // What the rules say: green when a rule turned it on, yellow for a recommendation not
            // followed, red for a conflict.
            let bar_color = match m.advice {
                Some(Standing::Conflict) => Some(t.red),
                Some(_) => Some(t.orange),
                None if m.linked && m.state != State::Overridden => Some(t.green),
                None => None,
            };
            if let Some(color) = bar_color {
                let bar = Rect::from_min_max(pos2(mark.max.x + 1.5, mark.min.y), pos2(mark.max.x + 1.5 + (2.5 * scale).clamp(2.0, 4.0), mark.max.y));
                painter.rect_filled(bar, 0.0, color);
            }
            // Origin, before the number: M* reworded, U written in this project.
            let tag = match m.origin {
                Origin::Modified => Some(("M*", t.orange)),
                Origin::Added => Some(("U", t.accent)),
                Origin::Master => None,
            };
            if let Some((tag, color)) = tag
                && m.state != State::Overridden
            {
                painter.text(pos2(mark.min.x - 2.0, mark.center().y), egui::Align2::RIGHT_CENTER, tag, font, color);
            }
            // Switched on or off this review round (M24): a blue bar left of the column.
            if m.switched {
                let x = mark.min.x - (16.0 * scale).clamp(12.0, 28.0);
                let bar = Rect::from_min_max(pos2(x, area.min.y), pos2(x + (2.0 * scale).clamp(2.0, 3.0), area.max.y.max(mark.max.y)));
                painter.rect_filled(bar, 0.0, t.blue);
            }
            let resp = ui.interact(mark.expand(1.0), ui.scope_id().with(("spec_mark", &m.id)), Sense::click());
            let resp = resp.on_hover_text(tl!(describe(m.state)));
            if resp.hovered() && m.state != State::Note {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() && m.state != State::Note {
                clicked = Some(m.id.clone());
            }
            if m.state != State::Note {
                resp.context_menu(|ui| {
                    if let Some(a) = mark_menu(ui, m) {
                        action = Some(a);
                    }
                });
            }
        }
    }
    if let Some((cmd, params)) = action
        && let Err(e) = app.run(cmd, params)
    {
        app.status(e);
    }
    if let Some(id) = clicked
        && let Err(e) = app.run("spec.toggle", json!({"id": id}))
    {
        app.status(e);
    }
}

/// A dotted outline (a note's number: it can't be switched on).
fn dotted(painter: &Painter, r: Rect, stroke: Stroke) {
    let step = 3.0;
    let mut x = r.min.x;
    while x < r.max.x {
        let x2 = (x + 1.5).min(r.max.x);
        painter.line_segment([pos2(x, r.min.y), pos2(x2, r.min.y)], stroke);
        painter.line_segment([pos2(x, r.max.y), pos2(x2, r.max.y)], stroke);
        x += step;
    }
    let mut y = r.min.y;
    while y < r.max.y {
        let y2 = (y + 1.5).min(r.max.y);
        painter.line_segment([pos2(r.min.x, y), pos2(r.min.x, y2)], stroke);
        painter.line_segment([pos2(r.max.x, y), pos2(r.max.x, y2)], stroke);
        y += step;
    }
}

/// The menu on a sequence number (M17): switch, leave out, why, and the paragraph's editing
/// commands. Returns the command chosen.
fn mark_menu(ui: &mut Ui, m: &Marked) -> Option<(&'static str, serde_json::Value)> {
    let id = json!(m.id);
    let mut out = None;
    let on = matches!(m.state, State::On | State::Linked);
    let label = if on { "Switch Off" } else { "Switch On" };
    if ui.button(tl!(label)).clicked() {
        out = Some(("spec.toggle", json!({"id": id})));
    }
    let mut excluded = m.state == State::Overridden;
    if ui.checkbox(&mut excluded, tl!("Leave Out and Ignore Rules")).clicked() {
        out = Some(("spec.exclude", json!({"id": id, "exclude": excluded})));
    }
    if ui.button(tl!("Why Is This On or Off?")).clicked() {
        out = Some(("ui.spec.why", json!({"id": id})));
    }
    ui.separator();
    if ui.button(tl!("Add Paragraph After")).clicked() {
        out = Some(("spec.addParagraph", json!({"after": id})));
    }
    if ui.button(tl!("Add Paragraph Under")).clicked() {
        out = Some(("spec.addParagraph", json!({"after": id, "child": true})));
    }
    if ui.button(tl!("Duplicate Paragraph")).clicked() {
        out = Some(("spec.duplicate", json!({"id": id})));
    }
    if ui.button(tl!("Indent")).clicked() {
        out = Some(("spec.indent", json!({"id": id, "by": 1})));
    }
    if ui.button(tl!("Outdent")).clicked() {
        out = Some(("spec.indent", json!({"id": id, "by": -1})));
    }
    if ui.button(tl!("No Autonumber")).clicked() {
        out = Some(("spec.noNumber", json!({"id": id})));
    }
    if m.origin == Origin::Modified && ui.button(tl!("Restore Master Wording")).clicked() {
        out = Some(("spec.restore", json!({"id": id})));
    }
    if m.origin == Origin::Added && ui.button(tl!("Delete Added Paragraph(s)")).clicked() {
        out = Some(("ui.spec.deleteAdded", json!({"id": id})));
    }
    if out.is_some() {
        ui.close();
    }
    out
}

/// The list of an open choice on the page (M19), until something is chosen or the page is
/// clicked elsewhere.
fn choice_popup(app: &mut WordApp, ui: &mut Ui, t: &Tokens, frame: u64) {
    let Some((id, at, opened)) = app.speccraft.choice_popup.clone() else { return };
    let Some(choice) = marked_fields(app).iter().find(|f| f.field.id == id).and_then(|f| super::choices::Choice::of(&f.field)) else {
        app.speccraft.choice_popup = None;
        return;
    };
    let mut fill = std::mem::take(&mut app.speccraft.choice_fill);
    let mut action = None;
    let area = egui::Area::new(ui.scope_id().with("spec_choice_popup")).order(egui::Order::Foreground).fixed_pos(at).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_max_width(320.0);
            action = super::choices::list(ui, t, &choice, &mut fill);
        });
    });
    app.speccraft.choice_fill = fill;
    let outside = ui.input(|i| i.pointer.any_click() && i.pointer.interact_pos().is_some_and(|p| !area.response.rect.contains(p)));
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    if let Some((cmd, params)) = action {
        // A multiple choice stays open for the next tick; one option closes it.
        if !choice.multiple || cmd != "spec.fields.set" {
            app.speccraft.choice_popup = None;
        }
        if let Err(e) = app.run(cmd, params) {
            app.status(e);
        }
    } else if escape || (outside && frame > opened) {
        app.speccraft.choice_popup = None;
    }
}

/// Paint the fill-ins (M19) and handle clicks on choices.
fn paint_fields(app: &mut WordApp, ui: &mut Ui, painter: &Painter, rects: &[Rect], layout: &Arc<DocLayout>, fallback_scale: f32) {
    let t = Tokens::get(ui.ctx());
    let clip = ui.clip_rect();
    // Fill-ins: open ones tinted, chosen ones underlined, left-out text struck through, each in
    // its kind's colour. A click on a choice opens its list; a double click puts the caret in it.
    let mut open_choice: Option<(String, egui::Pos2)> = None;
    let mut caret_into: Option<wordcraft_doc::Pos> = None;
    for f in marked_fields(app).iter() {
        let color = field_color(&t, &f.field);
        let mut first: Option<Rect> = None;
        for (pi, r) in &f.pieces {
            let Some(pr) = rects.get(*pi) else { continue };
            let scale = layout.pages.get(*pi).map_or(fallback_scale, |page| crate::canvas::page_screen_scale(*pr, page, fallback_scale));
            let sr = Rect::from_min_size(pos2(pr.min.x + r.x * scale, pr.min.y + r.y * scale), vec2(r.w * scale, r.h * scale));
            if !sr.intersects(clip) {
                continue;
            }
            first.get_or_insert(sr);
            match f.field.state {
                FieldState::Open => {
                    painter.rect_filled(sr, 2.0, color.gamma_multiply(0.16));
                }
                FieldState::Set => {
                    painter.line_segment([sr.left_bottom(), sr.right_bottom()], Stroke::new(1.0, color.gamma_multiply(0.8)));
                }
                FieldState::Omitted => {
                    let y = sr.center().y;
                    painter.line_segment([pos2(sr.min.x, y), pos2(sr.max.x, y)], Stroke::new(1.0, t.text_disabled));
                }
            }
        }
        if let Some(area) = first
            && matches!(f.field.kind, FieldKind::Choice | FieldKind::Optional)
        {
            let resp = ui.interact(area, ui.scope_id().with(("spec_choice", &f.field.id)), Sense::click());
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.double_clicked() {
                caret_into = Some(f.field.range.content_start());
            } else if resp.clicked() {
                open_choice = Some((f.field.id.clone(), area.left_bottom() + vec2(0.0, 2.0)));
            }
        }
    }
    let frame = ui.ctx().cumulative_frame_nr();
    if let Some((id, at)) = open_choice {
        app.speccraft.choice_popup = Some((id, at, frame));
        app.speccraft.choice_fill.clear();
    }
    if let Some(pos) = caret_into {
        app.speccraft.choice_popup = None;
        if app.run("caret.set", json!({"pos": wordcraft_engine::cmd::pos_json(&pos)})).is_err() {
            app.status(tl!("Couldn't put the caret in the fill-in"));
        }
    }
    choice_popup(app, ui, &t, frame);
    paint_texts(app, ui, painter, rects, layout, fallback_scale);
}

/// Paint global terms (purple), section references (green; Ctrl+click opens the section) and
/// reference standards (a dotted green line) (M20).
fn paint_texts(app: &mut WordApp, ui: &mut Ui, painter: &Painter, rects: &[Rect], layout: &Arc<DocLayout>, fallback_scale: f32) {
    let Some(texts) = app.speccraft.marks_cache.as_ref().map(|c| c.texts.clone()) else { return };
    let t = Tokens::get(ui.ctx());
    let clip = ui.clip_rect();
    let ctrl = ui.input(|i| i.modifiers.command);
    let mut open: Option<String> = None;
    for x in texts.iter() {
        let mut first: Option<Rect> = None;
        for (pi, r) in &x.pieces {
            let Some(pr) = rects.get(*pi) else { continue };
            let scale = layout.pages.get(*pi).map_or(fallback_scale, |page| crate::canvas::page_screen_scale(*pr, page, fallback_scale));
            let sr = Rect::from_min_size(pos2(pr.min.x + r.x * scale, pr.min.y + r.y * scale), vec2(r.w * scale, r.h * scale));
            if !sr.intersects(clip) {
                continue;
            }
            first.get_or_insert(sr);
            match x.kind {
                TextKind::Term => {
                    painter.rect_filled(sr, 2.0, TERM.gamma_multiply(0.14));
                    painter.line_segment([sr.left_bottom(), sr.right_bottom()], Stroke::new(1.0, TERM.gamma_multiply(0.8)));
                }
                TextKind::Ref => {
                    painter.line_segment([sr.left_bottom(), sr.right_bottom()], Stroke::new(1.0, t.green));
                }
                TextKind::Standard => {
                    let mut px = sr.min.x;
                    while px < sr.max.x {
                        painter.line_segment([pos2(px, sr.max.y), pos2((px + 1.5).min(sr.max.x), sr.max.y)], Stroke::new(1.0, t.green));
                        px += 3.0;
                    }
                }
            }
        }
        let Some(area) = first else { continue };
        let hint = match x.kind {
            TextKind::Term => crate::i18n::fmt(tl!("Global term: {name}. Change it in Project Information."), &[("name", &x.name)]),
            TextKind::Ref => tl!("Section reference: Ctrl+click to open the section").to_string(),
            TextKind::Standard => crate::i18n::fmt(tl!("Reference standard {name}"), &[("name", &x.name)]),
        };
        // Only a held Ctrl makes a reference clickable, so a plain click still places the caret.
        if x.kind == TextKind::Ref && ctrl {
            let resp = ui.interact(area, ui.scope_id().with(("spec_ref", &x.name, area.min.x as i32, area.min.y as i32)), Sense::click());
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                open = Some(x.name.clone());
            }
        } else {
            // Hover only: a click still goes to the page.
            let _ = ui
                .interact(area, ui.scope_id().with(("spec_text_tip", &x.name, area.min.x as i32, area.min.y as i32)), Sense::hover())
                .on_hover_text(hint);
        }
    }
    if let Some(number) = open {
        let key = wordcraft_engine::cmd::spec::project::number_key(&number);
        let file = app.speccraft.info.as_ref().and_then(|i| i.get("sections")).and_then(serde_json::Value::as_array).and_then(|s| {
            s.iter()
                .find(|x| {
                    x.get("number").and_then(serde_json::Value::as_str).is_some_and(|n| wordcraft_engine::cmd::spec::project::number_key(n) == key)
                })
                .and_then(|x| x.get("file").and_then(serde_json::Value::as_str).map(str::to_string))
        });
        match file {
            Some(f) => {
                if let Err(e) = app.run("ui.spec.openSection", json!({"file": f})) {
                    app.status(e);
                }
            }
            None => app.status(crate::i18n::fmt(tl!("Section {n} is not in this project"), &[("n", &number)])),
        }
    }
}
