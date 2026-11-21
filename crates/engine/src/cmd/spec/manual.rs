//! Specs › Project Manual: one document of every included section, in project order (milestone
//! M14). Added by the Speccraft fork; not part of the original WordCraft.
//!
//! [`build`] takes each section's issued copy (what prints, placeholders filled, Speccraft's
//! markings taken off; the same copy `spec.exportDocx` writes) and puts them one after another.
//! Each section keeps its own page setup, headers and footers, and starts on a new page; page
//! numbers start again at 1 in each section, as project manuals number them (or run on through,
//! if asked). A table of contents comes first: the project, then the sections by division with
//! their page counts. Check Project runs first and errors stop the manual unless forced. The
//! section files are not changed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use wordcraft_doc::props::{TabAlign, TabLeader, TabStop};
use wordcraft_doc::section::{HeaderSet, SectionStart};
use wordcraft_doc::{Align, Block, CharProps, Document, Paragraph, SectionProps};

use super::library::division_name;
use super::output;
use super::project::{self, number_key};
use crate::{CmdError, CmdResult, CommandSpec, Session, p};

const LOC: &str = "Specs › Project Manual";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("spec.project.manual", "Publish Project Manual", LOC, manual_cmd)
            .params(
                r#"{"path": string (the project's .speccraft file), "format"?: "pdf" (default), "docx" or "both", "name"?: string (file name without extension; default "<project name> - Project Manual"), "folder"?: string (default: Published beside the project file), "contents"?: bool (table of contents first; default true), "pageNumbers"?: "restart" (default: each section from 1) or "continue", "force"?: bool (publish even when Check Project finds errors), "form"?: "full" (default), "short" or "outline", "depth"?: n}"#,
            )
            .pure(),
    ]
}

/// One section ready for the manual.
pub struct Part {
    pub number: String,
    pub title: String,
    pub file: String,
    pub pages: usize,
    pub doc: Document,
}

/// The settings of a document's first section: on its first section break, else its last section.
fn first_section(doc: &mut Document) -> &mut SectionProps {
    let at = doc.body.iter().position(|b| b.as_para().is_some_and(|p| p.section.is_some()));
    if let Some(i) = at
        && let Some(b) = doc.body.get_mut(i)
        && let Block::Para(p) = Arc::make_mut(b)
        && let Some(s) = p.section.as_deref_mut()
    {
        return s;
    }
    &mut doc.last_section
}

/// End `doc`'s last section with a section break (its final settings), so more can follow.
fn close(doc: &mut Document) {
    let last = doc.last_section.clone();
    let needs_para = !matches!(doc.body.last().map(Arc::as_ref), Some(Block::Para(p)) if p.section.is_none());
    if needs_para {
        doc.body.push(Arc::new(Block::Para(Paragraph::new())));
    }
    if let Some(b) = doc.body.last_mut()
        && let Block::Para(p) = Arc::make_mut(b)
    {
        p.section = Some(Box::new(last));
        p.touch();
    }
}

/// Every section's settings in `doc` (section breaks and the last section).
fn sections_mut(doc: &mut Document, f: &mut dyn FnMut(&mut SectionProps)) {
    for b in doc.body.iter_mut() {
        if b.as_para().is_some_and(|p| p.section.is_some())
            && let Block::Para(p) = Arc::make_mut(b)
            && let Some(s) = p.section.as_deref_mut()
        {
            f(s);
        }
    }
    f(&mut doc.last_section);
}

/// Bring `src`'s headers and footers into `out` under new ids (their pictures, lists and styles
/// adopted like body text), and point `src`'s sections at them.
fn move_headers(out: &mut Document, src: &mut Document, perms: &mut u32) {
    let mut ids: Vec<u32> = Vec::new();
    sections_mut(src, &mut |s| {
        for set in [&s.headers, &s.footers] {
            ids.extend([set.default, set.first, set.even].into_iter().flatten());
        }
    });
    ids.sort_unstable();
    ids.dedup();
    let mut map: BTreeMap<u32, u32> = BTreeMap::new();
    for id in ids {
        let Some(part) = src.parts.get(&id).cloned() else { continue };
        let mut carrier = src.clone();
        carrier.body = part.blocks.clone();
        carrier.ensure_nonempty();
        let blocks = out.adopt_blocks(carrier, true, perms);
        let new = out.add_part(part.kind, blocks.into_iter().map(Arc::new).collect());
        if let Some(p) = out.parts.get_mut(&new) {
            p.body = part.body;
        }
        map.insert(id, new);
    }
    let relink = |set: &mut HeaderSet| {
        for slot in [&mut set.default, &mut set.first, &mut set.even] {
            *slot = slot.and_then(|id| map.get(&id).copied());
        }
    };
    sections_mut(src, &mut |s| {
        relink(&mut s.headers);
        relink(&mut s.footers);
    });
}

/// The manual: the table of contents (if any), then each part on its own pages.
pub fn build(info: &project::Info, parts: Vec<Part>, contents: bool, restart: bool) -> Result<Document, CmdError> {
    let toc: Vec<(String, String, usize)> = parts.iter().map(|p| (p.number.clone(), p.title.clone(), p.pages)).collect();
    let mut docs = parts.into_iter().map(|p| p.doc);
    let Some(mut out) = docs.next() else { return Err(CmdError::Failed("no section to put in the manual".into())) };
    if restart {
        first_section(&mut out).page_num_start = Some(1);
    }
    let mut perms = 0u32;
    for mut d in docs {
        close(&mut out);
        move_headers(&mut out, &mut d, &mut perms);
        {
            let s = first_section(&mut d);
            s.start = SectionStart::NextPage;
            if restart {
                s.page_num_start = Some(1);
            }
        }
        let last = d.last_section.clone();
        // Its own styles (renamed where ours have the name), so its lists number from 1 again
        // and its formatting is its own.
        let blocks = out.adopt_blocks(d, true, &mut perms);
        out.body.extend(blocks.into_iter().map(Arc::new));
        out.last_section = last;
    }
    if contents {
        put_contents(&mut out, info, &toc);
    }
    out.custom_props.retain(|p| !p.name.starts_with("Speccraft."));
    Ok(out)
}

/// Paragraphs for the table of contents, in their own first section: the first section's page
/// size and margins, no headers or footers.
fn put_contents(out: &mut Document, info: &project::Info, toc: &[(String, String, usize)]) {
    let text = |t: &str, size: f32, bold: bool, align: Align, after: f32| {
        let mut p = Paragraph::with_text(t, CharProps { size: Some(size), bold: Some(bold), ..Default::default() });
        p.props.align = Some(align);
        p.props.space_after = Some(after);
        Arc::new(Block::Para(p))
    };
    let mut page = first_section(out).clone();
    let width = (page.page_w - page.margin_left - page.margin_right).max(72.0);
    let mut front = vec![text("PROJECT MANUAL", 20.0, true, Align::Center, 18.0)];
    for line in [info.name.as_str(), info.number.as_str(), info.issue.as_str(), info.date.as_str()] {
        if !line.trim().is_empty() {
            front.push(text(line.trim(), 13.0, false, Align::Center, 4.0));
        }
    }
    front.push(text("", 11.0, false, Align::Left, 18.0));
    front.push(text("TABLE OF CONTENTS", 14.0, true, Align::Center, 12.0));
    let mut last_div = None;
    for (number, title, pages) in toc {
        let div = number_key(number).get(..2).unwrap_or("").to_string();
        if last_div.as_ref() != Some(&div) {
            let head = if div.is_empty() { "OTHER SECTIONS".to_string() } else { format!("DIVISION {div} - {}", division_name(&div).to_uppercase()) };
            front.push(text(&head, 11.0, true, Align::Left, 4.0));
            last_div = Some(div);
        }
        let name = if number.is_empty() { title.clone() } else { format!("{number}\t{title}") };
        let mut p = Paragraph::with_text(&format!("{name}\t{pages}"), CharProps { size: Some(11.0), ..Default::default() });
        p.props.space_after = Some(2.0);
        p.props.tabs = Some(vec![
            TabStop { pos: 72.0, align: TabAlign::Left, leader: TabLeader::None },
            TabStop { pos: width, align: TabAlign::Right, leader: TabLeader::Dot },
        ]);
        front.push(Arc::new(Block::Para(p)));
    }
    // The contents' own section: no headers or footers, numbering left to the sections.
    page.headers = HeaderSet::default();
    page.footers = HeaderSet::default();
    page.title_page = false;
    page.page_num_start = None;
    page.start = SectionStart::NextPage;
    let mut end = Paragraph::new();
    end.section = Some(Box::new(page));
    front.push(Arc::new(Block::Para(end)));
    // The first part starts a new page after the contents.
    first_section(out).start = SectionStart::NextPage;
    front.append(&mut out.body);
    out.body = front;
}

fn manual_cmd(s: &mut Session, v: &Value) -> CmdResult {
    let path = PathBuf::from(p::req_str(v, "path")?);
    let formats: &[&str] = match p::str(v, "format").unwrap_or("pdf") {
        "pdf" => &["pdf"],
        "docx" => &["docx"],
        "both" => &["pdf", "docx"],
        other => return Err(CmdError::Params(format!("`format` must be \"pdf\", \"docx\" or \"both\", not {other:?}"))),
    };
    let restart = match p::str(v, "pageNumbers").unwrap_or("restart") {
        "restart" => true,
        "continue" => false,
        other => return Err(CmdError::Params(format!("`pageNumbers` must be \"restart\" or \"continue\", not {other:?}"))),
    };
    let form = super::forms::Form::from_params(v)?;
    let checked = s.run("spec.project.validate", &json!({"path": path.to_string_lossy()}))?;
    let errors = checked.get("errors").and_then(Value::as_u64).unwrap_or(0);
    if errors > 0 && !p::bool(v, "force").unwrap_or(false) {
        return Err(CmdError::Failed(format!("Check Project found {errors} errors: fix them first, or publish with force")));
    }
    let f = project::load(&path)?;
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let out_dir = p::str(v, "folder").map_or_else(|| dir.join("Published"), PathBuf::from);
    if out_dir == dir {
        return Err(CmdError::Params("publish to a folder other than the project folder, so outputs and section files stay apart".into()));
    }
    let name = match p::str(v, "name").map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => n.to_string(),
        None => format!("{} - Project Manual{}", if f.project.name.trim().is_empty() { "Project" } else { f.project.name.trim() }, form.suffix()),
    };
    if name.chars().count() > 150 || name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) {
        return Err(CmdError::Params(format!("{name:?} can't be a file name")));
    }
    let mut parts = Vec::new();
    let mut skipped = Vec::new();
    for sec in f.sections.iter().filter(|x| x.included) {
        let full = dir.join(&sec.file);
        let opened = crate::io::read_document(&full).and_then(|b| crate::io::open_bytes(&sec.file, &b)).map_err(CmdError::Failed);
        let issued = opened.and_then(|doc| {
            let mut sess = Session::new(doc);
            sess.path = Some(full.clone());
            output::issued_copy_as(&sess, Some(&path.to_string_lossy()), form)
        });
        match issued {
            Ok(mut i) => {
                output::strip_markings(&mut i.copy);
                let pages = output::page_count(&i.copy);
                parts.push(Part { number: sec.number.clone(), title: sec.title.clone(), file: sec.file.clone(), pages, doc: i.copy });
            }
            Err(e) => skipped.push(json!({"file": sec.file, "error": e.to_string()})),
        }
    }
    if parts.is_empty() {
        return Err(CmdError::Failed("no included section could be read for the manual".into()));
    }
    let listed: Vec<Value> = parts.iter().map(|p| json!({"file": p.file, "number": p.number, "title": p.title, "pages": p.pages})).collect();
    let contents = p::bool(v, "contents").unwrap_or(true);
    let manual = build(&f.project, parts, contents, restart)?;
    let pages = output::page_count(&manual);
    crate::io::create_dir_all(&out_dir).map_err(CmdError::Failed)?;
    let mut written = serde_json::Map::new();
    for ext in formats {
        let file = out_dir.join(format!("{name}.{ext}"));
        let bytes = crate::io::save_bytes_opts(&format!("manual.{ext}"), &manual, &crate::io::ExportOptions::default()).map_err(CmdError::Failed)?;
        crate::io::write_file(&file.to_string_lossy(), &bytes).map_err(CmdError::Failed)?;
        written.insert((*ext).to_string(), json!(file.to_string_lossy()));
    }
    let check = json!({"errors": errors, "warnings": checked["warnings"], "notices": checked["notices"]});
    Ok(json!({"files": written, "pages": pages, "sections": listed, "skipped": skipped, "check": check}))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::{Value, json};
    use wordcraft_doc::{Block, Document, Paragraph, PartKind};

    use crate::Session;

    fn run(id: &str, v: Value) -> Value {
        let mut s = Session::new(Document::new());
        s.run(id, &v).unwrap_or_else(|e| panic!("{id}: {e}"))
    }

    /// A master with a footer naming it.
    fn master(number: &str, title: &str, paras: usize) -> Document {
        let p = |s: &str, t: &str| Arc::new(Block::Para(Paragraph::with_text(t, Default::default()).styled(s)));
        let mut d = Document::new();
        d.body = vec![p("SCT", &format!("SECTION {number} - {title}")), p("PRT", "GENERAL"), p("ART", "SUMMARY")];
        for i in 0..paras {
            d.body.push(p("PR1", &format!("Requirement {i} of {title}, long enough to fill some of a line of text on the page.")));
        }
        d.body.push(p("EOS", &format!("END OF SECTION {number}")));
        for id in ["SCT", "PRT", "ART", "PR1", "EOS"] {
            Arc::make_mut(&mut d.styles).upsert(wordcraft_doc::Style { id: id.into(), name: id.into(), ..Default::default() });
        }
        let id =
            d.add_part(PartKind::Footer, vec![Arc::new(Block::Para(Paragraph::with_text(&format!("{number} {title} footer"), Default::default())))]);
        d.last_section.footers.default = Some(id);
        d
    }

    fn texts(d: &Document) -> Vec<String> {
        d.body.iter().filter_map(|b| b.as_para()).map(|p| p.plain_text()).collect()
    }

    #[test]
    fn the_manual_holds_every_section_with_its_own_footer_and_a_table_of_contents() {
        let dir = std::env::temp_dir().join(format!("speccraft-manual-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("project")).unwrap();
        let path = dir.join("project").join("Riverside.speccraft");
        let ps = path.to_string_lossy().to_string();
        run("spec.project.new", json!({"path": ps, "name": "Riverside Library", "number": "2026-14"}));
        for (n, t, k) in [("09 2900", "GYPSUM BOARD", 3), ("07 9200", "JOINT SEALANTS", 60), ("08 7100", "DOOR HARDWARE", 2)] {
            let m = dir.join(format!("{n}.docx"));
            crate::io::save_path(&m, &master(n, t, k)).unwrap();
            run("spec.project.addSection", json!({"path": ps, "master": m.to_string_lossy()}));
            // Switch the whole section on.
            let file = dir.join("project").join(format!("{n}.docx"));
            let mut s = Session::new(crate::io::open_bytes("x.docx", &std::fs::read(&file).unwrap()).unwrap());
            let ids: Vec<String> = (2..=k + 4).map(|i| format!("p{i:04}")).collect();
            s.run("spec.activate", &json!({"ids": ids})).unwrap();
            crate::io::save_path(&file, &s.doc).unwrap();
        }
        // Left out of the project: not in the manual.
        run("spec.project.include", json!({"path": ps, "file": "08 7100.docx", "included": false}));
        let r = run("spec.project.manual", json!({"path": ps, "format": "both"}));
        let listed: Vec<&str> = r["sections"].as_array().unwrap().iter().map(|x| x["number"].as_str().unwrap()).collect();
        assert_eq!(listed, ["07 9200", "09 2900"], "project order, included only: {r}");
        let docx = dir.join("project").join("Published").join("Riverside Library - Project Manual.docx");
        assert_eq!(r["files"]["docx"], docx.to_string_lossy().as_ref());
        assert!(std::fs::read(dir.join("project").join("Published").join("Riverside Library - Project Manual.pdf")).unwrap().starts_with(b"%PDF"));
        let sealants = r["sections"][0]["pages"].as_u64().unwrap();
        assert!(sealants >= 2, "{r}");
        assert_eq!(r["pages"].as_u64().unwrap(), 1 + sealants + r["sections"][1]["pages"].as_u64().unwrap(), "contents, then each section: {r}");

        let m = crate::io::open_bytes("m.docx", &std::fs::read(&docx).unwrap()).unwrap();
        let t = texts(&m);
        assert_eq!(t[0], "PROJECT MANUAL");
        assert!(t.iter().any(|x| x == "DIVISION 07 - THERMAL AND MOISTURE PROTECTION"), "{t:?}");
        assert!(t.iter().any(|x| x.starts_with("07 9200\tJoint Sealants\t") || x.starts_with("07 9200\tJOINT SEALANTS\t")), "{t:?}");
        let first = t.iter().position(|x| x == "SECTION 07 9200 - JOINT SEALANTS").unwrap();
        let second = t.iter().position(|x| x == "SECTION 09 2900 - GYPSUM BOARD").unwrap();
        assert!(first < second);
        assert!(!t.iter().any(|x| x.contains("DOOR HARDWARE")));
        // The second section brings its own styles, so its Parts and Articles number from 1 again.
        let style_of = |text: &str| m.body.iter().filter_map(|b| b.as_para()).find(|p| p.plain_text() == text).and_then(|p| p.props.style.clone());
        assert_eq!(style_of("SECTION 07 9200 - JOINT SEALANTS").as_deref(), Some("SCT"));
        assert_ne!(style_of("SECTION 09 2900 - GYPSUM BOARD").as_deref(), Some("SCT"));
        // No Speccraft markings; each section keeps its own footer, numbered from 1.
        assert!(m.control_ranges(wordcraft_doc::StoryRef::Body).is_empty());
        assert!(m.custom_props.iter().all(|p| !p.name.starts_with("Speccraft.")));
        let mut sections: Vec<wordcraft_doc::SectionProps> =
            m.body.iter().filter_map(|b| b.as_para()).filter_map(|p| p.section.as_deref().cloned()).collect();
        sections.push(m.last_section.clone());
        assert_eq!(sections.len(), 3, "contents, 07 9200, 09 2900");
        let footer = |s: &wordcraft_doc::SectionProps| {
            s.footers
                .default
                .and_then(|id| m.parts.get(&id))
                .map(|p| p.blocks.iter().filter_map(|b| b.as_para()).map(|p| p.plain_text()).collect::<String>())
        };
        assert_eq!(footer(&sections[0]), None);
        assert_eq!(footer(&sections[1]).as_deref(), Some("07 9200 JOINT SEALANTS footer"));
        assert_eq!(footer(&sections[2]).as_deref(), Some("09 2900 GYPSUM BOARD footer"));
        assert_eq!((sections[1].page_num_start, sections[2].page_num_start), (Some(1), Some(1)));

        // The outline form (M16): its own file, the first paragraph level only.
        let r = run("spec.project.manual", json!({"path": ps, "form": "outline", "depth": 0, "format": "docx"}));
        let file = r["files"]["docx"].as_str().unwrap().to_string();
        assert!(file.ends_with("Riverside Library - Project Manual (Outline).docx"), "{file}");
        let m = crate::io::open_bytes("m.docx", &std::fs::read(&file).unwrap()).unwrap();
        assert!(!texts(&m).iter().any(|t| t.starts_with("Requirement")), "no paragraphs at depth 0");
        assert!(texts(&m).iter().any(|t| t == "SUMMARY"));
        // Without contents, numbering running on.
        let r = run("spec.project.manual", json!({"path": ps, "contents": false, "pageNumbers": "continue", "name": "Plain", "format": "docx"}));
        let m = crate::io::open_bytes("m.docx", &std::fs::read(r["files"]["docx"].as_str().unwrap()).unwrap()).unwrap();
        assert_eq!(texts(&m)[0], "SECTION 07 9200 - JOINT SEALANTS");
        let mut s = Session::new(Document::new());
        assert!(s.run("spec.project.manual", &json!({"path": ps, "format": "rtf"})).is_err());
        assert!(s.run("spec.project.manual", &json!({"path": ps, "name": "a/b"})).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
