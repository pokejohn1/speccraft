//! Section tabs (Speccraft milestone M15). Added by the Speccraft fork; not part of the original
//! WordCraft.
//!
//! Sections open as tabs above the page, each keeping its own place, selection, undo and unsaved
//! changes while another is shown. Tabs are the session's open documents; switching shows the
//! other document in this window ([`WordApp::show_here`]), so no window opens. At most
//! [`MAX_TABS`] stay open: opening one more saves and closes the tab used least recently. A tab
//! shows its section number and title, a dot while it has unsaved changes and an arrow when
//! Check Project found a newer master. "Go to Section" follows a reference such as
//! "Section 07 9200" in the paragraph at the caret to that section's tab.

use std::path::{Path, PathBuf};

use egui::{RichText, Ui};
use serde_json::{Value, json};

use crate::WordApp;
use crate::theme::{Tokens, regular};

/// Most section tabs open at once.
pub const MAX_TABS: usize = 8;

/// The open documents, as the session lists them: (id, path, dirty).
fn open_docs(app: &WordApp) -> Vec<(u64, Option<PathBuf>, bool)> {
    app.session.documents().into_iter().map(|d| (d.id, d.path, d.dirty)).collect()
}

/// The open document showing `path`, if any.
pub(crate) fn tab_for(app: &WordApp, path: &Path) -> Option<u64> {
    open_docs(app).into_iter().find(|(_, p, _)| p.as_deref() == Some(path)).map(|(id, _, _)| id)
}

/// Remember `id` as the tab used last.
fn touch(app: &mut WordApp, id: u64) {
    let order = &mut app.speccraft.tab_order;
    order.retain(|x| *x != id);
    order.push(id);
}

/// Show tab `id` in this window.
pub(crate) fn switch(app: &mut WordApp, id: u64) -> Result<(), String> {
    if !app.session.is_open(id) {
        return Err(tl!("That tab is no longer open").to_string());
    }
    app.show_here(id);
    touch(app, id);
    Ok(())
}

/// Save document `id` if it has unsaved changes and a file, without showing it.
fn save_quietly(app: &mut WordApp, id: u64) -> Result<(), String> {
    let shown = app.session.document_id();
    let Some((_, Some(_), true)) = open_docs(app).into_iter().find(|(d, _, _)| *d == id) else { return Ok(()) };
    app.session.switch_document(id).map_err(|e| e.to_string())?;
    let r = app.session.run("file.save", &json!({})).map(|_| ()).map_err(|e| e.to_string());
    if let Err(e) = app.session.switch_document(shown) {
        log::error!("tabs: couldn't show document {shown} again: {e}");
    }
    r
}

/// Save every open section of the project that has unsaved changes (before Check, Publish, the
/// manual: they read the files).
pub(crate) fn save_sections(app: &mut WordApp) -> Result<(), String> {
    let Some(folder) = app.speccraft.project.as_ref().and_then(|p| p.parent()).map(Path::to_path_buf) else { return Ok(()) };
    let dirty: Vec<u64> =
        open_docs(app).into_iter().filter(|(_, p, d)| *d && p.as_deref().is_some_and(|p| p.starts_with(&folder))).map(|(id, _, _)| id).collect();
    for id in dirty {
        save_quietly(app, id)?;
    }
    Ok(())
}

/// Close tab `id`: saved first if it changed. The last tab stays.
pub(crate) fn close(app: &mut WordApp, id: u64) -> Result<(), String> {
    let docs = open_docs(app);
    if docs.len() <= 1 {
        return Err(tl!("The last tab stays open").to_string());
    }
    save_quietly(app, id)?;
    if app.session.document_id() == id {
        let next = app
            .speccraft
            .tab_order
            .iter()
            .rev()
            .copied()
            .find(|x| *x != id && app.session.is_open(*x))
            .or_else(|| docs.iter().map(|d| d.0).find(|x| *x != id));
        if let Some(next) = next {
            switch(app, next)?;
        }
    }
    app.session.close_document(id).map_err(|e| e.to_string())?;
    app.parked.remove(&id);
    app.speccraft.tab_order.retain(|x| *x != id);
    Ok(())
}

/// Open the section file at `path` in a tab of its own (or show its tab). An untouched blank
/// document is replaced; past [`MAX_TABS`] the tab used least recently is saved and closed.
pub(crate) fn open(app: &mut WordApp, path: &Path) -> Result<bool, String> {
    if let Some(id) = tab_for(app, path) {
        switch(app, id)?;
        return Ok(true);
    }
    let doc = wordcraft_engine::io::open_path(path)?;
    let blank = app.session.path.is_none() && !app.session.dirty && app.session.documents().len() == 1;
    if blank {
        app.replace_startup_document(doc);
        app.session.path = Some(path.to_path_buf());
        let id = app.session.document_id();
        touch(app, id);
        return Ok(false);
    }
    while open_docs(app).len() >= MAX_TABS {
        let active = app.session.document_id();
        let oldest = app.speccraft.tab_order.iter().copied().find(|x| *x != active && app.session.is_open(*x));
        let oldest = oldest.or_else(|| open_docs(app).into_iter().map(|d| d.0).find(|x| *x != active));
        match oldest {
            Some(id) => close(app, id)?,
            None => break,
        }
    }
    let id = app.session.add_document(doc);
    app.session.path = Some(path.to_path_buf());
    // The window follows the new document here, not in a window of its own.
    app.show_here(id);
    touch(app, id);
    Ok(false)
}

/// Section number and title for a tab, from the project's list; else the file name.
fn label(app: &WordApp, path: Option<&Path>, title: &str) -> String {
    let file = path.and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned());
    let known = file.as_ref().and_then(|f| {
        app.speccraft
            .info
            .as_ref()?
            .get("sections")?
            .as_array()?
            .iter()
            .find(|s| s.get("file").and_then(Value::as_str).is_some_and(|x| x.eq_ignore_ascii_case(f)))
            .cloned()
    });
    match known {
        Some(s) => {
            let n = s.get("number").and_then(Value::as_str).unwrap_or("");
            let t = super::title_case(s.get("title").and_then(Value::as_str).unwrap_or(""));
            format!("{n} {t}").trim().to_string()
        }
        None => file.unwrap_or_else(|| title.to_string()),
    }
}

/// Files Check Project found a newer master for.
fn newer_masters(app: &WordApp) -> Vec<String> {
    app.speccraft
        .findings
        .as_ref()
        .and_then(|f| f.get("findings"))
        .and_then(Value::as_array)
        .map(|v| {
            v.iter()
                .filter(|x| x.get("code").and_then(Value::as_str) == Some("masterChanged"))
                .filter_map(|x| x.get("file").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// The tab strip above the page, while a project is open.
pub(crate) fn strip(app: &mut WordApp, ui: &mut Ui) {
    if app.speccraft.project.is_none() {
        return;
    }
    // Only section tabs: a document opened the usual way has a window of its own.
    let tabs = &app.speccraft.tab_order;
    let docs: Vec<_> = app.session.documents().into_iter().filter(|d| tabs.contains(&d.id)).collect();
    if docs.is_empty() {
        return;
    }
    let t = Tokens::get(ui.ctx());
    let newer = newer_masters(app);
    let mut action: Option<(&'static str, Value)> = None;
    egui::Panel::top("spec_tabs").frame(egui::Frame::NONE.fill(t.ribbon).inner_margin(egui::Margin::symmetric(6, 3))).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            for d in &docs {
                let name = label(app, d.path.as_deref(), &d.title);
                let file = d.path.as_deref().and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let mut text = name.clone();
                if d.dirty {
                    text.push_str(" •");
                }
                if newer.iter().any(|f| f.eq_ignore_ascii_case(&file)) {
                    text.push_str(" ↑");
                }
                let rich = RichText::new(text).font(regular(12.0));
                let mut hover = d.path.as_ref().map_or_else(|| d.title.clone(), |p| p.to_string_lossy().into_owned());
                if d.dirty {
                    hover.push('\n');
                    hover.push_str(tl!("Unsaved changes"));
                }
                if newer.iter().any(|f| f.eq_ignore_ascii_case(&file)) {
                    hover.push('\n');
                    hover.push_str(tl!("Its master has changed: see Master Updates"));
                }
                if ui.selectable_label(d.active, rich).on_hover_text(hover).clicked() && !d.active {
                    action = Some(("ui.spec.tab", json!({"id": d.id})));
                }
                if docs.len() > 1 && ui.small_button("x").on_hover_text(tl!("Close this tab (saved first)")).clicked() {
                    action = Some(("ui.spec.closeTab", json!({"id": d.id})));
                }
                ui.add_space(6.0);
            }
        });
    });
    if let Some((id, args)) = action
        && let Err(e) = app.run(id, args)
    {
        app.status(e);
    }
}

/// The section a reference in the paragraph at the caret names ("Section 07 9200"), as
/// (number, the project's file for it if it has one).
pub(crate) fn reference_at_caret(app: &WordApp) -> Option<(String, Option<String>)> {
    let focus = &app.session.sel.focus;
    let text = app.session.doc.para_at(focus).map(|p| p.plain_text())?;
    let refs = wordcraft_engine::cmd::spec::project::referenced_sections(&text);
    let key = |n: &str| n.chars().filter(char::is_ascii_digit).collect::<String>();
    let sections = app.speccraft.info.as_ref().and_then(|i| i.get("sections")).and_then(Value::as_array).cloned().unwrap_or_default();
    let file_of = |n: &str| {
        sections
            .iter()
            .find(|s| s.get("number").and_then(Value::as_str).is_some_and(|x| key(x) == key(n)))
            .and_then(|s| s.get("file").and_then(Value::as_str).map(str::to_string))
    };
    // The first one the project has; else the first one.
    refs.iter().find(|n| file_of(n).is_some()).or_else(|| refs.first()).map(|n| (n.clone(), file_of(n)))
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wordcraft_doc::{Block, Document, Paragraph};
    use wordcraft_engine::Session;

    use crate::{Services, WordApp};

    fn master(number: &str, title: &str, extra: &str) -> Document {
        let p = |s: &str, t: &str| std::sync::Arc::new(Block::Para(Paragraph::with_text(t, Default::default()).styled(s)));
        let mut d = Document::new();
        d.body = vec![
            p("SCT", &format!("SECTION {number} - {title}")),
            p("PRT", "GENERAL"),
            p("ART", "SUMMARY"),
            p("PR1", extra),
            p("EOS", "END OF SECTION"),
        ];
        d
    }

    #[test]
    fn sections_open_in_tabs_that_keep_their_state() {
        let dir = std::env::temp_dir().join(format!("speccraft-ui-tabs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("project")).unwrap();
        let mut a = WordApp::new(Session::new(Document::new()), Services::default());
        let project = dir.join("project").join("P.speccraft");
        a.run("ui.spec.newProject", json!({"path": project.to_string_lossy(), "name": "P"})).unwrap();
        for (n, t, x) in [
            ("09 2900", "GYPSUM BOARD", "See Section 07 9200 for sealants."),
            ("07 9200", "JOINT SEALANTS", "Provide sealant."),
            ("08 7100", "DOOR HARDWARE", "Provide hinges."),
        ] {
            let m = dir.join(format!("{}.docx", n.replace(' ', "")));
            wordcraft_engine::io::save_path(&m, &master(n, t, x)).unwrap();
            a.run("ui.spec.addSection", json!({"master": m.to_string_lossy()})).unwrap();
        }
        // Adding a section opens it in a tab.
        assert_eq!(a.session.documents().len(), 3, "three tabs");
        a.run("ui.spec.closeTab", json!({})).unwrap();
        a.run("ui.spec.openSection", json!({"file": "092900.docx"})).unwrap();
        let gypsum = a.session.document_id();
        a.run("ui.spec.closeTab", json!({"id": a.session.documents().iter().find(|d| d.id != gypsum).unwrap().id})).unwrap();
        assert_eq!(a.session.documents().len(), 1);
        // An unsaved change stays with its tab while another is shown.
        a.run("spec.activate", json!({"ids": ["p0002"]})).unwrap();
        assert!(a.session.dirty);
        a.run("ui.spec.openSection", json!({"file": "079200.docx"})).unwrap();
        let sealants = a.session.document_id();
        assert_ne!(gypsum, sealants);
        assert_eq!(a.session.documents().len(), 2, "two tabs");
        assert!(a.session.documents().iter().any(|d| d.id == gypsum && d.dirty), "the change waits in its tab, unsaved");
        a.run("ui.spec.tab", json!({"id": gypsum})).unwrap();
        assert_eq!(a.session.document_id(), gypsum);
        assert!(a.session.dirty);
        // Switching tabs opens no windows: the sections waiting in tabs belong to this one.
        a.run("ui.spec.tab", json!({"id": sealants})).unwrap();
        a.run("ui.spec.tab", json!({"id": gypsum})).unwrap();
        a.sync_document();
        assert_eq!(a.windows.count(), 1, "one window, with tabs");
        // Opening a section that is open shows its tab.
        a.run("ui.spec.openSection", json!({"file": "079200.docx"})).unwrap();
        assert_eq!((a.session.document_id(), a.session.documents().len()), (sealants, 2));
        // Check Project saves the open sections first.
        a.run("ui.spec.check", json!({})).unwrap();
        assert!(a.session.documents().iter().all(|d| !d.dirty));
        // A reference at the caret leads to its section.
        a.run("ui.spec.tab", json!({"id": gypsum})).unwrap();
        a.run("caret.set", json!({"pos": {"block": 3, "off": 3}})).unwrap();
        a.run("ui.spec.followReference", json!({})).unwrap();
        assert_eq!(a.session.document_id(), sealants);
        // Closing a tab saves it and shows another; the last one stays.
        a.run("spec.activate", json!({"ids": ["p0002"]})).unwrap();
        a.run("ui.spec.closeTab", json!({"id": sealants})).unwrap();
        assert_eq!(a.session.documents().len(), 1);
        let saved = wordcraft_engine::io::open_path(&dir.join("project").join("079200.docx")).unwrap();
        assert!(saved.custom_prop("Speccraft.On").is_some_and(|v| v.contains("p0002")), "saved on closing");
        assert!(a.run("ui.spec.closeTab", json!({"id": gypsum})).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn at_most_eight_tabs_stay_open() {
        let dir = std::env::temp_dir().join(format!("speccraft-ui-tabs-max-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("project")).unwrap();
        let mut a = WordApp::new(Session::new(Document::new()), Services::default());
        let project = dir.join("project").join("P.speccraft");
        a.run("ui.spec.newProject", json!({"path": project.to_string_lossy(), "name": "P"})).unwrap();
        for i in 0..10 {
            let n = format!("09 {:04}", 2100 + i * 10);
            let m = dir.join(format!("m{i}.docx"));
            wordcraft_engine::io::save_path(&m, &master(&n, "PART", "Text.")).unwrap();
            a.run("ui.spec.addSection", json!({"master": m.to_string_lossy()})).unwrap();
        }
        for i in 0..10 {
            a.run("ui.spec.openSection", json!({"file": format!("m{i}.docx")})).unwrap();
        }
        assert_eq!(a.session.documents().len(), super::MAX_TABS);
        let open: Vec<String> =
            a.session.documents().iter().filter_map(|d| d.path.as_ref().map(|p| p.file_name().unwrap().to_string_lossy().into_owned())).collect();
        assert!(!open.contains(&"m0.docx".to_string()) && open.contains(&"m9.docx".to_string()), "the least recently used went: {open:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
