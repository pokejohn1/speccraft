//! Specs › Print: a PDF of exactly the paragraphs that print (milestone M3). Added by the
//! Speccraft fork; not part of the original WordCraft.
//!
//! The live document is never touched: [`printable_copy`] clones it, keeps what prints (the rule
//! of [`sections::printable`]) and the PDF is written from the copy through WordCraft's own PDF
//! export and safe file write. Numbering comes from the paragraph styles, so it closes up by itself.
//!
//! Word export (M13) writes the same copy as a `.docx` without Speccraft's own markings: the
//! content controls around spec paragraphs and fill-ins are taken off (their text stays) and the
//! `Speccraft.*` document properties are dropped, so the file is a plain Word document.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};
use wordcraft_doc::para::OBJ;
use wordcraft_doc::{Block, Document, Paragraph, StoryRef};

use super::sections::{self, Kind};
use crate::{CmdError, CmdResult, CommandSpec, Session};

const LOC: &str = "Specs › Print";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("spec.exportPdf", "Export Printable PDF", LOC, export_pdf)
            .params(r#"{"path"?: string (a .pdf file; default: the section file's own path with .pdf), "project"?: string (a .speccraft file whose values fill placeholders; default: the only one in the section's folder), "form"?: "full" (default), "short" or "outline", "depth"?: n (outline: paragraph levels below the Article, default 1)}"#)
            .pure(),
        CommandSpec::new("spec.exportDocx", "Export to Word", LOC, export_docx)
            .params(r#"{"path"?: string (a .docx file; default: "<section> (issued).docx" beside the section file), "project"?: string, "form"?: string, "depth"?: n (as for spec.exportPdf)}"#)
            .pure(),
    ]
}

/// What [`printable_copy`] kept and left out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Spec items that print.
    pub printed: usize,
    /// Spec items left out (notes included).
    pub omitted: usize,
    /// Spec items that are notes.
    pub notes: usize,
    /// Top-level paragraphs outside any spec item.
    pub unmanaged: usize,
    /// Of those, how many print (they follow the nearest earlier spec item).
    pub unmanaged_printed: usize,
    /// Section breaks moved onto an earlier paragraph that prints.
    pub breaks_moved: usize,
    /// Section breaks kept on an empty paragraph (nothing earlier could take them).
    pub breaks_kept: usize,
    /// Fill-ins still open in what prints: (name, the master's wording).
    pub fields_open: Vec<(String, String)>,
}

/// A copy of `doc` holding only what prints: spec items that [`sections::printable`] lists, and
/// tables and paragraphs outside any spec item when the nearest earlier spec item that is not a
/// note prints (an unmanaged note never prints). Section breaks on paragraphs left out move to the nearest earlier
/// paragraph kept, so page setup, headers and footers survive.
pub fn printable_copy(doc: &Document) -> Result<(Document, Counts), CmdError> {
    printable_copy_as(doc, super::forms::Form::Full)
}

/// [`printable_copy`] in a form (M16): the short form leaves out what the author marked and uses
/// their short wording; the outline also stops at its depth.
pub fn printable_copy_as(doc: &Document, form: super::forms::Form) -> Result<(Document, Counts), CmdError> {
    let entries = sections::outline(doc);
    if entries.is_empty() {
        return Err(CmdError::Failed("this document has no spec paragraphs yet: run spec.import first".into()));
    }
    let top = |e: &sections::Entry| e.range.start.path.0.first().copied();
    let left_out = super::forms::left_out(doc, &entries, form);
    let prints: HashSet<u32> = sections::printable(doc).iter().filter(|e| !left_out.contains(&e.id)).filter_map(top).collect();
    let managed: HashMap<u32, bool> = entries.iter().filter_map(top).map(|b| (b, prints.contains(&b))).collect();
    let mut c = Counts { notes: entries.iter().filter(|e| e.kind == Kind::Note).count(), ..Default::default() };
    c.printed = managed.values().filter(|p| **p).count();
    c.omitted = managed.len().saturating_sub(c.printed);

    let mut kept: wordcraft_doc::Blocks = Vec::with_capacity(doc.body.len());
    // Whether the nearest earlier spec item that is not a note prints.
    let mut follows = false;
    let notes: HashSet<u32> = entries.iter().filter(|e| e.kind == Kind::Note).filter_map(top).collect();
    for (i, b) in doc.body.iter().enumerate() {
        let para = match b.as_ref() {
            Block::Table(_) => {
                if follows {
                    kept.push(b.clone());
                }
                continue;
            }
            Block::Para(p) => p,
        };
        let keep = match u32::try_from(i).ok().and_then(|i| managed.get(&i)) {
            Some(p) => {
                if !notes.contains(&(i as u32)) {
                    follows = *p;
                }
                *p
            }
            None => {
                c.unmanaged += 1;
                let note = sections::classify(para.props.style.as_deref(), &para.text).0 == Kind::Note;
                let keep = follows && !note;
                if keep {
                    c.unmanaged_printed += 1;
                }
                keep
            }
        };
        if keep {
            kept.push(b.clone());
            continue;
        }
        let Some(sect) = &para.section else { continue };
        let earlier = kept.iter().rposition(|b| matches!(b.as_ref(), Block::Para(q) if q.section.is_none()));
        // Only a paragraph after the last kept section break can take this one.
        let last_break = kept.iter().rposition(|b| matches!(b.as_ref(), Block::Para(q) if q.section.is_some()));
        match earlier.filter(|e| last_break.is_none_or(|l| *e > l)).and_then(|e| kept.get_mut(e)) {
            Some(b) => {
                if let Block::Para(q) = Arc::make_mut(b) {
                    q.section = Some(sect.clone());
                    q.touch();
                }
                c.breaks_moved += 1;
            }
            None => {
                let mut blank = Paragraph::new();
                blank.section = Some(sect.clone());
                kept.push(Arc::new(Block::Para(blank)));
                c.breaks_kept += 1;
            }
        }
    }
    let mut out = doc.clone();
    out.body = kept;
    out.ensure_nonempty();
    // The author's short wording, in the short and outline forms.
    super::forms::apply_wording(&mut out, &super::forms::marks(doc), form)?;
    // Fill-ins: left-out text goes; open ones print as written and are reported.
    c.fields_open = super::fields::resolve_for_print(&mut out);
    Ok((out, c))
}

/// The file to write: `path`, else beside the section file (`.pdf`: the same name; `.docx`:
/// "<name> (issued).docx", never the section file). Only a file with extension `ext`.
fn target(s: &Session, v: &Value, ext: &str) -> Result<PathBuf, CmdError> {
    let path = match v.get("path") {
        None | Some(Value::Null) => s
            .path
            .as_ref()
            .map(|p| {
                let stem = p.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let form = super::forms::Form::from_params(v).map(|f| f.suffix()).unwrap_or("");
                match ext {
                    "pdf" => p.with_file_name(format!("{stem}{form}.pdf")),
                    _ => p.with_file_name(format!("{stem}{form} (issued).{ext}")),
                }
            })
            .ok_or_else(|| CmdError::Params(format!("this document has not been saved yet: give `path` (a .{ext} file to write)")))?,
        Some(Value::String(p)) => PathBuf::from(p),
        Some(_) => return Err(CmdError::Params("`path` must be a string".into())),
    };
    if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)) || path.file_stem().is_none() {
        return Err(CmdError::Params(format!("`path` must name a .{ext} file (refusing to write {:?})", path.display().to_string())));
    }
    if s.path.as_ref() == Some(&path) {
        return Err(CmdError::Params("`path` is the section file itself".into()));
    }
    Ok(path)
}

/// The issued copy of a section: what prints, placeholders filled from the project, and what to
/// warn about. Changes nothing in the document.
pub struct Issued {
    pub copy: Document,
    pub counts: Counts,
    pub filled: usize,
    pub warnings: Vec<String>,
}

pub fn issued_copy(s: &Session, project: Option<&str>) -> Result<Issued, CmdError> {
    issued_copy_as(s, project, super::forms::Form::Full)
}

/// [`issued_copy`] in a form (M16).
pub fn issued_copy_as(s: &Session, project: Option<&str>, form: super::forms::Form) -> Result<Issued, CmdError> {
    let (mut copy, mut c) = printable_copy_as(&s.doc, form)?;
    let mut warnings = Vec::new();
    // Placeholders such as <Insert Project Name> take the project's values, in the copy only.
    let mut filled = 0;
    if let Some(project) = super::project::project_for(s.path.as_deref(), project)? {
        match super::project::load(&project) {
            Ok(f) => {
                let values = super::project::placeholder_values(&f);
                c.fields_open.retain(|(_, original)| !super::project::filled_by_project(original, &values));
                let (n, missing) = super::project::fill_placeholders(&mut copy, &values);
                filled = n;
                if !missing.is_empty() {
                    let names: Vec<String> = missing.iter().map(|m| format!("<Insert {m}>")).collect();
                    warnings.push(format!("no project value for {}", names.join(", ")));
                }
            }
            Err(e) => warnings.push(format!("project values not used: {e}")),
        }
    }
    if c.printed == 0 {
        warnings.push("nothing is switched on: the copy is a blank page".to_string());
    }
    if c.unmanaged > 0 {
        warnings.push(format!(
            "{} paragraph(s) are not spec items ({} printed with the item before them): run spec.import to manage them",
            c.unmanaged, c.unmanaged_printed
        ));
    }
    if !c.fields_open.is_empty() {
        let names: Vec<String> = c.fields_open.iter().take(10).map(|(n, o)| format!("{n} ({o})")).collect();
        let more = if c.fields_open.len() > 10 { ", …" } else { "" };
        warnings.push(format!("{} fill-ins are still open and print as written: {}{more}", c.fields_open.len(), names.join(", ")));
    }
    if c.breaks_kept > 0 {
        warnings.push(format!("{} section break(s) kept on an empty paragraph", c.breaks_kept));
    }
    Ok(Issued { copy, counts: c, filled, warnings })
}

/// Take Speccraft's own markings off a copy so it is a plain Word document: the content controls
/// around spec paragraphs and fill-ins go (their text stays), and so do the `Speccraft.*`
/// properties. Returns how many controls were taken off.
pub fn strip_markings(doc: &mut Document) -> usize {
    let ours = |tag: &str| tag.starts_with(super::TAG_PREFIX) || tag.starts_with(super::fields::FIELD_PREFIX);
    let w = OBJ.len_utf8();
    let mut n = 0;
    // The last one first, so an inner fill-in goes before the paragraph around it; positions
    // are read afresh each time.
    while n < 200_000 {
        let Some(r) = doc.control_ranges(StoryRef::Body).into_iter().filter(|r| ours(&r.control.tag)).max_by(|a, b| a.start.cmp(&b.start)) else {
            break;
        };
        if r.control.showing_placeholder && doc.delete_range(&r.content_start(), &r.end).is_err() {
            break;
        }
        let end = doc.control_at(&r.content_start()).map(|x| x.end).unwrap_or(r.end.clone());
        let gone = doc.para_mut(end.story, &end.path).and_then(|p| p.delete(end.off, end.off + w)).is_ok()
            && doc.para_mut(r.start.story, &r.start.path).and_then(|p| p.delete(r.start.off, r.start.off + w)).is_ok();
        if !gone {
            break;
        }
        n += 1;
    }
    doc.custom_props.retain(|p| !p.name.starts_with("Speccraft."));
    n
}

pub fn page_count(doc: &Document) -> usize {
    let opts = wordcraft_layout::LayoutOptions { view: wordcraft_layout::ViewMode::Print, ..Default::default() };
    wordcraft_layout::layout(doc, &mut wordcraft_layout::LayoutCache::new(), &opts).pages.len()
}

fn report(path: &str, bytes: usize, pages: usize, i: &Issued) -> Value {
    let c = &i.counts;
    json!({
        "path": path,
        "pages": pages,
        "bytes": bytes,
        "printed": c.printed,
        "omitted": c.omitted,
        "notes": c.notes,
        "unmanaged": c.unmanaged,
        "sectionBreaksMoved": c.breaks_moved,
        "placeholdersFilled": i.filled,
        "fieldsOpen": c.fields_open.len(),
        "warnings": i.warnings,
    })
}

/// Write a PDF of the paragraphs that print. Changes nothing in the document.
fn export_pdf(s: &mut Session, v: &Value) -> CmdResult {
    let path = target(s, v, "pdf")?;
    let issued = issued_copy_as(s, crate::p::str(v, "project"), super::forms::Form::from_params(v)?)?;
    let pages = page_count(&issued.copy);
    let bytes = crate::io::save_bytes_opts("printable.pdf", &issued.copy, &crate::io::ExportOptions::default()).map_err(CmdError::Failed)?;
    let shown = path.to_string_lossy().to_string();
    crate::io::write_file(&shown, &bytes).map_err(CmdError::Failed)?;
    Ok(report(&shown, bytes.len(), pages, &issued))
}

/// Write a Word document of the paragraphs that print, without Speccraft's markings (M13).
/// Changes nothing in the document.
fn export_docx(s: &mut Session, v: &Value) -> CmdResult {
    let path = target(s, v, "docx")?;
    let mut issued = issued_copy_as(s, crate::p::str(v, "project"), super::forms::Form::from_params(v)?)?;
    let removed = strip_markings(&mut issued.copy);
    let pages = page_count(&issued.copy);
    let bytes = crate::io::save_bytes_opts("issued.docx", &issued.copy, &crate::io::ExportOptions::default()).map_err(CmdError::Failed)?;
    let shown = path.to_string_lossy().to_string();
    crate::io::write_file(&shown, &bytes).map_err(CmdError::Failed)?;
    let mut r = report(&shown, bytes.len(), pages, &issued);
    r["controlsRemoved"] = json!(removed);
    Ok(r)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::{Value, json};
    use wordcraft_doc::para::OBJ;
    use wordcraft_doc::{Block, Document, Paragraph, SectionProps, Table};

    use super::printable_copy;
    use crate::Session;

    /// p0001 … p0009.
    const SECTION: [(&str, &str); 9] = [
        ("SCT", "SECTION 09 2900 - GYPSUM BOARD"),
        ("PRT", "GENERAL"),
        ("ART", "SUMMARY"),
        ("CMT", "Retain or delete this article."),
        ("PR1", "Section includes:"),
        ("PR2", "Interior gypsum board."),
        ("ART", "SUBMITTALS"),
        ("PR1", "Product data."),
        ("EOS", "END OF SECTION 09 2900"),
    ];

    fn para(style: &str, text: &str) -> Arc<Block> {
        Arc::new(Block::Para(Paragraph::with_text(text, Default::default()).styled(style)))
    }

    fn run(s: &mut Session, id: &str, v: Value) -> Value {
        s.run(id, &v).unwrap_or_else(|e| panic!("{id}: {e}"))
    }

    /// The section imported, with GENERAL › SUMMARY › Section includes on.
    fn imported() -> Session {
        let mut d = Document::new();
        d.body = SECTION.iter().map(|(style, text)| para(style, text)).collect();
        let mut s = Session::new(d);
        run(&mut s, "spec.import", json!({}));
        run(&mut s, "spec.activate", json!({"ids": ["p0002", "p0003", "p0005"]}));
        s
    }

    /// Each body paragraph's text, without content control markers ("[table]" for a table).
    fn texts(d: &Document) -> Vec<String> {
        d.body
            .iter()
            .map(|b| match b.as_ref() {
                Block::Para(p) => p.text.replace(OBJ, ""),
                Block::Table(_) => "[table]".into(),
            })
            .collect()
    }

    /// A fresh file path in the temp folder.
    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("speccraft-m3-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn the_copy_keeps_only_what_prints() {
        let s = imported();
        let (copy, c) = printable_copy(&s.doc).unwrap();
        assert_eq!(texts(&copy), ["SECTION 09 2900 - GYPSUM BOARD", "GENERAL", "SUMMARY", "Section includes:", "END OF SECTION 09 2900"]);
        assert_eq!((c.printed, c.omitted, c.notes, c.unmanaged), (5, 4, 1, 0));
        // Styles (and so numbering) are the document's own.
        let styles: Vec<_> = copy.body.iter().filter_map(|b| b.as_para()).map(|p| p.props.style.clone().unwrap_or_default()).collect();
        assert_eq!(styles, ["SCT", "PRT", "ART", "PR1", "EOS"]);
        assert_eq!(copy.styles, s.doc.styles);
        assert_eq!(copy.last_section, s.doc.last_section);
    }

    #[test]
    fn unmanaged_paragraphs_follow_the_item_before_them_and_tables_stay() {
        let mut s = imported();
        // After "Section includes:" (on), after "Interior gypsum board." (off), and a note.
        s.doc.body.insert(5, para("PR2", "Typed after import."));
        s.doc.body.insert(7, para("PR2", "Typed under an off paragraph."));
        s.doc.body.insert(8, para("PR1", "// my own note"));
        s.doc.body.insert(4, Arc::new(Block::Table(Table::new(1, 1, 100.0))));
        let (copy, c) = printable_copy(&s.doc).unwrap();
        assert_eq!(
            texts(&copy),
            ["SECTION 09 2900 - GYPSUM BOARD", "GENERAL", "SUMMARY", "[table]", "Section includes:", "Typed after import.", "END OF SECTION 09 2900"]
        );
        assert_eq!((c.unmanaged, c.unmanaged_printed), (3, 1));
    }

    #[test]
    fn a_table_follows_the_paragraph_before_it_and_skips_notes() {
        let mut s = imported();
        // A table under "Interior gypsum board." (off) and one under "Section includes:" (on),
        // with the article's note between the heading and its table.
        s.doc.body.insert(6, Arc::new(Block::Table(Table::new(1, 1, 100.0))));
        let (copy, _) = printable_copy(&s.doc).unwrap();
        assert!(!texts(&copy).contains(&"[table]".to_string()), "{:?}", texts(&copy));

        let mut s = imported();
        s.doc.body.insert(4, Arc::new(Block::Table(Table::new(1, 1, 100.0))));
        let (copy, _) = printable_copy(&s.doc).unwrap();
        assert_eq!(texts(&copy)[..4], ["SECTION 09 2900 - GYPSUM BOARD", "GENERAL", "SUMMARY", "[table]"]);
        // The same table once SUMMARY is off.
        run(&mut s, "spec.deactivate", json!({"id": "p0003"}));
        let (copy, _) = printable_copy(&s.doc).unwrap();
        assert!(!texts(&copy).contains(&"[table]".to_string()), "{:?}", texts(&copy));
    }

    #[test]
    fn a_section_break_on_a_left_out_paragraph_moves_to_the_one_before() {
        let mut s = imported();
        let landscape = SectionProps { landscape: true, ..Default::default() };
        // "Interior gypsum board." (off) ends a landscape section.
        if let Some(Block::Para(p)) = s.doc.body.get_mut(5).map(Arc::make_mut) {
            p.section = Some(Box::new(landscape.clone()));
        }
        let (copy, c) = printable_copy(&s.doc).unwrap();
        assert_eq!(c.breaks_moved, 1);
        let ends: Vec<_> = copy
            .body
            .iter()
            .filter_map(|b| b.as_para())
            .filter(|p| p.section.is_some())
            .map(|p| (p.text.replace(OBJ, ""), p.section.clone()))
            .collect();
        assert_eq!(ends, [("Section includes:".to_string(), Some(Box::new(landscape.clone())))]);
        assert_eq!(texts(&copy).len(), 5, "no extra paragraph");

        // With nothing kept before it, the break stays, on an empty paragraph.
        run(&mut s, "spec.deactivate", json!({"id": "p0001"}));
        let (copy, c) = printable_copy(&s.doc).unwrap();
        assert_eq!((c.printed, c.breaks_moved, c.breaks_kept), (0, 0, 1));
        let first = copy.body.first().and_then(|b| b.as_para()).unwrap();
        assert!(first.text.is_empty() && first.props.style.is_none());
        assert_eq!(first.section.as_deref(), Some(&landscape));
    }

    #[test]
    fn export_writes_a_pdf_and_changes_nothing() {
        let mut s = imported();
        run(&mut s, "caret.set", json!({"pos": {"block": 2, "off": 3}}));
        let before = (s.doc.clone(), s.sel.clone(), s.undo_labels(), s.can_redo());
        let path = temp("section.pdf");
        let r = run(&mut s, "spec.exportPdf", json!({"path": path.to_string_lossy()}));
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        assert_eq!(r["bytes"], bytes.len());
        assert_eq!(r["pages"], 1);
        assert_eq!(
            (r["printed"].as_u64(), r["omitted"].as_u64(), r["notes"].as_u64(), r["unmanaged"].as_u64()),
            (Some(5), Some(4), Some(1), Some(0))
        );
        assert_eq!(r["warnings"], json!([]));
        assert_eq!((s.doc.clone(), s.sel.clone(), s.undo_labels(), s.can_redo()), before, "document, selection and undo unchanged");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn word_export_is_a_plain_document_of_what_prints() {
        let mut s = imported();
        // A fill-in in a printing paragraph, chosen.
        let e = crate::cmd::spec::sections::outline(&s.doc).into_iter().find(|e| e.id == "p0006").unwrap();
        crate::cmd::spec::edits::set_paragraph_text(&mut s.doc, &e, "Interior [gypsum] [cement] board.").unwrap();
        run(&mut s, "spec.activate", json!({"id": "p0006"}));
        crate::cmd::spec::fields::detect(&mut s.doc).unwrap();
        let f = run(&mut s, "spec.fields.list", json!({}));
        run(&mut s, "spec.fields.set", json!({"id": f["fields"][0]["id"], "value": "gypsum"}));
        let before = (s.doc.clone(), s.sel.clone());
        let file = temp("09 2900.docx");
        s.path = Some(file.clone());
        let r = run(&mut s, "spec.exportDocx", json!({}));
        let out = file.with_file_name("09 2900 (issued).docx");
        assert_eq!(r["path"], out.to_string_lossy().to_string(), "never the section file itself");
        assert!(r["controlsRemoved"].as_u64().unwrap() >= 6, "{r}");
        assert_eq!((s.doc.clone(), s.sel.clone()), before, "the section is unchanged");
        // Opened again: no controls, no Speccraft properties, the printed text and styles.
        let back = crate::io::open_bytes("x.docx", &std::fs::read(&out).unwrap()).unwrap();
        assert!(back.control_ranges(wordcraft_doc::StoryRef::Body).is_empty());
        assert!(back.custom_props.iter().all(|p| !p.name.starts_with("Speccraft.")), "{:?}", back.custom_props);
        let (printed, _) = printable_copy(&s.doc).unwrap();
        let mut want = texts(&printed);
        if let Some(t) = want.iter_mut().find(|t| t.contains("Interior")) {
            *t = "Interior gypsum board.".into();
        }
        assert_eq!(texts(&back), want);
        let styles: Vec<Option<String>> = back.body.iter().filter_map(|b| b.as_para()).map(|p| p.props.style.clone()).collect();
        assert_eq!(styles, ["SCT", "PRT", "ART", "PR1", "PR2", "EOS"].map(|x| Some(x.to_string())));
        assert!(s.run("spec.exportDocx", &json!({"path": file.to_string_lossy()})).is_err(), "not over the section");
        assert!(s.run("spec.exportDocx", &json!({"path": temp("x.pdf").to_string_lossy()})).is_err());
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn export_defaults_to_the_section_files_own_name() {
        let mut s = imported();
        let file = temp("09 2900.docx");
        s.path = Some(file.clone());
        let r = run(&mut s, "spec.exportPdf", json!({}));
        let pdf = file.with_extension("pdf");
        assert_eq!(r["path"], pdf.to_string_lossy().to_string());
        assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF-"));
        let _ = std::fs::remove_file(&pdf);
    }

    #[test]
    fn refusals_write_nothing() {
        // Not imported yet.
        let mut d = Document::new();
        d.body = SECTION.iter().map(|(style, text)| para(style, text)).collect();
        let mut fresh = Session::new(d);
        let path = temp("not-imported.pdf");
        let e = fresh.run("spec.exportPdf", &json!({"path": path.to_string_lossy()})).unwrap_err().to_string();
        assert!(e.contains("spec.import"), "{e}");
        assert!(!path.exists());

        let mut s = imported();
        // No path, and no file the document came from.
        let e = s.run("spec.exportPdf", &json!({})).unwrap_err().to_string();
        assert!(e.contains("path"), "{e}");
        // Never anything but a .pdf: an existing file keeps its bytes.
        let other = temp("keep.txt");
        std::fs::write(&other, b"keep me").unwrap();
        assert!(s.run("spec.exportPdf", &json!({"path": other.to_string_lossy()})).is_err());
        assert_eq!(std::fs::read(&other).unwrap(), b"keep me");
        let docx = temp("section.docx");
        assert!(s.run("spec.exportPdf", &json!({"path": docx.to_string_lossy()})).is_err());
        assert!(!docx.exists());
        // The section file itself is never the target.
        let own = temp("own.pdf");
        s.path = Some(own.clone());
        assert!(s.run("spec.exportPdf", &json!({})).is_err());
        assert!(!own.exists());
        s.path = None;

        // Hostile params.
        let before = s.doc.clone();
        for v in [
            json!(null),
            json!({"path": 5}),
            json!({"path": []}),
            json!({"path": {"x": 1}}),
            json!({"path": ""}),
            json!({"path": ".pdf"}),
            json!({"path": "noextension"}),
            json!({"path": "x.pdf.docx"}),
            json!({"path": "\u{0}.txt"}),
        ] {
            assert!(s.run("spec.exportPdf", &v).is_err(), "{v}");
        }
        assert_eq!(s.doc, before);
        let _ = std::fs::remove_file(&other);
    }
}
