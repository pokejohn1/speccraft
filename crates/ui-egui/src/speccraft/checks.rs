//! Findings of the quality checks (Speccraft milestone M10). Added by the Speccraft fork; not part
//! of the original WordCraft.
//!
//! The Spec Outline pane's Checks view lists `spec.check` for the section on screen; the
//! Sections pane lists Check Project's findings by section. Each finding takes the author to its
//! paragraph (opening the section first if needed); warnings and notices can be accepted, which
//! records them in the project file so they stop counting.

use egui::{RichText, Ui};
use serde_json::{Value, json};

use crate::WordApp;
use crate::theme::{Tokens, semibold};

/// `spec.check` for the document on screen, cached per revision.
fn section_check(app: &mut WordApp) -> Value {
    let key = (app.session.document_id(), app.session.rev());
    if let Some((k, v)) = &app.speccraft.check_cache
        && *k == key
    {
        return v.clone();
    }
    let v = app.session.run("spec.check", &json!({})).unwrap_or(Value::Null);
    app.speccraft.check_cache = Some((key, v.clone()));
    v
}

/// Move the caret to paragraph `id` of the section on screen.
pub(crate) fn go_to_para(app: &mut WordApp, id: &str) -> Result<(), String> {
    let o = super::outline::outline(app);
    let block = o
        .get("items")
        .and_then(Value::as_array)
        .and_then(|v| v.iter().find(|x| x.get("id").and_then(Value::as_str) == Some(id)))
        .and_then(|x| x.get("block"))
        .and_then(Value::as_u64)
        .ok_or_else(|| crate::i18n::fmt(tl!("Paragraph {id} is not in this section"), &[("id", id)]))?;
    super::outline::go_to(app, block);
    Ok(())
}

fn severity_color(t: &Tokens, sev: &str) -> egui::Color32 {
    match sev {
        "error" => t.red,
        "warning" => t.orange,
        _ => t.text_dim,
    }
}

fn severity_label(sev: &str) -> &'static str {
    match sev {
        "error" => "Error",
        "warning" => "Warning",
        _ => "Notice",
    }
}

/// One finding: severity, paragraph and message; returns whether it was clicked.
fn row(ui: &mut Ui, t: &Tokens, x: &Value, accepted: bool) -> bool {
    let s = |k: &str| x.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let sev = s("severity");
    let color = if accepted { t.text_dim } else { severity_color(t, &sev) };
    let para = s("para");
    let head = if para.is_empty() { tl!(severity_label(&sev)).to_string() } else { format!("{} · {para}", tl!(severity_label(&sev))) };
    let mut msg = RichText::new(format!("{head}: {}", s("message"))).small().color(color);
    if accepted {
        msg = msg.strikethrough();
    }
    let resp = ui.add(egui::Label::new(msg).wrap().sense(egui::Sense::click()));
    let hint = if para.is_empty() { tl!("Open the section") } else { tl!("Go to the paragraph") };
    resp.on_hover_text(hint).clicked()
}

/// The Spec Outline pane's Checks view: the section on screen.
pub(crate) fn section_view(app: &mut WordApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let r = section_check(app);
    let findings = r.get("findings").and_then(Value::as_array).cloned().unwrap_or_default();
    let n = |k: &str| r.get(k).and_then(Value::as_u64).unwrap_or(0).to_string();
    ui.label(
        RichText::new(crate::i18n::fmt(
            tl!("{e} errors, {w} warnings, {n} notices"),
            &[("e", &n("errors")), ("w", &n("warnings")), ("n", &n("notices"))],
        ))
        .font(semibold(12.0)),
    );
    ui.label(
        RichText::new(tl!("What prints in this section, checked as you edit. Check Project also checks fill-ins against the project's values."))
            .small()
            .color(t.text_dim),
    );
    ui.separator();
    let mut goto = None;
    egui::ScrollArea::vertical().id_salt("spec_checks").auto_shrink([false, false]).show(ui, |ui| {
        if findings.is_empty() {
            ui.label(RichText::new(tl!("Nothing to report.")).small().color(t.green));
        }
        for x in &findings {
            if row(ui, &t, x, false)
                && let Some(p) = x.get("para").and_then(Value::as_str)
            {
                goto = Some(p.to_string());
            }
        }
    });
    if let Some(p) = goto
        && let Err(e) = go_to_para(app, &p)
    {
        app.status(e);
    }
}

/// Check Project's findings in the Sections pane, by section, with Accept for warnings and notices.
pub(crate) fn project_report(app: &mut WordApp, ui: &mut Ui, f: &Value, action: &mut Option<(&'static str, Value)>) {
    let t = Tokens::get(ui.ctx());
    let n = |k: &str| f.get(k).and_then(Value::as_u64).unwrap_or(0).to_string();
    let summary =
        crate::i18n::fmt(tl!("Check: {e} errors, {w} warnings, {n} notices"), &[("e", &n("errors")), ("w", &n("warnings")), ("n", &n("notices"))]);
    ui.label(RichText::new(summary).font(semibold(12.0)));
    let acked = f.get("acknowledged").and_then(Value::as_u64).unwrap_or(0);
    ui.horizontal_wrapped(|ui| {
        if acked > 0 {
            let label = crate::i18n::fmt(tl!("Show {n} accepted"), &[("n", &acked.to_string())]);
            ui.checkbox(&mut app.speccraft.show_accepted, label);
        }
        if app.speccraft.publish_blocked
            && ui.small_button(tl!("Publish Anyway")).on_hover_text(tl!("Publish although Check Project found errors")).clicked()
        {
            *action = Some(("ui.spec.publish", json!({"force": true})));
        }
    });
    let all = f.get("findings").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut last_file: Option<String> = None;
    for x in &all {
        let accepted = x.get("acknowledged").and_then(Value::as_bool) == Some(true);
        if accepted && !app.speccraft.show_accepted {
            continue;
        }
        let file = x.get("file").and_then(Value::as_str).map(str::to_string);
        if file != last_file {
            ui.add_space(4.0);
            let head = file.clone().unwrap_or_else(|| tl!("Project").to_string());
            ui.label(RichText::new(head).font(semibold(11.5)).color(t.text_dim));
            last_file = file.clone();
        }
        ui.horizontal(|ui| {
            let sev = x.get("severity").and_then(Value::as_str).unwrap_or("");
            let key = x.get("key").cloned().unwrap_or(Value::Null);
            // The button first, so a long message wraps beside it.
            if sev != "error" {
                let (label, hint, undo) = if accepted {
                    (tl!("Undo"), tl!("Count this finding again"), true)
                } else {
                    (tl!("Accept"), tl!("Accept this finding: it stops counting, and the project file records it"), false)
                };
                if ui.small_button(label).on_hover_text(hint).clicked() {
                    *action = Some(("ui.spec.acknowledge", json!({"key": key, "undo": undo})));
                }
            }
            ui.vertical(|ui| {
                if row(ui, &t, x, accepted)
                    && let Some(file) = &file
                {
                    *action = Some(("ui.spec.goTo", json!({"file": file, "para": x.get("para")})));
                }
                if accepted && let Some(note) = x.get("note").and_then(Value::as_str).filter(|n| !n.is_empty()) {
                    ui.label(RichText::new(note).small().italics().color(t.text_dim));
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wordcraft_doc::{Block, Document, Paragraph};
    use wordcraft_engine::Session;

    use crate::{Services, WordApp};

    fn master() -> Document {
        let p = |s: &str, t: &str| std::sync::Arc::new(Block::Para(Paragraph::with_text(t, Default::default()).styled(s)));
        let mut d = Document::new();
        d.body = vec![
            p("SCT", "SECTION 09 2900 - GYPSUM BOARD"),
            p("PRT", "GENERAL"),
            p("ART", "SUMMARY"),
            p("PR1", "Provide XXX panels."),
            p("EOS", "END OF SECTION 09 2900"),
        ];
        d
    }

    #[test]
    fn findings_lead_to_their_paragraph_and_errors_stop_publishing() {
        let dir = std::env::temp_dir().join(format!("speccraft-ui-checks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("project")).unwrap();
        let m = dir.join("092900.docx");
        wordcraft_engine::io::save_path(&m, &master()).unwrap();
        let mut a = WordApp::new(Session::new(Document::new()), Services::default());
        let project = dir.join("project").join("Riverside.speccraft");
        let path = project.to_string_lossy().to_string();
        a.run("ui.spec.newProject", json!({"path": path, "name": "Riverside"})).unwrap();
        a.run("ui.spec.addSection", json!({"master": m.to_string_lossy()})).unwrap();
        a.run("spec.activate", json!({"ids": ["p0002", "p0003", "p0004", "p0005"]})).unwrap();
        a.run("file.save", json!({})).unwrap();

        // The section's own check finds the editing mark, and takes the caret to it.
        let r = a.run("spec.check", json!({})).unwrap();
        let mark = r["findings"].as_array().unwrap().iter().find(|f| f["code"] == "editingMark").cloned().unwrap();
        assert_eq!(mark["para"], "p0004");
        super::go_to_para(&mut a, "p0004").unwrap();
        assert_eq!(a.session.sel.focus.path.0.first(), Some(&3));

        // An error stops Publish; the report offers Publish Anyway, which publishes.
        a.run("spec.project.requires", json!({"path": path, "file": "092900.docx", "requires": ["07 9200"]})).unwrap();
        assert!(a.run("ui.spec.publish", json!({})).is_err());
        assert!(a.speccraft.publish_blocked);
        assert_eq!(a.speccraft.findings.as_ref().unwrap()["errors"], 1);
        a.run("ui.spec.publish", json!({"force": true})).unwrap();
        assert!(!a.speccraft.publish_blocked);

        // Accepting the warning stops it counting.
        let check = a.run("ui.spec.check", json!({})).unwrap();
        let w = check["findings"].as_array().unwrap().iter().find(|f| f["code"] == "editingMark").cloned().unwrap();
        a.run("ui.spec.acknowledge", json!({"key": w["key"], "note": "intended"})).unwrap();
        let f = a.speccraft.findings.clone().unwrap();
        assert_eq!(f["acknowledged"], 1);
        assert_eq!(f["warnings"], check["warnings"].as_u64().unwrap() - 1);

        // Going to a finding in another section opens it first.
        a.run("file.new", json!({})).ok();
        a.run("ui.spec.goTo", json!({"file": "092900.docx", "para": "p0004"})).unwrap();
        assert!(a.session.path.as_deref().is_some_and(|p| p.ends_with("092900.docx")));
        assert!(a.run("ui.spec.goTo", json!({"file": "092900.docx", "para": "p9999"})).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
