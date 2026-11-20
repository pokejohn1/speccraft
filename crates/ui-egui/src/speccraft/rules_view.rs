//! The Spec Outline pane's Rules view: the section's rules with how each stands now, and a form
//! to add one ("when this is on / this fill-in is …, require / recommend / warn / exclude that").

use egui::{RichText, Ui};
use serde_json::{Value, json};

use crate::WordApp;
use crate::theme::{Tokens, regular, semibold};

/// The new-rule form.
#[derive(Clone, Debug, Default)]
pub(crate) struct RuleForm {
    /// 0 require, 1 recommend, 2 warn, 3 exclude.
    kind: usize,
    /// The condition is a fill-in (else a paragraph).
    on_field: bool,
    /// The condition's paragraph must be off (else on).
    when_off: bool,
    cond_para: String,
    cond_field: String,
    /// The fill-in's value; empty: any value.
    cond_is: String,
    target: String,
    note: String,
    /// Narrows the paragraph lists.
    find: String,
}

const KINDS: [(&str, &str); 4] =
    [("require", "Require (switch it on)"), ("recommend", "Recommend"), ("warn", "Warn against"), ("exclude", "Exclude (can't be on)")];

/// `spec.rules.list` for the document on screen, cached per revision.
pub(crate) fn rule_list(app: &mut WordApp) -> Value {
    let key = (app.session.document_id(), app.session.rev());
    if let Some((k, v)) = &app.speccraft.rules_cache
        && *k == key
    {
        return v.clone();
    }
    let v = app.session.run("spec.rules.list", &json!({})).unwrap_or(Value::Null);
    app.speccraft.rules_cache = Some((key, v.clone()));
    v
}

/// The section's paragraphs as (id, short text), for the pickers.
fn paragraphs(app: &mut WordApp) -> Vec<(String, String)> {
    let o = super::outline::outline(app);
    o.get("items")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter(|i| i["state"] != "note")
                .filter_map(|i| Some((i.get("id")?.as_str()?.to_string(), i.get("text")?.as_str()?.chars().take(70).collect())))
                .collect()
        })
        .unwrap_or_default()
}

/// A paragraph picker: a combo box listing the paragraphs that contain `find`.
fn pick_paragraph(ui: &mut Ui, salt: &str, current: &mut String, all: &[(String, String)], find: &str) {
    let shown = all.iter().find(|(id, _)| id == current).map_or_else(|| tl!("Choose a paragraph…").to_string(), |(id, t)| format!("{id}  {t}"));
    let needle = find.trim().to_lowercase();
    craft_ui::ComboBox::from_id_salt(salt).selected_text(shown).width(ui.available_width() - 8.0).show_ui(ui, |ui| {
        for (id, t) in all.iter().filter(|(_, t)| needle.is_empty() || t.to_lowercase().contains(&needle)).take(400) {
            if ui.selectable_label(current == id, format!("{id}  {t}")).clicked() {
                *current = id.clone();
            }
        }
    });
}

/// The Rules view.
pub(crate) fn pane(app: &mut WordApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let list = rule_list(app);
    let rules: Vec<Value> = list.get("rules").and_then(Value::as_array).cloned().unwrap_or_default();
    let paras = paragraphs(app);
    if paras.is_empty() {
        ui.label(RichText::new(tl!("This document has no spec paragraphs yet.")).font(regular(13.0)));
        return;
    }
    let mut remove: Option<String> = None;
    let mut goto: Option<String> = None;
    egui::ScrollArea::vertical().id_salt("spec_rules").auto_shrink([false, false]).max_height((ui.available_height() - 330.0).max(140.0)).show(
        ui,
        |ui| {
            if rules.is_empty() {
                ui.label(
                    RichText::new(tl!("No rules yet. A rule switches paragraphs on, suggests them or keeps them apart.")).small().color(t.text_dim),
                );
            }
            for r in &rules {
                let s = |k: &str| r.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                let (color, standing) = match s("standing").as_str() {
                    "met" => (t.green, tl!("in force")),
                    "suggested" => (t.orange, tl!("suggestion not followed")),
                    "conflict" => (t.red, tl!("conflict")),
                    "overridden" => (t.red, tl!("overridden by you")),
                    "unmet" => (t.orange, tl!("can't be met")),
                    _ => (t.text_disabled, tl!("not in force")),
                };
                let verb = match s("kind").as_str() {
                    "require" => tl!("requires"),
                    "recommend" => tl!("recommends"),
                    "warn" => tl!("warns against"),
                    _ => tl!("excludes"),
                };
                let target_text = paras.iter().find(|(id, _)| *id == s("target")).map_or_else(String::new, |(_, t)| t.clone());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(s("id")).font(semibold(12.0)));
                    ui.label(RichText::new(standing).small().color(color));
                    if ui.small_button("×").on_hover_text(tl!("Remove this rule")).clicked() {
                        remove = Some(s("id"));
                    }
                });
                let line = format!("{} {} {} {}", tl!("When"), s("when"), verb, s("target"));
                if ui
                    .add(egui::Label::new(RichText::new(line).small()).wrap().sense(egui::Sense::click()))
                    .on_hover_text(tl!("Go to the target"))
                    .clicked()
                {
                    goto = Some(s("target"));
                }
                ui.label(RichText::new(target_text).small().italics().color(t.text_dim));
                if !s("note").is_empty() {
                    ui.label(RichText::new(s("note")).small().color(t.text_dim));
                }
                ui.add_space(6.0);
            }
        },
    );
    ui.separator();
    ui.label(RichText::new(tl!("Add a rule")).font(semibold(12.5)));
    let caret = super::outline::outline(app).get("caret").and_then(Value::as_str).map(str::to_string);
    let fields: Vec<Value> = super::outline::field_list(app).get("fields").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut form = app.speccraft.rule_form.clone();
    if form.cond_para.is_empty()
        && let Some(c) = &caret
    {
        form.cond_para = c.clone();
    }
    ui.add(egui::TextEdit::singleline(&mut form.find).hint_text(tl!("Narrow the paragraph lists")).desired_width(f32::INFINITY));
    ui.horizontal(|ui| {
        ui.label(RichText::new(tl!("When")).small());
        ui.radio_value(&mut form.on_field, false, tl!("a paragraph"));
        ui.add_enabled_ui(!fields.is_empty(), |ui| {
            ui.radio_value(&mut form.on_field, true, tl!("a fill-in"));
        });
    });
    if form.on_field {
        let shown = fields.iter().find(|f| f["id"] == form.cond_field.as_str()).map_or_else(
            || tl!("Choose a fill-in…").to_string(),
            |f| format!("{}  {}", f["id"].as_str().unwrap_or(""), f["original"].as_str().unwrap_or("")),
        );
        craft_ui::ComboBox::from_id_salt("rule_field").selected_text(shown).width(ui.available_width() - 8.0).show_ui(ui, |ui| {
            for f in &fields {
                let id = f["id"].as_str().unwrap_or("").to_string();
                if ui.selectable_label(form.cond_field == id, format!("{id}  {}", f["original"].as_str().unwrap_or(""))).clicked() {
                    form.cond_field = id;
                    form.cond_is.clear();
                }
            }
        });
        let options: Vec<String> = fields
            .iter()
            .find(|f| f["id"] == form.cond_field.as_str())
            .and_then(|f| f["options"].as_array())
            .map(|o| o.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label(RichText::new(tl!("is")).small());
            if options.is_empty() {
                ui.add(egui::TextEdit::singleline(&mut form.cond_is).hint_text(tl!("any value")).desired_width(160.0));
            } else {
                let shown = if form.cond_is.is_empty() { tl!("any value").to_string() } else { form.cond_is.clone() };
                craft_ui::ComboBox::from_id_salt("rule_is").selected_text(shown).show_ui(ui, |ui| {
                    if ui.selectable_label(form.cond_is.is_empty(), tl!("any value")).clicked() {
                        form.cond_is.clear();
                    }
                    for o in &options {
                        if ui.selectable_label(form.cond_is == *o, o).clicked() {
                            form.cond_is = o.clone();
                        }
                    }
                });
            }
        });
    } else {
        pick_paragraph(ui, "rule_cond", &mut form.cond_para, &paras, &form.find);
        ui.horizontal(|ui| {
            ui.label(RichText::new(tl!("is")).small());
            ui.radio_value(&mut form.when_off, false, tl!("on"));
            ui.radio_value(&mut form.when_off, true, tl!("off"));
        });
    }
    craft_ui::ComboBox::from_id_salt("rule_kind").selected_text(tl!(KINDS[form.kind.min(3)].1)).show_ui(ui, |ui| {
        for (i, (_, label)) in KINDS.iter().enumerate() {
            ui.selectable_value(&mut form.kind, i, tl!(label));
        }
    });
    pick_paragraph(ui, "rule_target", &mut form.target, &paras, &form.find);
    ui.add(egui::TextEdit::singleline(&mut form.note).hint_text(tl!("Why (optional, shown with the rule)")).desired_width(f32::INFINITY));
    let ready = !form.target.is_empty() && if form.on_field { !form.cond_field.is_empty() } else { !form.cond_para.is_empty() };
    let mut add = false;
    ui.add_enabled_ui(ready, |ui| {
        add = ui.button(tl!("Add Rule")).clicked();
    });
    if add {
        let cond = if form.on_field {
            if form.cond_is.trim().is_empty() {
                json!({"field": form.cond_field})
            } else {
                json!({"field": form.cond_field, "is": form.cond_is.trim()})
            }
        } else if form.when_off {
            json!({"off": form.cond_para})
        } else {
            json!({"on": form.cond_para})
        };
        let params = json!({"kind": KINDS[form.kind.min(3)].0, "if": [cond], "target": form.target, "note": form.note});
        match app.run("spec.rules.add", params) {
            Ok(_) => {
                form.target.clear();
                form.note.clear();
            }
            Err(e) => app.status(e),
        }
    }
    app.speccraft.rule_form = form;
    if let Some(id) = remove
        && let Err(e) = app.run("spec.rules.remove", json!({"id": id}))
    {
        app.status(e);
    }
    if let Some(id) = goto {
        let block = super::outline::outline(app)
            .get("items")
            .and_then(Value::as_array)
            .and_then(|a| a.iter().find(|i| i["id"] == id.as_str()).and_then(|i| i.get("block")).and_then(Value::as_u64));
        if let Some(b) = block {
            super::outline::go_to(app, b);
        }
    }
}
