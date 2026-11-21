//! Specs › Project Overview: where every section stands before issue (milestone M12). Added by
//! the Speccraft fork; not part of the original WordCraft.
//!
//! [`overview`] runs Check Project once and reads each section, and lists per section: what prints,
//! fill-ins still open, Check Project's findings (errors, warnings, notices not accepted), how far
//! it differs from its master (changed, added, deleted master paragraphs), whether its master has
//! changed since and the last master update taken, and whether it is published (and whether the
//! published PDF or Word file is older than the section). A section "needs attention" when it has
//! errors or warnings, a newer master, or is unpublished or out of date. Findings that belong to no
//! section (required sections missing, rules between sections) are listed for the project.
//! Nothing is written.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::edits::{self, Origin};
use super::fields::{self, FieldState};
use super::project;
use super::update::HISTORY_PROP;
use crate::{CmdError, CmdResult, CommandSpec, Session, p};

const LOC: &str = "Specs › Project Overview";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("spec.project.overview", "Project Overview", LOC, overview_cmd)
            .params(r#"{"path": string (the project's .speccraft file)}"#)
            .pure(),
    ]
}

/// The files in `dir` by name (lower case): their modified times.
fn modified_times(dir: &Path) -> BTreeMap<String, u64> {
    crate::io::list_dir(dir, 20_000)
        .map(|(entries, _)| entries.into_iter().filter(|e| !e.dir).map(|e| (e.name.to_lowercase(), e.modified.unwrap_or(0))).collect())
        .unwrap_or_default()
}

/// What one section's file says about it.
fn read_section(path: &Path, name: &str) -> Result<Value, String> {
    let bytes = crate::io::read_document(path)?;
    let doc = crate::io::open_bytes(name, &bytes)?;
    let (items, deleted) = edits::origins(&doc);
    let count = |o: Origin| items.iter().filter(|(_, x)| *x == o).count();
    let printing = items.iter().filter(|(e, _)| e.on).count();
    let notes = items.iter().filter(|(e, _)| e.kind == super::sections::Kind::Note).count();
    let all_fields = fields::fields(&doc);
    let open = all_fields.iter().filter(|f| f.state == FieldState::Open).count();
    let history: Vec<Value> = doc.custom_prop(HISTORY_PROP).and_then(|h| serde_json::from_str(h).ok()).unwrap_or_default();
    let master_version = doc
        .custom_prop(project::MASTER_PROP)
        .and_then(|m| serde_json::from_str::<Value>(m).ok())
        .and_then(|m| m.get("version").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_default();
    Ok(json!({
        "paragraphs": items.len() - notes,
        "printing": printing,
        "fillIns": all_fields.len(),
        "fillInsOpen": open,
        "modified": count(Origin::Modified),
        "added": count(Origin::Added),
        "deleted": deleted.len(),
        "masterVersion": master_version,
        "lastUpdate": history.last().and_then(|h| h.get("date")).cloned().unwrap_or(Value::Null),
        "updates": history.len(),
    }))
}

pub fn overview(s: &mut Session, path: &Path) -> Result<Value, CmdError> {
    let checked = s.run("spec.project.validate", &json!({"path": path.to_string_lossy()}))?;
    let f = project::load(path)?;
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let published_dir: PathBuf = dir.join("Published");
    let here = modified_times(&dir);
    let published = modified_times(&published_dir);
    let findings = checked.get("findings").and_then(Value::as_array).cloned().unwrap_or_default();
    let counts_for = |file: &str| {
        let mine = findings.iter().filter(|x| x.get("file").and_then(Value::as_str) == Some(file) && x.get("acknowledged").is_none());
        let mut c = BTreeMap::<&str, usize>::new();
        let mut newer = false;
        for x in mine {
            if x.get("code").and_then(Value::as_str) == Some("masterChanged") {
                newer = true;
            }
            *c.entry(match x.get("severity").and_then(Value::as_str) {
                Some("error") => "errors",
                Some("warning") => "warnings",
                _ => "notices",
            })
            .or_default() += 1;
        }
        (c, newer)
    };
    let mut rows = Vec::new();
    let mut attention = 0;
    for sec in &f.sections {
        let (c, newer) = counts_for(&sec.file);
        let section_time = here.get(&sec.file.to_lowercase()).copied();
        let out = |ext: &str| published.get(&Path::new(&sec.file).with_extension(ext).to_string_lossy().to_lowercase()).copied();
        let latest = out("pdf").into_iter().chain(out("docx")).max();
        let status = match (latest, section_time) {
            _ if !sec.included => "left out",
            (None, _) => "not published",
            (Some(p), Some(s)) if s > p => "out of date",
            _ => "published",
        };
        let details = match section_time {
            None => json!({"missing": true}),
            Some(_) => read_section(&dir.join(&sec.file), &sec.file).unwrap_or_else(|e| json!({"error": e})),
        };
        let n = |k: &str| c.get(k).copied().unwrap_or(0);
        let needs = sec.included
            && (n("errors") > 0
                || n("warnings") > 0
                || newer
                || status != "published"
                || details.get("missing").is_some()
                || details.get("error").is_some());
        if needs {
            attention += 1;
        }
        let mut row = json!({
            "file": sec.file, "number": sec.number, "title": sec.title, "included": sec.included,
            "errors": n("errors"), "warnings": n("warnings"), "notices": n("notices"),
            "newerMaster": newer, "published": status, "needsAttention": needs,
        });
        if let (Value::Object(r), Value::Object(d)) = (&mut row, details) {
            r.extend(d);
        }
        rows.push(row);
    }
    let project_findings: Vec<Value> = findings
        .iter()
        .filter(|x| x.get("file").and_then(Value::as_str).is_none_or(|file| !f.sections.iter().any(|s| s.file == file)))
        .cloned()
        .collect();
    let manual =
        published_dir.join(format!("{} - Project Manual.pdf", if f.project.name.trim().is_empty() { "Project" } else { f.project.name.trim() }));
    Ok(json!({
        "project": f.project.name,
        "sections": rows,
        "included": f.sections.iter().filter(|s| s.included).count(),
        "needsAttention": attention,
        "errors": checked["errors"], "warnings": checked["warnings"], "notices": checked["notices"],
        "projectFindings": project_findings,
        "manual": published.contains_key(&manual.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default()),
    }))
}

fn overview_cmd(s: &mut Session, v: &Value) -> CmdResult {
    let path = PathBuf::from(p::req_str(v, "path")?);
    overview(s, &path)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::{Value, json};
    use wordcraft_doc::{Block, Document, Paragraph};

    use crate::Session;

    fn run(id: &str, v: Value) -> Value {
        let mut s = Session::new(Document::new());
        s.run(id, &v).unwrap_or_else(|e| panic!("{id}: {e}"))
    }

    fn master(number: &str, title: &str, text: &str) -> Document {
        let p = |s: &str, t: &str| Arc::new(Block::Para(Paragraph::with_text(t, Default::default()).styled(s)));
        let mut d = Document::new();
        d.body = vec![
            p("SCT", &format!("SECTION {number} - {title}")),
            p("PRT", "GENERAL"),
            p("ART", "SUMMARY"),
            p("PR1", text),
            p("PR1", "Second paragraph of text."),
            p("PRT", "PRODUCTS (Not Used)"),
            p("PRT", "EXECUTION (Not Used)"),
            p("EOS", &format!("END OF SECTION {number}")),
        ];
        d
    }

    fn row(o: &Value, file: &str) -> Value {
        o["sections"].as_array().unwrap().iter().find(|r| r["file"] == file).cloned().unwrap()
    }

    #[test]
    fn the_overview_says_where_each_section_stands() {
        let dir = std::env::temp_dir().join(format!("speccraft-overview-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("project")).unwrap();
        let path = dir.join("project").join("P.speccraft");
        let ps = path.to_string_lossy().to_string();
        run("spec.project.new", json!({"path": ps, "name": "P"}));
        for (n, t, x, f) in
            [("09 2900", "GYPSUM BOARD", "Provide [Type X] [Type C] board.", "a.docx"), ("07 9200", "JOINT SEALANTS", "Provide sealant.", "b.docx")]
        {
            let m = dir.join(f);
            crate::io::save_path(&m, &master(n, t, x)).unwrap();
            run("spec.project.addSection", json!({"path": ps, "master": m.to_string_lossy(), "file": f}));
            let file = dir.join("project").join(f);
            let mut s = Session::new(crate::io::open_bytes(f, &std::fs::read(&file).unwrap()).unwrap());
            s.run("spec.activate", &json!({"ids": ["p0002", "p0003", "p0004", "p0005", "p0006", "p0007"]})).unwrap();
            if f == "b.docx" {
                let e = crate::cmd::spec::sections::outline(&s.doc).into_iter().find(|e| e.id == "p0005").unwrap();
                crate::cmd::spec::edits::set_paragraph_text(&mut s.doc, &e, "Second paragraph, edited.").unwrap();
            }
            crate::io::save_path(&file, &s.doc).unwrap();
        }
        let o = run("spec.project.overview", json!({"path": ps}));
        let a = row(&o, "a.docx");
        assert_eq!((a["printing"].clone(), a["fillInsOpen"].clone(), a["published"].clone()), (json!(8), json!(1), json!("not published")), "{a}");
        assert!(a["warnings"].as_u64().unwrap() >= 1 && a["needsAttention"] == true, "the open fill-in: {a}");
        let b = row(&o, "b.docx");
        assert_eq!((b["modified"].clone(), b["warnings"].clone(), b["newerMaster"].clone()), (json!(1), json!(0), json!(false)), "{b}");
        assert_eq!(o["needsAttention"], 2, "neither is published yet");
        // Published: up to date, until the section changes again.
        run("spec.project.publish", json!({"path": ps}));
        let o = run("spec.project.overview", json!({"path": ps}));
        assert_eq!(row(&o, "b.docx")["published"], "published");
        assert_eq!(row(&o, "b.docx")["needsAttention"], false);
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let file = dir.join("project").join("b.docx");
        let bytes = std::fs::read(&file).unwrap();
        std::fs::write(&file, bytes).unwrap();
        let o = run("spec.project.overview", json!({"path": ps}));
        assert_eq!(row(&o, "b.docx")["published"], "out of date");
        // A newer master; a required section missing is the project's own finding.
        crate::io::save_path(&dir.join("b.docx"), &master("07 9200", "JOINT SEALANTS", "Provide silicone sealant.")).unwrap();
        run("spec.project.requires", json!({"path": ps, "file": "a.docx", "requires": ["08 7100"]}));
        let o = run("spec.project.overview", json!({"path": ps}));
        assert_eq!(row(&o, "b.docx")["newerMaster"], true);
        assert!(o["projectFindings"].as_array().unwrap().iter().any(|x| x["code"] == "requiredMissing") || row(&o, "a.docx")["errors"] == 1, "{o}");
        // Left out: listed, never needing attention.
        run("spec.project.include", json!({"path": ps, "file": "b.docx", "included": false}));
        let o = run("spec.project.overview", json!({"path": ps}));
        assert_eq!((row(&o, "b.docx")["published"].clone(), row(&o, "b.docx")["needsAttention"].clone()), (json!("left out"), json!(false)));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
