//! Sections pane: the project's header and footer (W10, W11). Added by the Speccraft fork; not
//! part of the original WordCraft.
//!
//! The header and footer live in the project's header and footer Word file. Here each is edited
//! as lines of three cells (left, centre, right) with one font and size; the Keywords and Global
//! Terms menus put `{Section ID}`, `{Project Name}`, `{Architect Name}`… into the cell being
//! edited. Saving writes the file and brings every section up to date with it.

use egui::{RichText, Ui};
use serde_json::{Value, json};

use crate::WordApp;
use crate::theme::Tokens;

/// One band (header or footer) as edited.
#[derive(Clone, Debug, Default)]
pub(crate) struct BandForm {
    lines: Vec<[String; 3]>,
    font: String,
    size: f32,
    bold: bool,
    custom: bool,
}

impl BandForm {
    fn from(v: &Value) -> BandForm {
        let s = |l: &Value, k: &str| l.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let mut lines: Vec<[String; 3]> = v
            .get("lines")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(|l| [s(l, "left"), s(l, "center"), s(l, "right")]).collect())
            .unwrap_or_default();
        if lines.is_empty() {
            lines.push(Default::default());
        }
        BandForm {
            lines,
            font: v.get("font").and_then(Value::as_str).unwrap_or("").to_string(),
            size: v.get("size").and_then(Value::as_f64).map_or(0.0, |x| x as f32),
            bold: v.get("bold").and_then(Value::as_bool).unwrap_or(false),
            custom: v.get("custom").and_then(Value::as_bool).unwrap_or(false),
        }
    }

    fn to_json(&self) -> Value {
        let lines: Vec<Value> = self.lines.iter().map(|[l, c, r]| json!({"left": l.trim(), "center": c.trim(), "right": r.trim()})).collect();
        let font = self.font.trim();
        json!({
            "lines": lines,
            "font": if font.is_empty() { Value::Null } else { json!(font) },
            "size": if self.size >= 4.0 { json!(self.size) } else { Value::Null },
            "bold": self.bold,
        })
    }
}

/// The header and footer as edited, with the file's keywords and global terms.
#[derive(Clone, Debug, Default)]
pub(crate) struct FurnitureForm {
    got: Value,
    bands: [BandForm; 2],
    /// The cell last edited: (band, line, column).
    focus: Option<(usize, usize, usize)>,
}

/// Keywords whose values depend on the section, for the preview.
fn preview(text: &str, got: &Value, number: &str, title: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(a) = rest.find('{') {
        let Some(b) = rest.get(a..).and_then(|r| r.find('}')).map(|k| k + a) else { break };
        out.push_str(rest.get(..a).unwrap_or(""));
        let name = rest.get(a + 1..b).unwrap_or("").trim();
        let low = name.to_lowercase();
        let division: String = number.chars().filter(char::is_ascii_digit).take(2).collect();
        let value = match low.as_str() {
            "section id" | "section number" => Some(number.to_string()),
            "section name" | "section title" => Some(super::title_case(title)),
            "division id" | "division number" => Some(division.clone()),
            "division name" => Some(wordcraft_engine::cmd::spec::library::division_name(&division).to_string()),
            "page" | "page number" => Some("1".into()),
            "pages" | "section pages" => Some("12".into()),
            "today" => Some(chrono::Local::now().format("%B %-d, %Y").to_string()),
            _ => got
                .get("keywords")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .chain(got.get("terms").and_then(Value::as_array).into_iter().flatten())
                .find(|k| k.get("name").and_then(Value::as_str).is_some_and(|n| n.eq_ignore_ascii_case(name)))
                .and_then(|k| k.get("value").and_then(Value::as_str))
                .map(str::to_string),
        };
        let known = got
            .get("keywords")
            .and_then(Value::as_array)
            .is_some_and(|k| k.iter().any(|k| k.get("name").and_then(Value::as_str).is_some_and(|n| n.eq_ignore_ascii_case(name))));
        match value {
            Some(v) => out.push_str(&v),
            // Project Information left empty prints nothing.
            None if known => {}
            None => out.push_str(&format!("{{{name}}}")),
        }
        rest = rest.get(b + 1..).unwrap_or("");
    }
    out.push_str(rest);
    out
}

/// Read the project's header and footer into a form.
fn load(app: &mut WordApp) -> Result<FurnitureForm, String> {
    let path = super::project_path(app)?;
    let got = app.session.run("spec.project.furniture", &json!({"path": path})).map_err(|e| e.to_string())?;
    let bands = [BandForm::from(got.get("header").unwrap_or(&Value::Null)), BandForm::from(got.get("footer").unwrap_or(&Value::Null))];
    Ok(FurnitureForm { got, bands, focus: None })
}

/// Bring every section up to date with the project: the one on screen now, other open ones when
/// their tab is shown, the rest on disk.
pub(crate) fn update_all(app: &mut WordApp) -> Result<Value, String> {
    let path = super::project_path(app)?;
    super::project_settings::sync_open(app);
    let skip: Vec<String> =
        app.session.documents().into_iter().filter_map(|d| d.path).filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).collect();
    app.session.run("spec.project.syncAll", &json!({"path": path, "skip": skip})).map_err(|e| e.to_string())
}

/// The section on screen's number and title (for the preview), else the project's first.
fn section_on_screen(app: &WordApp, info: &Value) -> (String, String) {
    let here = app.session.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned());
    let rows = info.get("sections").and_then(Value::as_array).cloned().unwrap_or_default();
    let pick = rows.iter().find(|r| r.get("file").and_then(Value::as_str) == here.as_deref()).or_else(|| rows.first());
    match pick {
        Some(r) => {
            (r.get("number").and_then(Value::as_str).unwrap_or("").to_string(), r.get("title").and_then(Value::as_str).unwrap_or("").to_string())
        }
        None => ("09 2900".into(), "GYPSUM BOARD".into()),
    }
}

/// The Header and Footer part of the Sections pane.
pub(crate) fn show(app: &mut WordApp, ui: &mut Ui, info: &Value) {
    let t = Tokens::get(ui.ctx());
    let head = egui::CollapsingHeader::new(RichText::new(tl!("Header and Footer")).small()).id_salt("spec_furniture").default_open(false);
    let shown = head.show(ui, |ui| {
        if info.get("furnitureExists").and_then(Value::as_bool) != Some(true) {
            ui.label(
                RichText::new(tl!("This project has no header and footer file yet. Make one: every section then takes its header and footer, and you can send it to consultants."))
                    .small()
                    .color(t.text_dim),
            );
            if ui.small_button(tl!("Create Header and Footer File")).clicked() {
                let r = super::project_path(app).and_then(|p| app.run("spec.project.createFurniture", json!({"path": p})));
                match r.and_then(|_| refresh(app)) {
                    Ok(()) => app.status(tl!("Header and footer file made in the project folder")),
                    Err(e) => app.status(e),
                }
            }
            return;
        }
        if app.speccraft.furniture_form.is_none() {
            match load(app) {
                Ok(f) => app.speccraft.furniture_form = Some(f),
                Err(e) => {
                    ui.label(RichText::new(e).small().color(t.red));
                    return;
                }
            }
        }
        let (number, title) = section_on_screen(app, info);
        let Some(form) = app.speccraft.furniture_form.as_mut() else { return };
        let file = form.got.get("file").and_then(Value::as_str).unwrap_or("").to_string();
        let file_path = form.got.get("filePath").and_then(Value::as_str).unwrap_or("").to_string();
        ui.label(RichText::new(&file).small().color(t.text_dim)).on_hover_text(tl!("Every section copies this file's header and footer, fonts and all"));
        let mut open_file = false;
        let mut update = false;
        ui.horizontal_wrapped(|ui| {
            open_file = ui.small_button(tl!("Open File")).on_hover_text(tl!("Edit it like any document: tables, lines, pictures. Save it, then Update All Sections.")).clicked();
            update = ui.small_button(tl!("Update All Sections")).clicked();
        });
        // Keywords and global terms go into the cell being edited.
        let mut insert: Option<String> = None;
        ui.horizontal_wrapped(|ui| {
            ui.menu_button(RichText::new(tl!("Keywords")).small(), |ui| {
                for k in form.got.get("keywords").and_then(Value::as_array).cloned().unwrap_or_default() {
                    let name = k.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                    let about = k.get("about").and_then(Value::as_str).unwrap_or("");
                    if ui.button(&name).on_hover_text(tl!(about)).clicked() {
                        insert = Some(name);
                        ui.close();
                    }
                }
            });
            ui.menu_button(RichText::new(tl!("Global Terms")).small(), |ui| {
                let terms = form.got.get("terms").and_then(Value::as_array).cloned().unwrap_or_default();
                if terms.is_empty() {
                    ui.label(RichText::new(tl!("None yet: add them under Project Information")).small());
                }
                for k in terms {
                    let name = k.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                    let value = k.get("value").and_then(Value::as_str).unwrap_or("").to_string();
                    if ui.button(&name).on_hover_text(value).clicked() {
                        insert = Some(name);
                        ui.close();
                    }
                }
            });
            if form.focus.is_none() {
                ui.label(RichText::new(tl!("Click a cell, then pick one")).small().color(t.text_disabled));
            }
        });
        if let Some(name) = insert {
            let (b, l, c) = form.focus.unwrap_or((1, 0, 0));
            if let Some(cell) = form.bands.get_mut(b).and_then(|band| band.lines.get_mut(l)).and_then(|line| line.get_mut(c)) {
                if !cell.is_empty() && !cell.ends_with(' ') {
                    cell.push(' ');
                }
                cell.push_str(&format!("{{{name}}}"));
            }
        }
        let w = (ui.available_width() - 60.0).max(120.0);
        for (bi, label) in [(0usize, tl!("Header")), (1, tl!("Footer"))] {
            ui.add_space(4.0);
            ui.label(RichText::new(label).small().strong());
            let Some(band) = form.bands.get_mut(bi) else { continue };
            if band.custom {
                ui.label(
                    RichText::new(tl!("Made with formatting these cells can't show (tables, pictures, mixed fonts). Saving here replaces it; use Open File to keep it."))
                        .small()
                        .color(t.orange),
                );
            }
            let n = band.lines.len();
            for (li, line) in band.lines.iter_mut().enumerate() {
                if n > 1 {
                    ui.label(RichText::new(crate::i18n::fmt(tl!("Line {n}"), &[("n", &(li + 1).to_string())])).small().color(t.text_dim));
                }
                egui::Grid::new(("spec_hf_grid", bi, li)).num_columns(2).spacing([6.0, 3.0]).show(ui, |ui| {
                    for (ci, (cell, side)) in line.iter_mut().zip([tl!("Left"), tl!("Center"), tl!("Right")]).enumerate() {
                        ui.label(RichText::new(side).small().color(t.text_dim));
                        let resp = ui.add(egui::TextEdit::singleline(cell).desired_width(w));
                        if resp.has_focus() || resp.gained_focus() {
                            form.focus = Some((bi, li, ci));
                        }
                        ui.end_row();
                    }
                });
            }
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(tl!("Font")).small());
                ui.add(egui::TextEdit::singleline(&mut band.font).hint_text(tl!("as the file")).desired_width(90.0));
                ui.label(RichText::new(tl!("Size")).small());
                ui.add(egui::DragValue::new(&mut band.size).range(0.0..=72.0).speed(0.5).suffix(" pt"));
                ui.checkbox(&mut band.bold, RichText::new(tl!("Bold")).small());
                if band.lines.len() < 6 && ui.small_button("+").on_hover_text(tl!("Add a line")).clicked() {
                    band.lines.push(Default::default());
                }
                if band.lines.len() > 1 && ui.small_button("−").on_hover_text(tl!("Remove the last line")).clicked() {
                    band.lines.pop();
                }
            });
            // What the section on screen will show.
            for [l, c, r] in &band.lines {
                let p = |s: &str| preview(s, &form.got, &number, &title);
                let text = [p(l), p(c), p(r)].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("  |  ");
                if !text.is_empty() {
                    ui.label(RichText::new(text).small().italics().color(t.text_dim));
                }
            }
        }
        ui.add_space(4.0);
        let mut save = false;
        let mut revert = false;
        ui.horizontal(|ui| {
            save = ui.button(tl!("Save and Update Sections")).clicked();
            revert = ui.small_button(tl!("Revert")).clicked();
        });
        let bands = [form.bands[0].to_json(), form.bands[1].to_json()];
        if revert {
            app.speccraft.furniture_form = None;
        }
        if open_file
            && !file_path.is_empty()
            && let Err(e) = super::tabs::open(app, std::path::Path::new(&file_path))
        {
            app.status(e);
        }
        if save {
            let r = super::project_path(app).and_then(|p| app.run("spec.project.setFurniture", json!({"path": p, "header": bands[0], "footer": bands[1]})));
            match r {
                Ok(_) => {
                    app.speccraft.furniture_form = None;
                    update = true;
                }
                Err(e) => app.status(e),
            }
        }
        if update {
            match update_all(app) {
                Ok(r) => {
                    let n = r.get("updated").and_then(Value::as_array).map_or(0, Vec::len);
                    let failed = r.get("failed").and_then(Value::as_array).map_or(0, Vec::len);
                    let mut msg = crate::i18n::fmt(tl!("Header and footer: {n} sections updated; open sections update when shown"), &[("n", &n.to_string())]);
                    if failed > 0 {
                        msg.push_str(&crate::i18n::fmt(tl!("; {k} could not be updated"), &[("k", &failed.to_string())]));
                    }
                    app.status(msg);
                }
                Err(e) => app.status(e),
            }
        }
    });
    if shown.body_returned.is_none() {
        app.speccraft.furniture_form = None;
    }
}

/// Read the project again after a change.
fn refresh(app: &mut WordApp) -> Result<(), String> {
    let path = super::project_path(app)?;
    let info = app.run("spec.project.info", json!({"path": path}))?;
    app.speccraft.info = Some(info);
    app.speccraft.furniture_form = None;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A new project's header and footer load into the cells, save, and reach a section on disk;
    /// the pane draws.
    #[test]
    fn the_header_and_footer_are_edited_in_cells_and_reach_every_section() {
        let dir = std::env::temp_dir().join(format!("speccraft-ui-hf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("m")).unwrap();
        let mut a = crate::WordApp::new(wordcraft_engine::Session::new(wordcraft_doc::Document::new()), crate::Services::default());
        a.run("ui.spec.newProject", json!({"path": dir.join("P.speccraft").to_string_lossy(), "name": "Riverside Library"})).unwrap();
        let project = a.speccraft.project.clone().unwrap();
        let mut d = wordcraft_doc::Document::new();
        let para = |st: &str, t: &str| {
            std::sync::Arc::new(wordcraft_doc::Block::Para(wordcraft_doc::Paragraph::with_text(t, Default::default()).styled(st)))
        };
        d.body = vec![para("SCT", "SECTION 09 2900 - GYPSUM BOARD"), para("PRT", "GENERAL"), para("EOS", "END OF SECTION 09 2900")];
        wordcraft_engine::io::save_path(&dir.join("m").join("092900.docx"), &d).unwrap();
        a.run("ui.spec.addSection", json!({"master": dir.join("m").join("092900.docx").to_string_lossy()})).unwrap();
        let mut form = load(&mut a).unwrap();
        assert_eq!(form.bands[0].lines[0][0], "{Project Name}");
        form.bands[1].lines[0] = ["{Division Name}".into(), String::new(), "{Section ID}".into()];
        form.bands[1].font = "Arial".into();
        form.bands[1].size = 8.0;
        let p = super::super::project_path(&a).unwrap();
        a.run("spec.project.setFurniture", json!({"path": p, "header": form.bands[0].to_json(), "footer": form.bands[1].to_json()})).unwrap();
        // The section is open in a tab: it is brought up to date there, not on disk.
        let r = update_all(&mut a).unwrap();
        assert_eq!(r["updated"], json!([]), "{r}");
        let set = a.session.doc.last_section.footers;
        let footer = a.session.doc.parts.get(&set.default.unwrap()).unwrap().blocks[0].as_para().unwrap().text.clone();
        assert_eq!(footer, "Finishes\t\t09 2900");
        // The pane draws with the editor open.
        a.speccraft.furniture_form = Some(form);
        let info = a.speccraft.info.clone().unwrap();
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        let out = ctx.run_ui(egui::RawInput::default(), |ui| {
            egui::Panel::left("hf_test").default_size(300.0).show(ui, |ui| show(&mut a, ui, &info));
        });
        out.drop_without_applying_deltas();
        let _ = project;
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_preview_fills_keywords_for_the_section() {
        let got =
            json!({"keywords": [{"name": "Project Name", "value": "Riverside Library"}], "terms": [{"name": "Architect Name", "value": "Ruiz"}]});
        assert_eq!(
            preview("{Division ID} {Division Name} | {Section Name} | {Project Name}, {Architect Name} {Owner}", &got, "09 2900", "GYPSUM BOARD"),
            "09 Finishes | Gypsum Board | Riverside Library, Ruiz {Owner}"
        );
    }
}
