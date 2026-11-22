//! Review comments with priority and status, and the comments report (milestone M25). Added by
//! the Speccraft fork; not part of the original WordCraft.
//!
//! Comments are WordCraft's own (Review › New Comment, replies, resolve). Speccraft adds a
//! priority (critical, high, medium, low, not set) and a status (draft, open, accepted, rejected,
//! resolved, closed) to each, kept in [`META_PROP`] by the comment's durable id, so they survive
//! saving as Word. The report lists every comment of a section or of the whole project with its
//! section, paragraph, priority, status, author, date, text and replies.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use wordcraft_doc::{Document, StoryRef};

use super::project;
use super::sections;
use crate::{CmdError, CmdResult, CommandSpec, Session, p};

const LOC: &str = "Specs › Review";
/// Document property: `{durable id: {"priority": …, "status": …}}`.
pub const META_PROP: &str = "Speccraft.CommentMeta";
pub const PRIORITIES: [&str; 5] = ["critical", "high", "medium", "low", "none"];
pub const STATUSES: [&str; 6] = ["draft", "open", "accepted", "rejected", "resolved", "closed"];

/// A comment's priority and status.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meta {
    #[serde(default = "no_priority")]
    pub priority: String,
    #[serde(default = "open")]
    pub status: String,
}

fn no_priority() -> String {
    "none".into()
}

fn open() -> String {
    "open".into()
}

impl Default for Meta {
    fn default() -> Self {
        Meta { priority: no_priority(), status: open() }
    }
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("spec.comments.set", "Comment Priority and Status", LOC, set_cmd).params(
            r#"{"id": n (the comment), "priority"?: "critical" | "high" | "medium" | "low" | "none", "status"?: "draft" | "open" | "accepted" | "rejected" | "resolved" | "closed"}"#,
        ),
        CommandSpec::new("spec.comments.list", "Review Comments", LOC, list_cmd)
            .params(r#"{"priority"?: [string], "status"?: [string] (only these)}"#)
            .pure(),
        CommandSpec::new("spec.project.comments", "Comments Report", LOC, report_cmd).params(
            r#"{"path": string (the project), "format"?: "docx" (default) | "pdf" | "csv", "folder"?: string (default: the project's Published folder), "openOnly"?: bool (leave out resolved and closed)}"#,
        ),
    ]
}

fn metas(doc: &Document) -> BTreeMap<String, Meta> {
    doc.custom_prop(META_PROP).and_then(|v| serde_json::from_str(v).ok()).unwrap_or_default()
}

/// The key a comment's priority and status are kept under: its durable id, given one if it has
/// none so it is the same after saving.
fn key_of(doc: &mut Document, id: u32) -> Option<String> {
    let taken: Vec<String> = doc.comments.values().filter_map(|c| c.durable_id.clone()).collect();
    let c = doc.comments.get_mut(&id)?;
    if c.durable_id.is_none() {
        // Eight hex digits, below 0x7FFFFFFF as Word requires.
        let mut n: u32 = 0x1A00_0000 ^ (id.wrapping_mul(2_654_435_761) & 0x00FF_FFFF);
        while taken.contains(&format!("{n:08X}")) {
            n = n.wrapping_add(1) & 0x7FFF_FFFF;
        }
        c.durable_id = Some(format!("{n:08X}"));
    }
    c.durable_id.clone()
}

fn meta_of(doc: &Document, id: u32) -> Meta {
    let m = metas(doc);
    doc.comments.get(&id).and_then(|c| c.durable_id.as_ref()).and_then(|k| m.get(k)).cloned().unwrap_or_default()
}

fn set_cmd(s: &mut Session, v: &Value) -> CmdResult {
    let id = p::u64(v, "id").and_then(|i| u32::try_from(i).ok()).ok_or_else(|| CmdError::Params("`id` must be a comment number".into()))?;
    if !s.doc.comments.contains_key(&id) {
        return Err(CmdError::Failed(format!("no comment {id}")));
    }
    let mut meta = meta_of(&s.doc, id);
    if let Some(pr) = p::str(v, "priority") {
        if !PRIORITIES.contains(&pr) {
            return Err(CmdError::Params(format!("`priority` must be one of {PRIORITIES:?}")));
        }
        meta.priority = pr.to_string();
    }
    if let Some(st) = p::str(v, "status") {
        if !STATUSES.contains(&st) {
            return Err(CmdError::Params(format!("`status` must be one of {STATUSES:?}")));
        }
        meta.status = st.to_string();
    }
    let key = key_of(&mut s.doc, id).ok_or_else(|| CmdError::Failed(format!("no comment {id}")))?;
    // Resolved and closed comments are marked done in Word too; reopening them clears that.
    let done = matches!(meta.status.as_str(), "resolved" | "closed");
    if let Some(c) = s.doc.comments.get_mut(&id) {
        c.resolved = done;
    }
    let mut all = metas(&s.doc);
    all.insert(key, meta.clone());
    // Only comments still in the document.
    let live: Vec<String> = s.doc.comments.values().filter_map(|c| c.durable_id.clone()).collect();
    all.retain(|k, _| live.contains(k));
    s.doc.set_custom_prop(META_PROP, &serde_json::to_string(&all).map_err(|e| CmdError::Failed(e.to_string()))?);
    Ok(json!({"id": id, "priority": meta.priority, "status": meta.status}))
}

/// A comment as the list and report show it.
#[derive(Clone, Debug)]
pub struct Row {
    pub id: u32,
    pub author: String,
    pub date: String,
    pub text: String,
    pub priority: String,
    pub status: String,
    /// The section paragraph it is on: (sequence number, id, the paragraph's first words).
    pub paragraph: Option<(usize, String, String)>,
    pub replies: Vec<(String, String, String)>,
}

/// The document's comments, in reading order, replies under the comment they answer.
pub fn rows(doc: &Document) -> Vec<Row> {
    let s = Session::new(doc.clone());
    let entries = sections::outline(doc);
    let place = |pos: &wordcraft_doc::Pos| {
        entries.iter().enumerate().find(|(_, e)| e.range.start.path.0.first() == pos.path.0.first() && pos.story == StoryRef::Body).map(|(seq, e)| {
            let text = doc.control_text(&e.range).replace(wordcraft_doc::para::OBJ, "");
            let words: String = text.split_whitespace().take(8).collect::<Vec<_>>().join(" ");
            (seq, e.id.clone(), words)
        })
    };
    let text_of = |c: &wordcraft_doc::Comment| doc.plain_text(StoryRef::Part(c.part)).trim().to_string();
    let mut out = Vec::new();
    for (id, pos) in crate::cmd::review::comment_list(&s) {
        let Some(c) = doc.comments.get(&id) else { continue };
        if c.parent.is_some() {
            continue;
        }
        let meta = meta_of(doc, id);
        let replies = doc.comments.values().filter(|r| r.parent == Some(id)).map(|r| (r.author.clone(), r.date.clone(), text_of(r))).collect();
        out.push(Row {
            id,
            author: c.author.clone(),
            date: c.date.clone(),
            text: text_of(c),
            priority: meta.priority,
            status: meta.status,
            paragraph: pos.as_ref().and_then(place),
            replies,
        });
    }
    out
}

fn row_json(r: &Row) -> Value {
    json!({
        "id": r.id, "author": r.author, "date": r.date, "text": r.text, "priority": r.priority, "status": r.status,
        "paragraph": r.paragraph.as_ref().map(|(seq, id, words)| json!({"seq": seq, "id": id, "words": words})),
        "replies": r.replies.iter().map(|(a, d, t)| json!({"author": a, "date": d, "text": t})).collect::<Vec<_>>(),
    })
}

fn filter_list(v: &Value, key: &str, allowed: &[&str]) -> Result<Option<Vec<String>>, CmdError> {
    let Some(x) = v.get(key) else { return Ok(None) };
    let list = x.as_array().ok_or_else(|| CmdError::Params(format!("`{key}` must be a list")))?;
    let mut out = Vec::new();
    for i in list.iter().take(20) {
        let s = i.as_str().filter(|s| allowed.contains(s)).ok_or_else(|| CmdError::Params(format!("`{key}` values are {allowed:?}")))?;
        out.push(s.to_string());
    }
    Ok(Some(out))
}

fn list_cmd(s: &mut Session, v: &Value) -> CmdResult {
    let pri = filter_list(v, "priority", &PRIORITIES)?;
    let st = filter_list(v, "status", &STATUSES)?;
    let all = rows(&s.doc);
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for r in &all {
        *counts.entry(r.priority.clone()).or_default() += 1;
    }
    let shown: Vec<Value> = all
        .iter()
        .filter(|r| pri.as_ref().is_none_or(|l| l.contains(&r.priority)) && st.as_ref().is_none_or(|l| l.contains(&r.status)))
        .map(row_json)
        .collect();
    Ok(json!({"count": shown.len(), "total": all.len(), "byPriority": counts, "comments": shown}))
}

/// One CSV field.
fn csv(t: &str) -> String {
    // A leading = + - @ would run as a formula in a spreadsheet: start it with a quote mark.
    let t = if t.starts_with(['=', '+', '-', '@']) { format!("'{t}") } else { t.to_string() };
    format!("\"{}\"", t.replace('"', "\"\""))
}

fn report_cmd(_: &mut Session, v: &Value) -> CmdResult {
    let path = PathBuf::from(p::req_str(v, "path")?);
    let f = project::load(&path)?;
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let format = p::str(v, "format").unwrap_or("docx");
    if !matches!(format, "docx" | "pdf" | "csv") {
        return Err(CmdError::Params("`format` must be docx, pdf or csv".into()));
    }
    let open_only = p::bool(v, "openOnly") == Some(true);
    let out_dir = match p::str(v, "folder") {
        Some(x) => PathBuf::from(x),
        None => dir.join("Published"),
    };
    // (section number, title, row)
    let mut all: Vec<(String, String, Row)> = Vec::new();
    let mut unread = Vec::new();
    for sec in &f.sections {
        let doc = crate::io::read_document(&dir.join(&sec.file)).and_then(|b| crate::io::open_bytes(&sec.file, &b));
        let Ok(doc) = doc else {
            unread.push(sec.file.clone());
            continue;
        };
        for r in rows(&doc) {
            if open_only && matches!(r.status.as_str(), "resolved" | "closed") {
                continue;
            }
            all.push((sec.number.clone(), sec.title.clone(), r));
        }
    }
    let name = format!("{} - Comments Report", if f.project.name.trim().is_empty() { "Project" } else { f.project.name.trim() });
    crate::io::create_dir_all(&out_dir).map_err(CmdError::Failed)?;
    let file = out_dir.join(format!("{name}.{format}"));
    let para = |r: &Row| r.paragraph.as_ref().map(|(seq, _, words)| format!("{seq:04} {words}")).unwrap_or_default();
    let bytes = if format == "csv" {
        let mut t = String::from("\u{feff}Section,Paragraph,Priority,Status,Author,Date,Comment,Replies\r\n");
        for (num, _, r) in &all {
            let replies: Vec<String> = r.replies.iter().map(|(a, _, x)| format!("{a}: {x}")).collect();
            let cells =
                [num.clone(), para(r), r.priority.clone(), r.status.clone(), r.author.clone(), r.date.clone(), r.text.clone(), replies.join(" | ")];
            t.push_str(&cells.iter().map(|c| csv(c)).collect::<Vec<_>>().join(","));
            t.push_str("\r\n");
        }
        t.into_bytes()
    } else {
        let doc = report_doc(&f.project, &all);
        crate::io::save_bytes_opts(&format!("report.{format}"), &doc, &crate::io::ExportOptions::default()).map_err(CmdError::Failed)?
    };
    crate::io::write_file(&file.to_string_lossy(), &bytes).map_err(CmdError::Failed)?;
    let mut by_priority: BTreeMap<String, usize> = BTreeMap::new();
    for (_, _, r) in &all {
        *by_priority.entry(r.priority.clone()).or_default() += 1;
    }
    Ok(json!({"path": file.to_string_lossy(), "count": all.len(), "byPriority": by_priority, "unread": unread}))
}

/// The report as a document: a heading, then each section's comments.
fn report_doc(info: &project::Info, all: &[(String, String, Row)]) -> Document {
    use std::sync::Arc;
    use wordcraft_doc::{Block, CharProps, Paragraph};
    let line = |t: &str, size: f32, bold: bool, after: f32| {
        let mut p = Paragraph::with_text(t, CharProps { size: Some(size), bold: Some(bold), ..Default::default() });
        p.props.space_after = Some(after);
        Arc::new(Block::Para(p))
    };
    let mut d = Document::new();
    d.body = vec![line("COMMENTS REPORT", 16.0, true, 6.0)];
    for t in [info.name.as_str(), info.number.as_str(), info.issue.as_str(), info.date.as_str()] {
        if !t.trim().is_empty() {
            d.body.push(line(t.trim(), 11.0, false, 2.0));
        }
    }
    d.body.push(line(&format!("{} comments", all.len()), 11.0, false, 12.0));
    let mut last = None;
    for (num, title, r) in all {
        if last.as_ref() != Some(num) {
            d.body.push(line(&format!("SECTION {num} - {title}"), 12.0, true, 6.0));
            last = Some(num.clone());
        }
        let place =
            r.paragraph.as_ref().map(|(seq, _, words)| format!("{seq:04} \u{201c}{words}\u{201d}")).unwrap_or_else(|| "(not on a paragraph)".into());
        d.body.push(line(&format!("{place} — {} / {} — {}, {}", r.priority.to_uppercase(), r.status, r.author, r.date), 10.0, true, 2.0));
        d.body.push(line(&r.text, 10.5, false, 2.0));
        for (a, date, t) in &r.replies {
            let mut p = Paragraph::with_text(&format!("{a}, {date}: {t}"), CharProps { size: Some(10.0), ..Default::default() });
            p.props.indent_left = Some(24.0);
            p.props.space_after = Some(2.0);
            d.body.push(Arc::new(Block::Para(p)));
        }
        d.body.push(line("", 6.0, false, 6.0));
    }
    d
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::{Value, json};
    use wordcraft_doc::{Block, Document, Paragraph};

    use super::*;

    fn para(style: &str, text: &str) -> Arc<Block> {
        Arc::new(Block::Para(Paragraph::with_text(text, Default::default()).styled(style)))
    }

    fn run(s: &mut Session, id: &str, v: Value) -> Value {
        s.run(id, &v).unwrap_or_else(|e| panic!("{id}: {e}"))
    }

    fn section() -> Session {
        let mut d = Document::new();
        d.body = vec![
            para("SCT", "SECTION 09 2900 - GYPSUM BOARD"),
            para("PRT", "GENERAL"),
            para("ART", "SUMMARY"),
            para("PR1", "Gypsum board, Type X, five-eighths inch."),
            para("EOS", "END OF SECTION 09 2900"),
        ];
        let mut s = Session::new(d);
        run(&mut s, "spec.import", json!({}));
        s
    }

    /// A comment on block `b`, by `who`.
    fn comment(s: &mut Session, b: usize, text: &str) -> u32 {
        let off = wordcraft_doc::para::OBJ.len_utf8();
        s.sel = crate::Selection {
            anchor: wordcraft_doc::Pos::new(StoryRef::Body, wordcraft_doc::Path::top(b), off),
            focus: wordcraft_doc::Pos::new(StoryRef::Body, wordcraft_doc::Path::top(b), off + 6),
        };
        let r = run(s, "review.newComment", json!({"text": text}));
        r["id"].as_u64().unwrap_or_else(|| *s.doc.comments.keys().last().unwrap() as u64) as u32
    }

    #[test]
    fn comments_carry_priority_and_status_and_are_listed_by_paragraph() {
        let mut s = section();
        let a = comment(&mut s, 3, "Confirm the fire rating with the architect.");
        let b = comment(&mut s, 2, "Summary is thin.");
        run(&mut s, "spec.comments.set", json!({"id": a, "priority": "critical", "status": "open"}));
        run(&mut s, "spec.comments.set", json!({"id": b, "priority": "low", "status": "resolved"}));
        assert!(s.doc.comments[&b].resolved, "resolved shows as done in Word too");
        assert!(s.run("spec.comments.set", &json!({"id": a, "priority": "urgent"})).is_err());
        assert!(s.run("spec.comments.set", &json!({"id": 999, "status": "open"})).is_err());
        let l = run(&mut s, "spec.comments.list", json!({}));
        assert_eq!(l["total"], 2);
        assert_eq!(l["comments"][0]["paragraph"]["id"], "p0003", "reading order");
        assert_eq!(l["comments"][1]["priority"], "critical");
        let only = run(&mut s, "spec.comments.list", json!({"priority": ["critical", "high"]}));
        assert_eq!(only["count"], 1);
        // Kept through saving as Word.
        let bytes = crate::io::save_bytes("x.docx", &s.doc).unwrap();
        let mut back = Session::new(crate::io::open_bytes("x.docx", &bytes).unwrap());
        let l = run(&mut back, "spec.comments.list", json!({"status": ["open"]}));
        assert_eq!((l["count"].clone(), l["comments"][0]["priority"].clone()), (json!(1), json!("critical")));
    }

    #[test]
    fn csv_cells_never_run_as_formulas() {
        assert_eq!(csv("=SUM(A1)"), "\"'=SUM(A1)\"");
        assert_eq!(csv("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn the_project_report_lists_every_sections_comments() {
        let d = std::env::temp_dir().join(format!("speccraft-comments-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("project")).unwrap();
        let m = d.join("092900.docx");
        crate::io::save_path(&m, &section().doc).unwrap();
        let path = d.join("project").join("P.speccraft");
        let p = path.to_string_lossy().to_string();
        let mut s = Session::new(Document::new());
        run(&mut s, "spec.project.new", json!({"path": p, "name": "Riverside"}));
        run(&mut s, "spec.project.addSection", json!({"path": p, "master": m.to_string_lossy()}));
        let file = d.join("project").join("092900.docx");
        let mut sec = Session::new(crate::io::open_bytes("x.docx", &std::fs::read(&file).unwrap()).unwrap());
        let a = comment(&mut sec, 3, "=Check the rating.");
        run(&mut sec, "spec.comments.set", json!({"id": a, "priority": "high"}));
        crate::io::save_path(&file, &sec.doc).unwrap();
        let r = run(&mut s, "spec.project.comments", json!({"path": p, "format": "csv"}));
        assert_eq!((r["count"].clone(), r["byPriority"]["high"].clone()), (json!(1), json!(1)));
        let csv = String::from_utf8(std::fs::read(r["path"].as_str().unwrap()).unwrap()).unwrap();
        assert!(csv.contains("\"09 2900\",\"0003 Gypsum board, Type X, five-eighths inch.\",\"high\",\"open\""), "{csv}");
        assert!(csv.contains("\"'=Check the rating.\""));
        let r = run(&mut s, "spec.project.comments", json!({"path": p, "format": "pdf"}));
        assert!(std::fs::read(r["path"].as_str().unwrap()).unwrap().starts_with(b"%PDF"));
        assert!(s.run("spec.project.comments", &json!({"path": p, "format": "xlsx"})).is_err());
    }
}
