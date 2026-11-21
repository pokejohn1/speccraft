//! The Project Overview pane (Speccraft milestone M12). Added by the Speccraft fork; not part of
//! the original WordCraft.
//!
//! One row per section from `spec.project.overview`: what prints, fill-ins open, Check Project's
//! findings, changes from the master, a newer master, and publishing. Rows can be filtered to what
//! needs attention, and each part links to where it is dealt with: the section itself, its
//! Fill-ins, Checks or Updates view, its changed paragraphs in the outline.

use egui::{RichText, Ui};
use serde_json::{Value, json};

use crate::WordApp;
use crate::theme::{Tokens, regular, semibold};

/// What the pane keeps between frames.
#[derive(Default)]
pub struct OverviewUi {
    pub open: bool,
    /// The last `spec.project.overview`.
    pub(crate) data: Option<Value>,
    /// `all`, `attention`, `master`, `published`, `fillins`, `changed`.
    pub(crate) filter: String,
}

/// The `ui.spec.overview` command: open (or close) the pane, reading the project afresh.
pub(crate) fn command(app: &mut WordApp, p: &Value) -> Result<Value, String> {
    let open = p.get("value").and_then(Value::as_bool).unwrap_or(true);
    app.speccraft.overview.open = open;
    if let Some(f) = p.get("filter").and_then(Value::as_str) {
        app.speccraft.overview.filter = f.to_string();
    }
    if !open {
        return Ok(json!({"open": false}));
    }
    refresh(app)
}

fn refresh(app: &mut WordApp) -> Result<Value, String> {
    let path = app.speccraft.project.clone().ok_or_else(|| tl!("Open a project first").to_string())?;
    // It reads the section files: open sections with changes are saved first.
    super::tabs::save_sections(app)?;
    if app.session.dirty && app.session.path.as_deref().is_some_and(|p| p.starts_with(path.parent().unwrap_or(&path))) {
        app.run("file.save", json!({}))?;
    }
    let r = app.run("spec.project.overview", json!({"path": path.to_string_lossy()}))?;
    app.speccraft.overview.data = Some(r.clone());
    Ok(r)
}

fn keep(filter: &str, row: &Value) -> bool {
    let n = |k: &str| row.get(k).and_then(Value::as_u64).unwrap_or(0);
    match filter {
        "attention" => row["needsAttention"] == true,
        "master" => row["newerMaster"] == true,
        "published" => row["included"] == true && row["published"] != "published",
        "fillins" => n("fillInsOpen") > 0,
        "changed" => n("modified") + n("added") + n("deleted") > 0,
        _ => true,
    }
}

fn filter_label(f: &str) -> &'static str {
    match f {
        "attention" => tl!("Needs attention"),
        "master" => tl!("Newer master"),
        "published" => tl!("Not published or out of date"),
        "fillins" => tl!("Fill-ins open"),
        "changed" => tl!("Changed from master"),
        _ => tl!("All sections"),
    }
}

/// The pane.
pub(crate) fn pane(app: &mut WordApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    if app.speccraft.project.is_none() {
        ui.label(RichText::new(tl!("Open a project to see where its sections stand.")).small().color(t.text_dim));
        return;
    }
    if app.speccraft.overview.data.is_none()
        && let Err(e) = refresh(app)
    {
        app.status(e);
    }
    let Some(o) = app.speccraft.overview.data.clone() else { return };
    let mut action: Option<(&'static str, Value)> = None;
    let n = |k: &str| o.get(k).and_then(Value::as_u64).unwrap_or(0).to_string();
    ui.label(RichText::new(o.get("project").and_then(Value::as_str).unwrap_or("")).font(semibold(13.5)));
    ui.label(
        RichText::new(crate::i18n::fmt(
            tl!("{n} sections included · {a} need attention · {e} errors, {w} warnings"),
            &[("n", &n("included")), ("a", &n("needsAttention")), ("e", &n("errors")), ("w", &n("warnings"))],
        ))
        .small()
        .color(t.text_dim),
    );
    ui.horizontal_wrapped(|ui| {
        let filter = app.speccraft.overview.filter.clone();
        craft_ui::ComboBox::from_id_salt("spec_overview_filter").selected_text(filter_label(&filter)).show_ui(ui, |ui| {
            for f in ["all", "attention", "master", "published", "fillins", "changed"] {
                if ui.selectable_label(filter == f || (filter.is_empty() && f == "all"), filter_label(f)).clicked() {
                    app.speccraft.overview.filter = f.to_string();
                }
            }
        });
        if ui.small_button(tl!("Refresh")).on_hover_text(tl!("Read the project and its sections again")).clicked() {
            action = Some(("ui.spec.overview", json!({})));
        }
    });
    // Findings of the project itself (required sections missing, rules between sections).
    let own = o.get("projectFindings").and_then(Value::as_array).cloned().unwrap_or_default();
    for x in own.iter().filter(|x| x.get("acknowledged").is_none()).take(20) {
        let sev = x.get("severity").and_then(Value::as_str).unwrap_or("");
        let color = if sev == "error" {
            t.red
        } else if sev == "warning" {
            t.orange
        } else {
            t.text_dim
        };
        ui.label(RichText::new(x.get("message").and_then(Value::as_str).unwrap_or("")).small().color(color));
    }
    ui.separator();
    let filter = app.speccraft.overview.filter.clone();
    let rows: Vec<Value> =
        o.get("sections").and_then(Value::as_array).map(|v| v.iter().filter(|r| keep(&filter, r)).cloned().collect()).unwrap_or_default();
    egui::ScrollArea::vertical().id_salt("spec_overview").auto_shrink([false, false]).show(ui, |ui| {
        if rows.is_empty() {
            ui.label(RichText::new(tl!("No section matches.")).small().color(t.green));
        }
        for r in &rows {
            section_row(ui, &t, r, &mut action);
            ui.add_space(6.0);
        }
    });
    if let Some((id, args)) = action
        && let Err(e) = app.run(id, args)
    {
        app.status(e);
    }
}

/// One section: its name and state, then what needs doing, each part a link.
fn section_row(ui: &mut Ui, t: &Tokens, r: &Value, action: &mut Option<(&'static str, Value)>) {
    let s = |k: &str| r.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let n = |k: &str| r.get(k).and_then(Value::as_u64).unwrap_or(0);
    let file = s("file");
    let included = r["included"] == true;
    let go = |view: Option<&str>| {
        let mut a = json!({"file": file});
        if let Some(v) = view {
            a["view"] = json!(v);
        }
        ("ui.spec.goTo", a)
    };
    ui.horizontal(|ui| {
        let name = format!("{}  {}", s("number"), super::title_case(&s("title")));
        let mut text = RichText::new(name).font(semibold(12.5));
        if !included {
            text = text.color(t.text_dim);
        }
        if ui.link(text).on_hover_text(tl!("Open this section")).clicked() {
            *action = Some(go(None));
        }
        let (badge, color) = if !included {
            (tl!("left out"), t.text_dim)
        } else if r["needsAttention"] == true {
            (tl!("needs attention"), t.orange)
        } else {
            (tl!("ready"), t.green)
        };
        ui.label(RichText::new(badge).small().color(color));
    });
    if r.get("missing").is_some() {
        ui.label(RichText::new(tl!("The section file is missing.")).small().color(t.red));
        return;
    }
    if !included {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let small = |x: String| RichText::new(x).font(regular(11.5));
        ui.label(
            small(crate::i18n::fmt(tl!("prints {p} of {a}"), &[("p", &n("printing").to_string()), ("a", &n("paragraphs").to_string())]))
                .color(t.text_dim),
        );
        let open = n("fillInsOpen");
        let open_text =
            if open == 1 { tl!("1 fill-in open").to_string() } else { crate::i18n::fmt(tl!("{n} fill-ins open"), &[("n", &open.to_string())]) };
        if open > 0 && ui.link(small(open_text).color(t.orange)).clicked() {
            *action = Some(go(Some("fields")));
        }
        let (e, w) = (n("errors"), n("warnings"));
        if e + w > 0 {
            let mut parts = Vec::new();
            match e {
                0 => {}
                1 => parts.push(tl!("1 error").to_string()),
                _ => parts.push(crate::i18n::fmt(tl!("{n} errors"), &[("n", &e.to_string())])),
            }
            match w {
                0 => {}
                1 => parts.push(tl!("1 warning").to_string()),
                _ => parts.push(crate::i18n::fmt(tl!("{n} warnings"), &[("n", &w.to_string())])),
            }
            let text = parts.join(", ");
            if ui.link(small(text).color(if e > 0 { t.red } else { t.orange })).on_hover_text(tl!("See the section's checks")).clicked() {
                *action = Some(go(Some("checks")));
            }
        }
        let changed = n("modified") + n("added") + n("deleted");
        if changed > 0 {
            let text = crate::i18n::fmt(
                tl!("{m} changed, {a} added, {d} deleted from master"),
                &[("m", &n("modified").to_string()), ("a", &n("added").to_string()), ("d", &n("deleted").to_string())],
            );
            if ui.link(small(text).color(t.text_dim)).on_hover_text(tl!("See the changed paragraphs in the outline")).clicked() {
                *action = Some(go(Some("outline")));
            }
        }
        if r["newerMaster"] == true
            && ui.link(small(tl!("newer master").to_string()).color(t.orange)).on_hover_text(tl!("See what changed in the master")).clicked()
        {
            *action = Some(go(Some("updates")));
        }
        let published = s("published");
        let (label, color) = match published.as_str() {
            "published" => (tl!("published"), t.green),
            "out of date" => (tl!("published, changed since"), t.orange),
            _ => (tl!("not published"), t.text_dim),
        };
        ui.label(small(label.to_string()).color(color));
        if let Some(date) = r.get("lastUpdate").and_then(Value::as_str) {
            let day = date.get(..10).unwrap_or(date);
            ui.label(small(crate::i18n::fmt(tl!("master update {d}"), &[("d", day)])).color(t.text_dim));
        }
    });
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wordcraft_doc::{Block, Document, Paragraph};
    use wordcraft_engine::Session;

    use crate::{Services, WordApp};

    fn master(number: &str, title: &str) -> Document {
        let p = |s: &str, t: &str| std::sync::Arc::new(Block::Para(Paragraph::with_text(t, Default::default()).styled(s)));
        let mut d = Document::new();
        d.body = vec![
            p("SCT", &format!("SECTION {number} - {title}")),
            p("PRT", "GENERAL"),
            p("ART", "SUMMARY"),
            p("PR1", "Provide [Type X] [Type C] board."),
            p("EOS", "END OF SECTION"),
        ];
        d
    }

    #[test]
    fn the_overview_lists_sections_and_filters_them() {
        let dir = std::env::temp_dir().join(format!("speccraft-ui-overview-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("project")).unwrap();
        let mut a = WordApp::new(Session::new(Document::new()), Services::default());
        assert!(a.run("ui.spec.overview", json!({})).is_err(), "no project yet");
        let project = dir.join("project").join("P.speccraft");
        a.run("ui.spec.newProject", json!({"path": project.to_string_lossy(), "name": "P"})).unwrap();
        let m = dir.join("092900.docx");
        wordcraft_engine::io::save_path(&m, &master("09 2900", "GYPSUM BOARD")).unwrap();
        a.run("ui.spec.addSection", json!({"master": m.to_string_lossy()})).unwrap();
        a.run("spec.activate", json!({"ids": ["p0002", "p0003", "p0004"]})).unwrap();
        // The unsaved change is saved first, so the overview sees it.
        let r = a.run("ui.spec.overview", json!({"filter": "fillins"})).unwrap();
        assert!(a.speccraft.overview.open);
        assert_eq!(r["sections"][0]["fillInsOpen"], 1, "{r}");
        assert_eq!(r["sections"][0]["printing"], 5, "{r}");
        assert!(super::keep("fillins", &r["sections"][0]));
        assert!(!super::keep("master", &r["sections"][0]));
        a.run("ui.spec.overview", json!({"value": false})).unwrap();
        assert!(!a.speccraft.overview.open);
        // M16: publishing in the short form, without what is left out of it.
        a.run("spec.short.set", json!({"id": "p0004", "leave": true})).unwrap();
        a.speccraft.publish_form = wordcraft_engine::cmd::spec::forms::Form::Short;
        let r = a.run("ui.spec.publish", json!({"force": true, "format": "docx"})).unwrap();
        let file = r["sections"][0]["docx"].as_str().unwrap().to_string();
        assert!(file.ends_with("092900 (Short Form).docx"), "{file}");
        let back = wordcraft_engine::io::open_path(std::path::Path::new(&file)).unwrap();
        assert!(!back.plain_text(wordcraft_doc::StoryRef::Body).contains("Provide"), "left out of the short form");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
