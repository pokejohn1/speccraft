//! PDFs attached to sections (milestone M23): forms, appendices and schedules that issue with a
//! section. Added by the Speccraft fork; not part of the original WordCraft.
//!
//! An attached PDF is copied into the project's `Attachments` folder (named after its section,
//! `087100-ATT1.pdf`, unless the author names it) and listed in the project file. A section's
//! published PDF ends with its attachments; the project manual puts them after their section or
//! at the end (print option `attachments`), and its table of contents lists them.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use wordcraft_doc::Document;

use super::project::{self, ProjectFile};
use crate::{CmdError, CmdResult, CommandSpec, Session, p};

const LOC: &str = "Specs › Project";
/// The project's folder for attached PDFs.
pub const FOLDER: &str = "Attachments";
/// Largest PDF attached, in bytes.
const MAX_BYTES: u64 = 200 * 1024 * 1024;
/// Most attachments in a project.
const MAX_ATTACHMENTS: usize = 2_000;

/// A PDF attached to a section.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    /// Where it is, from the project folder: `Attachments/087100-ATT1.pdf`.
    pub file: String,
    /// The section file it goes with.
    pub section: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// Its pages when attached.
    #[serde(default)]
    pub pages: usize,
}

impl Attachment {
    /// Its name without the folder and extension: `087100-ATT1`.
    pub fn name(&self) -> String {
        Path::new(&self.file).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
    }
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("spec.project.attach", "Attach PDF to Section", LOC, attach_cmd).params(
            r#"{"path": string, "section": string (the section's file or number), "source": string (the PDF), "title"?: string, "name"?: string (default: 087100-ATT1)}"#,
        ),
        CommandSpec::new("spec.project.detach", "Remove Attachment", LOC, detach_cmd)
            .params(r#"{"path": string, "file": string (as listed; the PDF stays in the Attachments folder)}"#),
        CommandSpec::new("spec.project.attachments", "List Attachments", LOC, list_cmd).params(r#"{"path": string}"#).pure(),
        CommandSpec::new("spec.project.attachmentTitle", "Attachment Title", LOC, title_cmd).params(r#"{"path": string, "file": string, "title": string}"#),
    ]
}

fn folder(path: &Path) -> PathBuf {
    path.parent().map(Path::to_path_buf).unwrap_or_default()
}

/// The section a `section` parameter names: its file, or its number.
fn section_file(f: &ProjectFile, which: &str) -> Result<String, CmdError> {
    let which = which.trim();
    if let Some(s) = f.sections.iter().find(|s| s.file == which) {
        return Ok(s.file.clone());
    }
    let key = project::number_key(which);
    f.sections
        .iter()
        .find(|s| !key.is_empty() && project::number_key(&s.number) == key)
        .map(|s| s.file.clone())
        .ok_or_else(|| CmdError::Failed(format!("no section {which:?} in this project")))
}

/// A file name the author gave, made safe: letters, digits, spaces and `-_.()`.
fn clean_name(n: &str) -> Result<String, CmdError> {
    let n = n.trim().trim_end_matches(".pdf").trim_end_matches(".PDF").trim();
    let ok = !n.is_empty() && n.chars().count() <= 100 && n.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '(' | ')'));
    if !ok || n.starts_with('.') {
        return Err(CmdError::Params("`name` is 1 to 100 letters, digits, spaces and - _ . ( )".into()));
    }
    Ok(n.to_string())
}

fn attach_cmd(_: &mut Session, v: &Value) -> CmdResult {
    let path = PathBuf::from(p::req_str(v, "path")?);
    let source = PathBuf::from(p::req_str(v, "source")?);
    let mut f = project::load(&path)?;
    if f.attachments.len() >= MAX_ATTACHMENTS {
        return Err(CmdError::Failed(format!("a project has at most {MAX_ATTACHMENTS} attachments")));
    }
    let section = section_file(&f, p::req_str(v, "section")?)?;
    let number = f.sections.iter().find(|s| s.file == section).map(|s| s.number.clone()).unwrap_or_default();
    let bytes = crate::io::read_file_capped(&source.to_string_lossy(), MAX_BYTES, "PDF").map_err(CmdError::Failed)?;
    let pages = wordcraft_pdf::pdf_page_count(&bytes).map_err(|e| CmdError::Failed(format!("{}: {e}", source.display())))?;
    if pages == 0 {
        return Err(CmdError::Failed(format!("{} has no pages", source.display())));
    }
    let dir = folder(&path).join(FOLDER);
    crate::io::create_dir_all(&dir).map_err(CmdError::Failed)?;
    // 087100-ATT1, -ATT2, … unless named; never over another file.
    let stem = match p::str(v, "name") {
        Some(n) => clean_name(n)?,
        None => {
            let digits = project::number_key(&number);
            let base = if digits.is_empty() { "ATT".to_string() } else { format!("{digits}-ATT") };
            (1..=MAX_ATTACHMENTS).map(|k| format!("{base}{k}")).find(|n| !dir.join(format!("{n}.pdf")).exists()).unwrap_or(base)
        }
    };
    let rel = format!("{FOLDER}/{stem}.pdf");
    let out = dir.join(format!("{stem}.pdf"));
    if out.exists() || f.attachments.iter().any(|a| a.file == rel) {
        return Err(CmdError::Failed(format!("{rel} already exists in the project: choose another name")));
    }
    crate::io::write_file(&out.to_string_lossy(), &bytes).map_err(CmdError::Failed)?;
    let title = p::str(v, "title").map(|t| t.trim().chars().take(200).collect::<String>()).unwrap_or_default();
    let a = Attachment { file: rel, section, title, pages };
    f.attachments.push(a.clone());
    project::store_file(&path, &f)?;
    Ok(json!({"attachment": a, "name": a.name(), "count": f.attachments.len()}))
}

fn detach_cmd(_: &mut Session, v: &Value) -> CmdResult {
    let path = PathBuf::from(p::req_str(v, "path")?);
    let file = p::req_str(v, "file")?.trim().to_string();
    let mut f = project::load(&path)?;
    let at = f.attachments.iter().position(|a| a.file == file).ok_or_else(|| CmdError::Failed(format!("no attachment {file:?}")))?;
    let a = f.attachments.remove(at);
    project::store_file(&path, &f)?;
    Ok(json!({"removed": a, "count": f.attachments.len()}))
}

fn title_cmd(_: &mut Session, v: &Value) -> CmdResult {
    let path = PathBuf::from(p::req_str(v, "path")?);
    let file = p::req_str(v, "file")?.trim().to_string();
    let title = p::req_str(v, "title")?.trim().to_string();
    if title.chars().count() > 200 || title.contains(['\n', '\r']) {
        return Err(CmdError::Params("`title` is one line of at most 200 characters".into()));
    }
    let mut f = project::load(&path)?;
    let a = f.attachments.iter_mut().find(|a| a.file == file).ok_or_else(|| CmdError::Failed(format!("no attachment {file:?}")))?;
    a.title = title.clone();
    project::store_file(&path, &f)?;
    Ok(json!({"file": file, "title": title}))
}

fn list_cmd(_: &mut Session, v: &Value) -> CmdResult {
    let path = PathBuf::from(p::req_str(v, "path")?);
    let f = project::load(&path)?;
    let dir = folder(&path);
    let rows: Vec<Value> = f
        .attachments
        .iter()
        .map(|a| {
            let number = f.sections.iter().find(|s| s.file == a.section).map(|s| s.number.clone()).unwrap_or_default();
            json!({"file": a.file, "name": a.name(), "section": a.section, "number": number, "title": a.title, "pages": a.pages, "exists": dir.join(&a.file).is_file()})
        })
        .collect();
    Ok(json!({"count": rows.len(), "attachments": rows}))
}

/// The attachments of section file `section`, read, in order; missing or unreadable ones are
/// named in the warnings.
pub fn read_for(project_file: &Path, f: &ProjectFile, section: &str, warnings: &mut Vec<String>) -> Vec<(Attachment, Arc<Vec<u8>>)> {
    let dir = folder(project_file);
    f.attachments
        .iter()
        .filter(|a| a.section == section)
        .filter_map(|a| match crate::io::read_file_capped(&dir.join(&a.file).to_string_lossy(), MAX_BYTES, "PDF") {
            Ok(b) if wordcraft_pdf::pdf_page_count(&b).is_ok_and(|n| n > 0) => Some((a.clone(), Arc::new(b))),
            Ok(_) => {
                warnings.push(format!("attachment {} can't be read as a PDF; left out", a.file));
                None
            }
            Err(e) => {
                warnings.push(format!("attachment {} is missing ({e}); left out", a.file));
                None
            }
        })
        .collect()
}

/// A PDF of `doc` with other PDFs' pages put in (after the given number of `doc`'s pages); with
/// `markup`, tracked changes show (M24).
pub fn pdf_with(doc: &Document, inserts: Vec<wordcraft_pdf::PdfInsert>, markup: bool) -> Result<Vec<u8>, CmdError> {
    let export = crate::io::ExportOptions { markup, ..Default::default() };
    if inserts.is_empty() {
        return crate::io::save_bytes_opts("printable.pdf", doc, &export).map_err(CmdError::Failed);
    }
    // As the ordinary PDF save does: fields as Word fills them in as it prints.
    let out = crate::cmd::references::output_fields(doc);
    let doc = out.as_ref().unwrap_or(doc);
    let opts = wordcraft_pdf::PdfOptions { inserts, ..crate::strict::pdf_options(&export) };
    wordcraft_pdf::export(doc, &opts).map_err(|e| CmdError::Failed(e.to_string()))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use serde_json::{Value, json};
    use wordcraft_doc::{Block, Document, Paragraph};

    use super::super::sections::{self, Kind};
    use super::*;

    fn para(style: &str, text: &str) -> Arc<Block> {
        Arc::new(Block::Para(Paragraph::with_text(text, Default::default()).styled(style)))
    }

    fn run(id: &str, v: Value) -> Value {
        let mut s = Session::new(Document::new());
        s.run(id, &v).unwrap_or_else(|e| panic!("{id}: {e}"))
    }

    fn pdf_pages(path: &Path) -> usize {
        wordcraft_pdf::pdf_page_count(&std::fs::read(path).unwrap()).unwrap()
    }

    /// A project with 08 7100 and 09 2900, everything switched on, and a two-page form PDF.
    fn project() -> (PathBuf, PathBuf, PathBuf) {
        let d = std::env::temp_dir().join(format!("speccraft-attach-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("masters")).unwrap();
        let proj = d.join("project");
        std::fs::create_dir_all(&proj).unwrap();
        let path = proj.join("P.speccraft");
        let p = path.to_string_lossy().to_string();
        run("spec.project.new", json!({"path": p, "name": "Riverside"}));
        for (n, t) in [("08 7100", "DOOR HARDWARE"), ("09 2900", "GYPSUM BOARD")] {
            let mut m = Document::new();
            m.body = vec![
                para("SCT", &format!("SECTION {n} - {t}")),
                para("PRT", "GENERAL"),
                para("ART", "SUMMARY"),
                para("EOS", &format!("END OF SECTION {n}")),
            ];
            let mp = d.join("masters").join(format!("{}.docx", n.replace(' ', "")));
            crate::io::save_path(&mp, &m).unwrap();
            run("spec.project.addSection", json!({"path": p, "master": mp.to_string_lossy()}));
        }
        for sec in run("spec.project.info", json!({"path": p}))["sections"].as_array().unwrap() {
            let file = proj.join(sec["file"].as_str().unwrap());
            let mut x = Session::new(crate::io::open_bytes("x.docx", &std::fs::read(&file).unwrap()).unwrap());
            let ids: Vec<String> = sections::outline(&x.doc).iter().filter(|e| e.kind != Kind::Note).map(|e| e.id.clone()).collect();
            x.run("spec.activate", &json!({"ids": ids})).unwrap();
            crate::io::save_path(&file, &x.doc).unwrap();
        }
        let mut form = Document::new();
        let mut second = Paragraph::with_text("Form page 2", Default::default());
        second.props.page_break_before = Some(true);
        form.body = vec![para("Normal", "Form page 1"), Arc::new(Block::Para(second))];
        let pdf = d.join("Hardware Form.pdf");
        std::fs::write(&pdf, wordcraft_pdf::export(&form, &wordcraft_pdf::PdfOptions::default()).unwrap()).unwrap();
        (path, proj, pdf)
    }

    #[test]
    fn attached_pdfs_issue_with_their_section_and_in_the_manual() {
        let (path, proj, pdf) = project();
        let p = path.to_string_lossy().to_string();
        let src = pdf.to_string_lossy().to_string();
        let r = run("spec.project.attach", json!({"path": p, "section": "08 7100", "source": src, "title": "Hardware Schedule Form"}));
        assert_eq!((r["name"].clone(), r["attachment"]["pages"].clone()), (json!("087100-ATT1"), json!(2)));
        assert!(proj.join("Attachments").join("087100-ATT1.pdf").is_file());
        let r = run("spec.project.attach", json!({"path": p, "section": "08 7100", "source": src}));
        assert_eq!(r["name"], "087100-ATT2");
        // Not a PDF, an unknown section, a bad name: refused.
        let junk = proj.join("junk.pdf");
        std::fs::write(&junk, b"no pdf here").unwrap();
        let mut s = Session::new(Document::new());
        assert!(s.run("spec.project.attach", &json!({"path": p, "section": "08 7100", "source": junk.to_string_lossy()})).is_err());
        assert!(s.run("spec.project.attach", &json!({"path": p, "section": "01 1000", "source": src})).is_err());
        assert!(s.run("spec.project.attach", &json!({"path": p, "section": "08 7100", "source": src, "name": "../x"})).is_err());
        let list = run("spec.project.attachments", json!({"path": p}));
        assert_eq!(list["count"], 2);
        assert_eq!(list["attachments"][0]["number"], "08 7100");
        // The section's PDF ends with its attachments.
        let r = run("spec.project.publish", json!({"path": p, "force": true}));
        let door = r["sections"].as_array().unwrap().iter().find(|x| x["file"] == "087100.docx").unwrap()["pdf"].as_str().unwrap().to_string();
        assert_eq!(pdf_pages(Path::new(&door)), 1 + 4);
        // The manual: contents, 08 7100, its two forms, 09 2900.
        let m = run("spec.project.manual", json!({"path": p, "force": true}));
        let file = PathBuf::from(m["files"]["pdf"].as_str().unwrap());
        assert_eq!(m["attachedPages"], 4);
        assert_eq!(pdf_pages(&file), m["pages"].as_u64().unwrap() as usize + 4);
        let docx = run("spec.project.manual", json!({"path": p, "force": true, "format": "docx", "name": "w"}));
        let w = crate::io::open_bytes("w.docx", &std::fs::read(docx["files"]["docx"].as_str().unwrap()).unwrap()).unwrap();
        let text = w.plain_text(wordcraft_doc::StoryRef::Body);
        assert!(text.contains("087100-ATT1 - Hardware Schedule Form"), "the contents list it: {text}");
        // Left out when the print options say so.
        let none = run("spec.project.manual", json!({"path": p, "force": true, "name": "none", "print": {"attachments": "none"}}));
        assert_eq!(none["attachments"], 0);
        // Removed from the section; the PDF stays in the folder.
        let r = run("spec.project.detach", json!({"path": p, "file": "Attachments/087100-ATT2.pdf"}));
        assert_eq!(r["count"], 1);
        assert!(proj.join("Attachments").join("087100-ATT2.pdf").exists());
        run("spec.project.attachmentTitle", json!({"path": p, "file": "Attachments/087100-ATT1.pdf", "title": "Keying Form"}));
        assert_eq!(run("spec.project.attachments", json!({"path": p}))["attachments"][0]["title"], "Keying Form");
    }
}
