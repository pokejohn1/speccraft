//! Rules between sections, in the Sections pane (wish W3). Added by the Speccraft fork; not part
//! of the original WordCraft.
//!
//! The project's rules (M6, `spec.project.addRule` / `removeRule`) listed in words, with what the
//! last Check Project said about each, and a form to add one: when a section is in the project
//! (optionally: when one of its paragraphs prints) or a project value is set, a section (or one of
//! its paragraphs) is required, recommended, warned about or excluded.

use egui::{RichText, Ui};
use serde_json::{Value, json};

use crate::WordApp;
use crate::theme::Tokens;

/// The add form, as typed.
#[derive(Clone, Debug)]
pub struct ProjectRuleForm {
    pub kind: String,
    /// `section` or `value`.
    pub when: String,
    pub section: String,
    pub on: String,
    pub value: String,
    pub is: String,
    pub target: String,
    pub target_on: String,
    pub note: String,
}

impl Default for ProjectRuleForm {
    fn default() -> Self {
        ProjectRuleForm {
            kind: "require".into(),
            when: "section".into(),
            section: String::new(),
            on: String::new(),
            value: String::new(),
            is: String::new(),
            target: String::new(),
            target_on: String::new(),
            note: String::new(),
        }
    }
}

fn kind_label(k: &str) -> &'static str {
    match k {
        "recommend" => tl!("recommends"),
        "warn" => tl!("warns about"),
        "exclude" => tl!("excludes"),
        _ => tl!("requires"),
    }
}

/// A rule in words: "If 09 2900 (p0002 prints) is in the project, it requires 07 9200."
fn describe(r: &Value) -> String {
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let part = |v: &Value| {
        if !s(v, "value").is_empty() {
            if s(v, "is").is_empty() {
                crate::i18n::fmt(tl!("{name} is set"), &[("name", &s(v, "value"))])
            } else {
                format!("{} = {}", s(v, "value"), s(v, "is"))
            }
        } else if s(v, "on").is_empty() {
            crate::i18n::fmt(tl!("Section {n}"), &[("n", &s(v, "section"))])
        } else {
            crate::i18n::fmt(tl!("Section {n} paragraph {p}"), &[("n", &s(v, "section")), ("p", &s(v, "on"))])
        }
    };
    let when: Vec<String> = r.get("if").and_then(Value::as_array).map(|v| v.iter().map(part).collect()).unwrap_or_default();
    let target = r.get("target").map(part).unwrap_or_default();
    let kind = s(r, "kind");
    crate::i18n::fmt(tl!("If {when}: {kind} {target}"), &[("when", &when.join(tl!(" and "))), ("kind", kind_label(&kind)), ("target", &target)])
}

/// The rules part of the Sections pane.
pub(crate) fn show(app: &mut WordApp, ui: &mut Ui, info: &Value) {
    let t = Tokens::get(ui.ctx());
    let rules = info.get("rules").and_then(Value::as_array).cloned().unwrap_or_default();
    let title = crate::i18n::fmt(tl!("Rules Between Sections ({n})"), &[("n", &rules.len().to_string())]);
    let mut action: Option<(&'static str, Value)> = None;
    egui::CollapsingHeader::new(RichText::new(title).small()).id_salt("spec_project_rules").default_open(false).show(ui, |ui| {
        // What the last Check Project said, by rule.
        let findings = app.speccraft.findings.as_ref().and_then(|f| f.get("findings")).and_then(Value::as_array).cloned().unwrap_or_default();
        for r in &rules {
            let id = r.get("id").and_then(Value::as_str).unwrap_or("").to_string();
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&id).small().color(t.text_dim));
                ui.label(RichText::new(describe(r)).small());
                if ui.small_button("x").on_hover_text(tl!("Remove this rule")).clicked() {
                    action = Some(("ui.spec.removeProjectRule", json!({"id": id})));
                }
            });
            if let Some(note) = r.get("note").and_then(Value::as_str).filter(|n| !n.is_empty()) {
                ui.label(RichText::new(note).small().italics().color(t.text_dim));
            }
            for f in findings.iter().filter(|f| f.get("rule").and_then(Value::as_str) == Some(id.as_str())) {
                let color = if f["severity"] == "error" {
                    t.red
                } else if f["severity"] == "warning" {
                    t.orange
                } else {
                    t.text_dim
                };
                ui.label(RichText::new(f.get("message").and_then(Value::as_str).unwrap_or("")).small().color(color));
            }
        }
        if rules.is_empty() {
            ui.label(
                RichText::new(tl!(
                    "No rules yet. A rule ties sections together: one section in the project can require, recommend, warn about or exclude another."
                ))
                .small()
                .color(t.text_dim),
            );
        }
        ui.separator();
        form(app, ui, info, &mut action);
    });
    if let Some((id, args)) = action
        && let Err(e) = app.run(id, args)
    {
        app.status(e);
    }
}

/// The add form.
fn form(app: &mut WordApp, ui: &mut Ui, info: &Value, action: &mut Option<(&'static str, Value)>) {
    let sections: Vec<(String, String)> = info
        .get("sections")
        .and_then(Value::as_array)
        .map(|v| {
            v.iter()
                .map(|s| (s["number"].as_str().unwrap_or("").to_string(), super::title_case(s["title"].as_str().unwrap_or(""))))
                .filter(|(n, _)| !n.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let values: Vec<String> = info.get("values").and_then(Value::as_object).map(|m| m.keys().cloned().collect()).unwrap_or_default();
    let f = &mut app.speccraft.project_rule_form;
    ui.label(RichText::new(tl!("Add a rule")).small().strong());
    let pick_section = |ui: &mut Ui, salt: &str, current: &mut String| {
        ui.horizontal(|ui| {
            craft_ui::ComboBox::from_id_salt(salt)
                .selected_text(if current.is_empty() { tl!("Section…").to_string() } else { current.clone() })
                .width(150.0)
                .show_ui(ui, |ui| {
                    for (n, title) in &sections {
                        if ui.selectable_label(current == n, format!("{n}  {title}")).clicked() {
                            *current = n.clone();
                        }
                    }
                });
            ui.add(egui::TextEdit::singleline(current).hint_text(tl!("or type a number")).desired_width(90.0));
        });
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(tl!("If")).small());
        craft_ui::ComboBox::from_id_salt("spec_prule_when")
            .selected_text(if f.when == "value" { tl!("a project value") } else { tl!("a section is in the project") })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut f.when, "section".to_string(), tl!("a section is in the project"));
                ui.selectable_value(&mut f.when, "value".to_string(), tl!("a project value"));
            });
    });
    if f.when == "value" {
        ui.horizontal(|ui| {
            craft_ui::ComboBox::from_id_salt("spec_prule_value")
                .selected_text(if f.value.is_empty() { tl!("Value…").to_string() } else { f.value.clone() })
                .width(130.0)
                .show_ui(ui, |ui| {
                    for v in &values {
                        ui.selectable_value(&mut f.value, v.clone(), v);
                    }
                });
            ui.add(egui::TextEdit::singleline(&mut f.is).hint_text(tl!("is (blank: is set)")).desired_width(110.0));
        });
    } else {
        pick_section(ui, "spec_prule_section", &mut f.section);
        ui.add(egui::TextEdit::singleline(&mut f.on).hint_text(tl!("and its paragraph prints (id, optional)")).desired_width(f32::INFINITY));
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new(tl!("then it")).small());
        craft_ui::ComboBox::from_id_salt("spec_prule_kind").selected_text(kind_label(&f.kind)).show_ui(ui, |ui| {
            for k in ["require", "recommend", "warn", "exclude"] {
                ui.selectable_value(&mut f.kind, k.to_string(), kind_label(k));
            }
        });
    });
    pick_section(ui, "spec_prule_target", &mut f.target);
    ui.add(egui::TextEdit::singleline(&mut f.target_on).hint_text(tl!("its paragraph (id, optional)")).desired_width(f32::INFINITY));
    ui.add(egui::TextEdit::singleline(&mut f.note).hint_text(tl!("Note (why)")).desired_width(f32::INFINITY));
    let ready = !f.target.trim().is_empty() && if f.when == "value" { !f.value.trim().is_empty() } else { !f.section.trim().is_empty() };
    if ui.add_enabled(ready, egui::Button::new(tl!("Add Rule")).small()).clicked() {
        let opt = |s: &str| (!s.trim().is_empty()).then(|| s.trim().to_string());
        let cond = if f.when == "value" {
            json!({"value": f.value.trim(), "is": opt(&f.is)})
        } else {
            json!({"section": f.section.trim(), "on": opt(&f.on)})
        };
        let target = json!({"section": f.target.trim(), "on": opt(&f.target_on)});
        *action = Some(("ui.spec.addProjectRule", json!({"kind": f.kind, "if": [cond], "target": target, "note": f.note.trim()})));
    }
}

/// `ui.spec.addProjectRule` / `ui.spec.removeProjectRule`: change the project's rules, then show
/// the project again (and what Check Project says now).
pub(crate) fn command(app: &mut WordApp, id: &str, p: &Value) -> Result<Value, String> {
    let path = app.speccraft.project.clone().ok_or_else(|| tl!("Open a project first").to_string())?;
    let mut args = p.clone();
    args["path"] = json!(path.to_string_lossy());
    // Nulls for optional parts are left out.
    if let Some(list) = args.get_mut("if").and_then(Value::as_array_mut) {
        for c in list.iter_mut() {
            if let Some(o) = c.as_object_mut() {
                o.retain(|_, v| !v.is_null());
            }
        }
    }
    if let Some(o) = args.get_mut("target").and_then(Value::as_object_mut) {
        o.retain(|_, v| !v.is_null());
    }
    let engine = if id == "ui.spec.addProjectRule" { "spec.project.addRule" } else { "spec.project.removeRule" };
    let r = app.run(engine, args)?;
    if id == "ui.spec.addProjectRule" {
        app.speccraft.project_rule_form = ProjectRuleForm::default();
    }
    let info = app.run("spec.project.info", json!({"path": path.to_string_lossy()}))?;
    app.speccraft.info = Some(info);
    if let Ok(check) = app.run("spec.project.validate", json!({"path": path.to_string_lossy()})) {
        app.speccraft.findings = Some(check);
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wordcraft_doc::{Block, Document, Paragraph};
    use wordcraft_engine::Session;

    use crate::{Services, WordApp};

    #[test]
    fn rules_between_sections_are_added_described_checked_and_removed() {
        let dir = std::env::temp_dir().join(format!("speccraft-ui-prules-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("project")).unwrap();
        let mut a = WordApp::new(Session::new(Document::new()), Services::default());
        let project = dir.join("project").join("P.speccraft");
        a.run("ui.spec.newProject", json!({"path": project.to_string_lossy(), "name": "P"})).unwrap();
        let p = |s: &str, t: &str| std::sync::Arc::new(Block::Para(Paragraph::with_text(t, Default::default()).styled(s)));
        let mut d = Document::new();
        d.body = vec![p("SCT", "SECTION 09 2900 - GYPSUM BOARD"), p("PRT", "GENERAL"), p("EOS", "END OF SECTION")];
        wordcraft_engine::io::save_path(&dir.join("m.docx"), &d).unwrap();
        a.run("ui.spec.addSection", json!({"master": dir.join("m.docx").to_string_lossy()})).unwrap();
        let r = a.run("ui.spec.addProjectRule", json!({"kind": "require", "if": [{"section": "09 2900", "on": null}], "target": {"section": "07 9200", "on": null}, "note": "sealant at joints"})).unwrap();
        let id = r["added"].as_str().unwrap().to_string();
        let rules = a.speccraft.info.as_ref().unwrap()["rules"].clone();
        assert_eq!(rules.as_array().unwrap().len(), 1);
        assert_eq!(super::describe(&rules[0]), "If Section 09 2900: requires Section 07 9200");
        // Check Project's say on it is kept for the list.
        let f = a.speccraft.findings.as_ref().unwrap();
        assert!(f["findings"].as_array().unwrap().iter().any(|x| x["rule"] == id.as_str() && x["severity"] == "error"), "{f}");
        a.run("ui.spec.removeProjectRule", json!({"id": id})).unwrap();
        assert_eq!(a.speccraft.info.as_ref().unwrap()["rules"].as_array().map_or(0, Vec::len), 0);
        assert!(a.run("ui.spec.addProjectRule", json!({"kind": "sometimes", "if": [], "target": {}})).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
