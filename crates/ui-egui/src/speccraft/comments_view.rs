//! Spec Outline pane › Comments (M25): the section's review comments with priority and status,
//! filters, go to a comment's paragraph, and the project's comments report.

use egui::{Color32, RichText, Ui};
use serde_json::{Value, json};
use wordcraft_engine::cmd::spec::comments::{PRIORITIES, STATUSES};

use crate::WordApp;
use crate::theme::Tokens;

/// A priority's colour and label.
pub(crate) fn priority_style(t: &Tokens, p: &str) -> (Color32, &'static str) {
    match p {
        "critical" => (t.red, "Critical"),
        "high" => (t.orange, "High"),
        "medium" => (Color32::from_rgb(196, 160, 0), "Medium"),
        "low" => (t.blue, "Low"),
        _ => (t.text_disabled, "Not set"),
    }
}

fn status_label(s: &str) -> &'static str {
    match s {
        "draft" => "Draft",
        "accepted" => "Accepted",
        "rejected" => "Rejected",
        "resolved" => "Resolved",
        "closed" => "Closed",
        _ => "Open",
    }
}

pub(crate) fn view(app: &mut WordApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let list = match app.run("spec.comments.list", json!({})) {
        Ok(v) => v,
        Err(e) => {
            ui.label(RichText::new(e).small().color(t.red));
            return;
        }
    };
    let rows: Vec<Value> = list.get("comments").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut action: Option<(&'static str, Value)> = None;
    // Counts by priority, each a filter switch.
    ui.horizontal_wrapped(|ui| {
        for p in PRIORITIES {
            let n = rows.iter().filter(|r| r.get("priority").and_then(Value::as_str) == Some(p)).count();
            let (color, label) = priority_style(&t, p);
            let on = !app.speccraft.comment_hidden.contains(p);
            let text = RichText::new(format!("{} {n}", tl!(label))).small().color(if on { color } else { t.text_disabled });
            if ui.selectable_label(on, text).clicked() {
                if on {
                    app.speccraft.comment_hidden.insert(p.to_string());
                } else {
                    app.speccraft.comment_hidden.remove(p);
                }
            }
        }
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut app.speccraft.comment_open_only, RichText::new(tl!("Hide resolved and closed")).small());
        if ui.small_button(tl!("New Comment")).on_hover_text(tl!("On the selection or the paragraph at the caret")).clicked() {
            action = Some(("review.newComment", json!({})));
        }
    });
    ui.horizontal(|ui| {
        if ui.small_button(tl!("Comments Report")).on_hover_text(tl!("Every section's comments, as a Word file in the Published folder")).clicked() {
            action = Some(("ui.spec.commentsReport", json!({"format": "docx"})));
        }
        if ui.small_button(tl!("As a Sheet (CSV)")).clicked() {
            action = Some(("ui.spec.commentsReport", json!({"format": "csv"})));
        }
    });
    ui.separator();
    if rows.is_empty() {
        ui.label(RichText::new(tl!("No comments in this section.")).small().color(t.text_dim));
    }
    egui::ScrollArea::vertical().id_salt("spec_comments").auto_shrink([false, false]).show(ui, |ui| {
        for r in &rows {
            let s = |k: &str| r.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            let (id, pri, st) = (r.get("id").and_then(Value::as_u64).unwrap_or(0), s("priority"), s("status"));
            if app.speccraft.comment_hidden.contains(&pri) || (app.speccraft.comment_open_only && matches!(st.as_str(), "resolved" | "closed")) {
                continue;
            }
            let (color, _) = priority_style(&t, &pri);
            egui::Frame::new().stroke(egui::Stroke::new(1.0, color)).corner_radius(4.0).inner_margin(6.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    let place = r.get("paragraph").map(|p| {
                        let seq = p.get("seq").and_then(Value::as_u64).unwrap_or(0);
                        format!("{seq:04}")
                    });
                    if let Some(place) = place
                        && ui.link(RichText::new(place).small().monospace()).on_hover_text(tl!("Go to the paragraph")).clicked()
                    {
                        action = Some(("review.nextComment", json!({"__goto": id})));
                    }
                    ui.label(RichText::new(s("author")).small().color(t.text_dim));
                });
                ui.label(RichText::new(s("text")).small());
                for rep in r.get("replies").and_then(Value::as_array).into_iter().flatten() {
                    let rs = |k: &str| rep.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                    ui.label(RichText::new(format!("↳ {}: {}", rs("author"), rs("text"))).small().color(t.text_dim));
                }
                ui.horizontal(|ui| {
                    let (_, plabel) = priority_style(&t, &pri);
                    craft_ui::ComboBox::from_id_salt(("spec_comment_pri", id)).selected_text(RichText::new(tl!(plabel)).color(color)).show_ui(
                        ui,
                        |ui| {
                            for p in PRIORITIES {
                                let (c, l) = priority_style(&t, p);
                                if ui.selectable_label(pri == p, RichText::new(tl!(l)).color(c)).clicked() {
                                    action = Some(("spec.comments.set", json!({"id": id, "priority": p})));
                                }
                            }
                        },
                    );
                    craft_ui::ComboBox::from_id_salt(("spec_comment_st", id)).selected_text(tl!(status_label(&st))).show_ui(ui, |ui| {
                        for x in STATUSES {
                            if ui.selectable_label(st == x, tl!(status_label(x))).clicked() {
                                action = Some(("spec.comments.set", json!({"id": id, "status": x})));
                            }
                        }
                    });
                });
            });
            ui.add_space(4.0);
        }
    });
    let Some((cmd, params)) = action else { return };
    let r = match (cmd, params.get("__goto").and_then(Value::as_u64)) {
        // Go to a comment: its paragraph's sequence number.
        (_, Some(id)) => goto_comment(app, id),
        _ => app.run(cmd, params).map(|_| ()),
    };
    if let Err(e) = r {
        app.status(e);
    }
}

/// Put the caret on comment `id`'s paragraph.
fn goto_comment(app: &mut WordApp, id: u64) -> Result<(), String> {
    let list = app.run("review.comments", json!({}))?;
    let anchor =
        list.as_array().and_then(|a| a.iter().find(|c| c.get("id").and_then(Value::as_u64) == Some(id))).and_then(|c| c.get("anchor")).cloned();
    let Some(pos) = anchor.filter(|a| !a.is_null()) else { return Err(tl!("That comment isn't on a paragraph").to_string()) };
    app.run("caret.set", json!({"pos": pos}))?;
    app.canvas.scroll_to_caret = true;
    Ok(())
}
