//! Spec marks on the page: a small mark in the left margin of each section paragraph, coloured
//! by its state, and a wash over paragraphs that won't print, so the page reads like SpecLink's
//! sequence-number column without changing the document. Clicking a mark switches its paragraph.
//!
//! Drawn over the page image, like Find's reading highlight, so nothing is added to the document
//! and nothing reaches the PDF.

use std::sync::Arc;

use egui::{Color32, Painter, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;
use wordcraft_engine::cmd::spec::fields::{self, FieldState};
use wordcraft_engine::cmd::spec::sections::{self, State};
use wordcraft_layout::DocLayout;

use crate::WordApp;
use crate::theme::Tokens;

/// One section paragraph as the marks need it: id, state, and its pieces on the pages.
#[derive(Clone)]
pub(crate) struct Marked {
    pub id: String,
    pub state: State,
    pub level: u8,
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
    let items: Vec<Marked> = sections::states(doc)
        .into_iter()
        .map(|(e, state)| {
            let pieces = layout.selection_rects(doc, &e.range.start, &e.range.end, hint);
            Marked { id: e.id, state, level: e.level.unwrap_or(1), pieces }
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

/// The colours of a state's mark: fill and outline.
fn colors(t: &Tokens, state: State) -> (Color32, Color32) {
    match state {
        State::On => (t.accent, t.accent),
        State::Off | State::Refused => (Color32::TRANSPARENT, t.text_disabled),
        State::Note => (Color32::TRANSPARENT, t.text_dim),
    }
}

/// What a state means, for the mark's tooltip.
pub(crate) fn describe(state: State) -> &'static str {
    match state {
        State::On => "On: prints. Click to switch off.",
        State::Off => "Off: doesn't print. Click to switch on.",
        State::Refused => "Wanted, but its parent is off: it prints once the parent is switched on. Click to clear.",
        State::Note => "Author note: never prints.",
    }
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
                State::Off | State::Refused => {
                    let wash = if t.dark { Color32::from_black_alpha(130) } else { Color32::from_white_alpha(150) };
                    painter.rect_filled(area.expand(1.0), 0.0, wash);
                }
                State::Note => {
                    let bar = Rect::from_min_max(pos2(area.min.x - 6.0, area.min.y), pos2(area.min.x - 4.0, area.max.y));
                    painter.rect_filled(bar, 0.0, t.text_dim.gamma_multiply(0.5));
                }
                State::On => {}
            }
            // The mark sits in the page's left margin, level by level, on the paragraph's first line.
            let Some(pr) = rects.get(*pi) else { continue };
            if k > 0 {
                continue;
            }
            let scale = layout.pages.get(*pi).map_or(fallback_scale, |page| crate::canvas::page_screen_scale(*pr, page, fallback_scale));
            let size = (9.0 * scale).clamp(6.0, 14.0);
            let x = pr.min.x + (18.0 + 6.0 * f32::from(m.level.min(7))) * scale;
            let center = Pos2::new(x, area.min.y + (size * 0.5 + 2.0 * scale).min(area.height() / 2.0));
            let mark = Rect::from_center_size(center, vec2(size, size));
            let (fill, stroke) = colors(&t, m.state);
            match m.state {
                State::Note => {
                    painter.circle_stroke(center, size * 0.35, Stroke::new(1.0, stroke));
                }
                _ => {
                    painter.rect_filled(mark, 2.0, fill);
                    painter.rect_stroke(mark, 2.0, Stroke::new(1.2, stroke), egui::StrokeKind::Inside);
                }
            }
            if m.state == State::Refused {
                painter.circle_filled(mark.center(), size * 0.22, t.blue);
            }
            let resp = ui.interact(mark.expand(2.0), ui.scope_id().with(("spec_mark", &m.id)), Sense::click());
            let resp = resp.on_hover_text(tl!(describe(m.state)));
            if resp.hovered() && m.state != State::Note {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() && m.state != State::Note {
                clicked = Some(m.id.clone());
            }
        }
    }
    if let Some(id) = clicked
        && let Err(e) = app.run("spec.toggle", json!({"id": id}))
    {
        app.status(e);
    }
}
