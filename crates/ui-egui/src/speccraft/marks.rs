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
use wordcraft_engine::cmd::spec::fields::{self, FieldState};
use wordcraft_engine::cmd::spec::rules::{self, Standing};
use wordcraft_engine::cmd::spec::sections::{self, State};
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
    pub pieces: Vec<(usize, wordcraft_geom::Rect)>,
}

/// A fill-in as the page shows it: its state and where its text is.
#[derive(Clone)]
pub(crate) struct MarkedField {
    pub state: FieldState,
    pub pieces: Vec<(usize, wordcraft_geom::Rect)>,
}

/// The marks of a document revision laid out one way.
pub(crate) struct MarksCache {
    rev: u64,
    layout: Arc<DocLayout>,
    pub items: Arc<Vec<Marked>>,
    pub fields: Arc<Vec<MarkedField>>,
}

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
    let items: Vec<Marked> = sections::states(doc)
        .into_iter()
        .enumerate()
        .map(|(seq, (e, state))| {
            let pieces = layout.selection_rects(doc, &e.range.start, &e.range.end, hint);
            let origin = origin_of.get(&e.id).copied().unwrap_or(Origin::Master);
            let advice = advice.get(&e.id).copied();
            Marked { id: e.id, seq, state, linked: e.linked, origin, advice, pieces }
        })
        .collect();
    let marked_fields: Vec<MarkedField> = fields::fields(doc)
        .into_iter()
        .map(|f| MarkedField { state: f.state, pieces: layout.selection_rects(doc, &f.range.content_start(), &f.range.end, hint) })
        .collect();
    let items = Arc::new(items);
    app.speccraft.marks_cache = Some(MarksCache { rev, layout: layout.clone(), items: items.clone(), fields: Arc::new(marked_fields) });
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
    // Fill-ins: open ones tinted, chosen ones underlined, left-out text struck through.
    for f in marked_fields(app).iter() {
        for (pi, r) in &f.pieces {
            let Some(pr) = rects.get(*pi) else { continue };
            let scale = layout.pages.get(*pi).map_or(fallback_scale, |page| crate::canvas::page_screen_scale(*pr, page, fallback_scale));
            let sr = Rect::from_min_size(pos2(pr.min.x + r.x * scale, pr.min.y + r.y * scale), vec2(r.w * scale, r.h * scale));
            if !sr.intersects(clip) {
                continue;
            }
            match f.state {
                FieldState::Open => {
                    painter.rect_filled(sr, 2.0, t.orange.gamma_multiply(0.18));
                }
                FieldState::Set => {
                    painter.line_segment([sr.left_bottom(), sr.right_bottom()], Stroke::new(1.0, t.accent.gamma_multiply(0.7)));
                }
                FieldState::Omitted => {
                    let y = sr.center().y;
                    painter.line_segment([pos2(sr.min.x, y), pos2(sr.max.x, y)], Stroke::new(1.0, t.text_disabled));
                }
            }
        }
    }
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
