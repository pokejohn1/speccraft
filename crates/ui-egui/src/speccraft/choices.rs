//! Choosing in a choice field (M19): one option, several (a multiple choice, printed as a list),
//! an option with a blank to type into, or leaving optional text out. The same list is shown in
//! the Fill-ins view and in the box that opens when a choice is clicked on the page.

use egui::{RichText, Ui};
use serde_json::{Value, json};
use wordcraft_engine::cmd::spec::fields::{self, Field, FieldKind, FieldState};

use crate::theme::Tokens;

/// A choice field as the list needs it.
#[derive(Clone, Debug, Default)]
pub(crate) struct Choice {
    pub id: String,
    pub optional: bool,
    pub options: Vec<String>,
    pub multiple: bool,
    pub chosen: Vec<usize>,
    pub value: String,
    pub open: bool,
    pub omitted: bool,
}

impl Choice {
    /// From a field (`None` for fields that aren't choices).
    pub fn of(f: &Field) -> Option<Choice> {
        matches!(f.kind, FieldKind::Choice | FieldKind::Optional).then(|| Choice {
            id: f.id.clone(),
            optional: f.kind == FieldKind::Optional,
            options: f.options.clone(),
            multiple: f.multiple,
            chosen: f.chosen.clone(),
            value: f.value.clone(),
            open: f.state == FieldState::Open,
            omitted: f.state == FieldState::Omitted,
        })
    }

    /// From a `spec.fields.list` row.
    pub fn from_json(f: &Value) -> Option<Choice> {
        let s = |k: &str| f.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let kind = s("kind");
        if kind != "choice" && kind != "optional" {
            return None;
        }
        let list = |k: &str| f.get(k).and_then(Value::as_array).cloned().unwrap_or_default();
        Some(Choice {
            id: s("id"),
            optional: kind == "optional",
            options: list("options").iter().filter_map(Value::as_str).map(str::to_string).collect(),
            multiple: f.get("multiple").and_then(Value::as_bool).unwrap_or(false),
            chosen: list("chosen").iter().filter_map(Value::as_u64).filter_map(|i| usize::try_from(i).ok()).collect(),
            value: s("value"),
            open: s("state") == "open",
            omitted: s("state") == "omitted",
        })
    }
}

/// The list of options; `fill` holds what is typed into an option's blank. Returns the command
/// to run when something is chosen.
pub(crate) fn list(ui: &mut Ui, t: &Tokens, c: &Choice, fill: &mut String) -> Option<(&'static str, Value)> {
    let mut out = None;
    if c.multiple {
        for (i, o) in c.options.iter().enumerate() {
            let mut on = c.chosen.contains(&i);
            if ui.checkbox(&mut on, RichText::new(o).color(t.blue)).changed() {
                let mut chosen: Vec<usize> = c.chosen.iter().copied().filter(|x| *x != i).collect();
                if on {
                    chosen.push(i);
                }
                chosen.sort_unstable();
                out = Some(("spec.fields.set", json!({"id": c.id, "options": chosen})));
            }
        }
    } else {
        for (i, o) in c.options.iter().enumerate() {
            let picked = !c.open && (c.value == *o || (fields::has_blank(o) && c.chosen.contains(&i)));
            if fields::has_blank(o) {
                ui.horizontal(|ui| {
                    if ui.radio(picked, RichText::new(o).color(t.blue)).clicked() {
                        out = Some(("spec.fields.set", json!({"id": c.id, "option": i})));
                    }
                    let resp = ui.add(egui::TextEdit::singleline(fill).hint_text(tl!("Fill in")).desired_width(120.0));
                    if resp.lost_focus() && !fill.trim().is_empty() {
                        out = Some(("spec.fields.set", json!({"id": c.id, "option": i, "fill": fill.trim()})));
                    }
                });
            } else if ui.radio(picked, RichText::new(o).color(t.blue)).clicked() {
                out = Some(("spec.fields.set", json!({"id": c.id, "option": i})));
            }
        }
        if c.optional && ui.radio(c.omitted, tl!("(leave out)")).clicked() {
            out = Some(("spec.fields.set", json!({"id": c.id, "omit": true})));
        }
    }
    ui.separator();
    ui.horizontal(|ui| {
        if !c.optional {
            let mut several = c.multiple;
            if ui.checkbox(&mut several, RichText::new(tl!("Several can be chosen")).small()).clicked() {
                out = Some(("spec.fields.multiple", json!({"id": c.id, "value": several})));
            }
        }
        if !c.open && ui.small_button(tl!("Master wording")).on_hover_text(tl!("Back to the master's wording")).clicked() {
            out = Some(("spec.fields.set", json!({"id": c.id, "reset": true})));
        }
    });
    out
}
