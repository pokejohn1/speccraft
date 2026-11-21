//! The Spec Outline pane's Updates view (Speccraft milestone M11). Added by the Speccraft fork;
//! not part of the original WordCraft.
//!
//! It compares the section on screen with its master (`spec.update.compare`), shows each change
//! word by word with what it means for the project, lets the author choose the master's or their
//! own for each, and applies the choices (`spec.update.apply`). Earlier updates are listed below.

use std::collections::BTreeMap;

use egui::text::{LayoutJob, TextFormat};
use egui::{RichText, Ui};
use serde_json::{Value, json};

use crate::WordApp;
use crate::theme::{Tokens, regular, semibold};

/// What the Updates view keeps between frames.
#[derive(Default)]
pub struct UpdateUi {
    /// The comparison, for one document revision.
    cache: Option<((u64, u64), Value)>,
    /// The author's choices: change number → `master` or `mine`.
    decisions: BTreeMap<u64, String>,
}

/// `spec.update.compare` for the section on screen, cached per revision.
fn comparison(app: &mut WordApp) -> Value {
    let key = (app.session.document_id(), app.session.rev());
    if let Some((k, v)) = &app.speccraft.update.cache
        && *k == key
    {
        return v.clone();
    }
    let v = match app.session.run("spec.update.compare", &json!({})) {
        Ok(v) => v,
        Err(e) => json!({"error": e.to_string()}),
    };
    // A new comparison numbers its changes afresh.
    app.speccraft.update.decisions.clear();
    app.speccraft.update.cache = Some((key, v.clone()));
    v
}

/// The `ui.spec.update*` commands.
pub(crate) fn command(app: &mut WordApp, id: &str, p: &Value) -> Result<Value, String> {
    match id {
        "ui.spec.updateCompare" => {
            app.speccraft.update.cache = None;
            let c = comparison(app);
            match c.get("error").and_then(Value::as_str) {
                Some(e) => Err(e.to_string()),
                None => Ok(c),
            }
        }
        "ui.spec.updateDecide" => {
            let n = p.get("n").and_then(Value::as_u64).ok_or("`n` must be a change number")?;
            let choice = p
                .get("choice")
                .and_then(Value::as_str)
                .filter(|c| matches!(*c, "master" | "mine"))
                .ok_or("`choice` must be \"master\" or \"mine\"")?;
            app.speccraft.update.decisions.insert(n, choice.to_string());
            Ok(json!({"n": n, "choice": choice}))
        }
        "ui.spec.updateApply" => {
            let c = comparison(app);
            if let Some(e) = c.get("error").and_then(Value::as_str) {
                return Err(e.to_string());
            }
            let decisions: serde_json::Map<String, Value> = app.speccraft.update.decisions.iter().map(|(n, d)| (n.to_string(), json!(d))).collect();
            let r = app.run("spec.update.apply", json!({"fingerprint": c.get("fingerprint"), "decisions": decisions}))?;
            app.speccraft.update.cache = None;
            app.speccraft.update.decisions.clear();
            let n = |k: &str| r.get(k).and_then(Value::as_u64).unwrap_or(0).to_string();
            app.status(crate::i18n::fmt(
                tl!("Updated from the master: {took} taken, {kept} kept as they were"),
                &[("took", &n("tookMaster")), ("kept", &n("keptMine"))],
            ));
            Ok(r)
        }
        _ => Err(format!("unknown command `{id}`")),
    }
}

/// Words of `a` and `b` lined up: (word, in a, in b).
fn word_diff(a: &str, b: &str) -> Option<Vec<(String, bool, bool)>> {
    let (x, y): (Vec<&str>, Vec<&str>) = (a.split_whitespace().collect(), b.split_whitespace().collect());
    let (n, m) = (x.len(), y.len());
    if n.saturating_mul(m) > 250_000 {
        return None;
    }
    let w = m + 1;
    let mut dp = vec![0u32; (n + 1) * w];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            let v = if x.get(i) == y.get(j) {
                dp.get((i + 1) * w + j + 1).copied().unwrap_or(0) + 1
            } else {
                dp.get((i + 1) * w + j).copied().unwrap_or(0).max(dp.get(i * w + j + 1).copied().unwrap_or(0))
            };
            if let Some(c) = dp.get_mut(i * w + j) {
                *c = v;
            }
        }
    }
    let at = |i: usize, j: usize| dp.get(i * w + j).copied().unwrap_or(0);
    let (mut i, mut j, mut out) = (0, 0, Vec::new());
    while i < n || j < m {
        if i < n && j < m && x.get(i) == y.get(j) {
            out.push((x.get(i).copied().unwrap_or("").to_string(), true, true));
            i += 1;
            j += 1;
        } else if i < n && (j == m || at(i + 1, j) >= at(i, j + 1)) {
            // Words taken out before the words put in their place.
            out.push((x.get(i).copied().unwrap_or("").to_string(), true, false));
            i += 1;
        } else {
            out.push((y.get(j).copied().unwrap_or("").to_string(), false, true));
            j += 1;
        }
    }
    Some(out)
}

/// Old and new wording as one paragraph: words taken out struck through, words put in coloured.
fn diff_job(ui: &Ui, t: &Tokens, old: &str, new: &str) -> LayoutJob {
    let font = regular(12.0);
    let plain = TextFormat { font_id: font.clone(), color: ui.visuals().text_color(), ..Default::default() };
    let gone = TextFormat { font_id: font.clone(), color: t.red, strikethrough: egui::Stroke::new(1.0, t.red), ..Default::default() };
    let came = TextFormat { font_id: font, color: t.green, underline: egui::Stroke::new(1.0, t.green), ..Default::default() };
    let mut job = LayoutJob::default();
    match word_diff(old, new) {
        Some(words) => {
            for (k, (word, a, b)) in words.iter().enumerate() {
                let fmt = match (a, b) {
                    (true, true) => plain.clone(),
                    (true, false) => gone.clone(),
                    _ => came.clone(),
                };
                let sep = if k == 0 { "" } else { " " };
                job.append(&format!("{sep}{word}"), 0.0, fmt);
            }
        }
        None => {
            job.append(old, 0.0, gone);
            job.append("\n", 0.0, plain);
            job.append(new, 0.0, came);
        }
    }
    job.wrap.max_width = ui.available_width();
    job
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

/// The Updates view.
pub(crate) fn view(app: &mut WordApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let c = comparison(app);
    if c.get("error").is_some() {
        ui.label(RichText::new(tl!("No master to compare with.")).font(semibold(12.5)));
        ui.label(RichText::new(tl!("This section doesn't record a master, or its master can't be read.")).small().color(t.text_dim));
        ui.add_space(4.0);
        ui.label(
            RichText::new(tl!("Sections copied from a master into a project can be compared with it when the master changes."))
                .small()
                .color(t.text_dim),
        );
        history(app, ui, &t);
        return;
    }
    let file = std::path::Path::new(s(&c, "master")).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    ui.label(RichText::new(file).font(semibold(12.5))).on_hover_text(s(&c, "master"));
    let (from, to) = (s(&c, "from"), s(&c, "version"));
    if !from.is_empty() || !to.is_empty() {
        let unknown = tl!("not stated");
        let line = crate::i18n::fmt(
            tl!("Copied from {from} · master now {to}"),
            &[("from", if from.is_empty() { unknown } else { from }), ("to", if to.is_empty() { unknown } else { to })],
        );
        ui.label(RichText::new(line).small().color(t.text_dim));
    }
    let changes = c.get("changes").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut action: Option<(&'static str, Value)> = None;
    ui.horizontal_wrapped(|ui| {
        if changes.is_empty() {
            ui.label(RichText::new(tl!("Up to date with the master.")).small().color(t.green));
        } else {
            let n = |k: &str| c.get(k).and_then(Value::as_u64).unwrap_or(0).to_string();
            let line = crate::i18n::fmt(
                tl!("{c} changed · {a} new · {r} removed · {x} conflicts"),
                &[("c", &n("changed")), ("a", &n("added")), ("r", &n("removed")), ("x", &n("conflicts"))],
            );
            ui.label(RichText::new(line).small().font(semibold(11.5)));
        }
        if ui.small_button(tl!("Compare Again")).on_hover_text(tl!("Read the master again")).clicked() {
            action = Some(("ui.spec.updateCompare", json!({})));
        }
    });
    ui.separator();
    let footer = if changes.is_empty() { 0.0 } else { 34.0 };
    let mut goto = None;
    egui::ScrollArea::vertical().id_salt("spec_updates").max_height((ui.available_height() - footer).max(60.0)).auto_shrink([false, false]).show(
        ui,
        |ui| {
            for ch in &changes {
                change(app, ui, &t, ch, &mut action, &mut goto);
                ui.separator();
            }
            if changes.is_empty() {
                history(app, ui, &t);
            }
        },
    );
    if !changes.is_empty()
        && ui
            .button(RichText::new(tl!("Apply Update")).font(semibold(12.5)))
            .on_hover_text(tl!("Make these choices, and treat this master as the one the section comes from"))
            .clicked()
    {
        action = Some(("ui.spec.updateApply", json!({})));
    }
    if let Some(id) = goto
        && let Err(e) = super::checks::go_to_para(app, &id)
    {
        app.status(e);
    }
    if let Some((id, args)) = action
        && let Err(e) = app.run(id, args)
    {
        app.status(e);
    }
}

/// One change: what, the words, what it means, and the choice.
fn change(app: &WordApp, ui: &mut Ui, t: &Tokens, ch: &Value, action: &mut Option<(&'static str, Value)>, goto: &mut Option<String>) {
    let n = ch.get("n").and_then(Value::as_u64).unwrap_or(0);
    let kind = s(ch, "kind");
    let flag = |k: &str| ch.get(k).and_then(Value::as_bool).unwrap_or(false);
    let (label, take, keep) = match kind {
        "added" => (tl!("New in the master"), tl!("Add it, switched off"), tl!("Leave it out")),
        "removed" => (tl!("Removed from the master"), tl!("Remove it"), tl!("Keep it")),
        _ => (tl!("Changed in the master"), tl!("Take the master's"), tl!("Keep mine")),
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(label).font(semibold(12.0)));
        let id = s(ch, "id").to_string();
        if flag("present") {
            if ui.link(RichText::new(&id).small()).on_hover_text(tl!("Go to the paragraph")).clicked() {
                *goto = Some(id);
            }
        } else {
            ui.label(RichText::new(&id).small().color(t.text_dim));
        }
    });
    let old = ch.get("old").map(|o| s(o, "text")).unwrap_or("");
    let new = ch.get("new").map(|o| s(o, "text")).unwrap_or("");
    match kind {
        "added" => {
            ui.label(RichText::new(new).font(regular(12.0)).color(t.green));
        }
        "removed" => {
            ui.label(RichText::new(old).font(regular(12.0)).color(t.red).strikethrough());
        }
        _ => {
            let job = diff_job(ui, t, old, new);
            ui.label(job);
        }
    }
    // What it means for the project.
    let mut notes: Vec<(String, egui::Color32)> = Vec::new();
    if ch.get("conflict").and_then(Value::as_bool) == Some(true) {
        notes.push((tl!("You changed this paragraph too. Yours:").to_string(), t.red));
    }
    if flag("already") {
        notes.push((tl!("Your copy already reads like this.").to_string(), t.text_dim));
    }
    if flag("printing") {
        notes.push((tl!("It prints in this project.").to_string(), t.text_dim));
    }
    if flag("rules") {
        notes.push((tl!("A rule of this section names it.").to_string(), t.orange));
    }
    let fills = ch.get("fillIns").and_then(Value::as_u64).unwrap_or(0);
    if fills > 0 {
        notes.push((crate::i18n::fmt(tl!("Taking the master's resets {n} fill-ins chosen in it."), &[("n", &fills.to_string())]), t.orange));
    }
    for (note, color) in notes {
        ui.label(RichText::new(note).small().color(color));
        if color == t.red {
            ui.label(RichText::new(s(ch, "mine")).small().italics());
        }
    }
    let chosen = app.speccraft.update.decisions.get(&n).cloned().unwrap_or_else(|| s(ch, "default").to_string());
    ui.horizontal_wrapped(|ui| {
        if ui.radio(chosen == "master", take).clicked() {
            *action = Some(("ui.spec.updateDecide", json!({"n": n, "choice": "master"})));
        }
        if ui.radio(chosen == "mine", keep).clicked() {
            *action = Some(("ui.spec.updateDecide", json!({"n": n, "choice": "mine"})));
        }
    });
}

/// Updates taken before.
fn history(app: &mut WordApp, ui: &mut Ui, t: &Tokens) {
    let h = app.session.run("spec.update.history", &json!({})).unwrap_or(Value::Null);
    let list = h.get("updates").and_then(Value::as_array).cloned().unwrap_or_default();
    if list.is_empty() {
        return;
    }
    ui.add_space(8.0);
    ui.label(RichText::new(tl!("Earlier updates")).font(semibold(11.5)).color(t.text_dim));
    for u in list.iter().rev() {
        let date = s(u, "date").get(..10).unwrap_or(s(u, "date"));
        let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0).to_string();
        let to = if s(u, "to").is_empty() { String::new() } else { format!(" · {}", s(u, "to")) };
        let line = crate::i18n::fmt(
            tl!("{date}{to}: {took} taken, {kept} kept"),
            &[("date", date), ("to", &to), ("took", &n("tookMaster")), ("kept", &n("keptMine"))],
        );
        ui.label(RichText::new(line).small());
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wordcraft_doc::{Block, Document, Paragraph};
    use wordcraft_engine::Session;

    use crate::{Services, WordApp};

    fn master(second: &str) -> Document {
        let p = |s: &str, t: &str| std::sync::Arc::new(Block::Para(Paragraph::with_text(t, Default::default()).styled(s)));
        let mut d = Document::new();
        d.body = vec![
            p("SCT", "SECTION 09 2900 - GYPSUM BOARD"),
            p("PRT", "GENERAL"),
            p("ART", "SUMMARY"),
            p("PR1", "Section includes gypsum board."),
            p("PR1", second),
            p("EOS", "END OF SECTION 09 2900"),
        ];
        d
    }

    #[test]
    fn the_updates_view_compares_decides_and_applies() {
        let dir = std::env::temp_dir().join(format!("speccraft-ui-updates-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("project")).unwrap();
        let m = dir.join("092900.docx");
        wordcraft_engine::io::save_path(&m, &master("Section includes tile backing panels.")).unwrap();
        let mut a = WordApp::new(Session::new(Document::new()), Services::default());
        let project = dir.join("project").join("P.speccraft");
        a.run("ui.spec.newProject", json!({"path": project.to_string_lossy(), "name": "P"})).unwrap();
        a.run("ui.spec.addSection", json!({"master": m.to_string_lossy()})).unwrap();
        assert_eq!(a.run("ui.spec.updateCompare", json!({})).unwrap()["changes"], json!([]));
        // The master changes; Check Project says so.
        wordcraft_engine::io::save_path(&m, &master("Section includes cement backing panels.")).unwrap();
        let check = a.run("ui.spec.check", json!({})).unwrap();
        assert!(check["findings"].as_array().unwrap().iter().any(|f| f["code"] == "masterChanged"), "{check}");
        let c = a.run("ui.spec.updateCompare", json!({})).unwrap();
        assert_eq!(c["changed"], 1, "{c}");
        assert!(a.run("ui.spec.updateDecide", json!({"n": 1, "choice": "maybe"})).is_err());
        a.run("ui.spec.updateDecide", json!({"n": 1, "choice": "mine"})).unwrap();
        let r = a.run("ui.spec.updateApply", json!({})).unwrap();
        assert_eq!((r["tookMaster"].clone(), r["keptMine"].clone()), (json!(0), json!(1)));
        assert_eq!(a.run("ui.spec.updateCompare", json!({})).unwrap()["changes"], json!([]));
        // Saved, the copy no longer reports a newer master.
        a.run("file.save", json!({})).unwrap();
        let check = a.run("ui.spec.check", json!({})).unwrap();
        assert!(!check["findings"].as_array().unwrap().iter().any(|f| f["code"] == "masterChanged"), "{check}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn word_differences_line_up() {
        let d = super::word_diff("Provide tile backing panels.", "Provide cement backing panels.").unwrap();
        let changed: Vec<(&str, bool, bool)> = d.iter().filter(|(_, a, b)| !(*a && *b)).map(|(w, a, b)| (w.as_str(), *a, *b)).collect();
        assert_eq!(changed, [("tile", true, false), ("cement", false, true)]);
    }
}
