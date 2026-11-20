//! Specs › Library: the master library (milestone M9). Added by the Speccraft fork; not part of
//! the original WordCraft.
//!
//! A library is a folder of master sections (`.docx`, in subfolders too). [`scan`] reads each
//! master once and keeps what search and preview need in an index file in the folder
//! (`library.speccraft-library`, JSON): number, title, division, version stamp, paragraphs (kind,
//! level, text), and the author's own tags, note and "approved" mark, which survive rescans. Only
//! masters whose size or date changed are read again. Masters are never written. Project copies
//! (files marked `Speccraft.Role = project`) are not masters and are skipped.
//!
//! Sections group under the MasterFormat divisions; only the division names are built in.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use wordcraft_doc::Document;
use wordcraft_doc::para::OBJ;

use super::project::{self, number_key, split_title};
use super::sections::{self, Kind};
use crate::{CmdError, CmdResult, CommandSpec, Session, p};

const LOC: &str = "Specs › Library";
/// The index file's name, in the library folder.
pub const INDEX_FILE: &str = "library.speccraft-library";
const FORMAT: &str = "speccraft-library";
const VERSION: u32 = 1;
/// Most masters indexed.
const MAX_MASTERS: usize = 5_000;
/// Most paragraphs kept per master.
const MAX_PARAS: usize = 3_000;
/// Deepest subfolder scanned.
const MAX_DEPTH: usize = 4;
/// Most search results returned.
const MAX_RESULTS: usize = 200;
/// Largest index file read, in bytes.
const MAX_INDEX: usize = 256 << 20;

/// MasterFormat division numbers and names (division level only).
pub const DIVISIONS: [(&str, &str); 35] = [
    ("00", "Procurement and Contracting Requirements"),
    ("01", "General Requirements"),
    ("02", "Existing Conditions"),
    ("03", "Concrete"),
    ("04", "Masonry"),
    ("05", "Metals"),
    ("06", "Wood, Plastics, and Composites"),
    ("07", "Thermal and Moisture Protection"),
    ("08", "Openings"),
    ("09", "Finishes"),
    ("10", "Specialties"),
    ("11", "Equipment"),
    ("12", "Furnishings"),
    ("13", "Special Construction"),
    ("14", "Conveying Equipment"),
    ("21", "Fire Suppression"),
    ("22", "Plumbing"),
    ("23", "Heating, Ventilating, and Air Conditioning"),
    ("25", "Integrated Automation"),
    ("26", "Electrical"),
    ("27", "Communications"),
    ("28", "Electronic Safety and Security"),
    ("31", "Earthwork"),
    ("32", "Exterior Improvements"),
    ("33", "Utilities"),
    ("34", "Transportation"),
    ("35", "Waterway and Marine Construction"),
    ("40", "Process Interconnections"),
    ("41", "Material Processing and Handling Equipment"),
    ("42", "Process Heating, Cooling, and Drying Equipment"),
    ("43", "Process Gas and Liquid Handling, Purification, and Storage Equipment"),
    ("44", "Pollution and Waste Control Equipment"),
    ("45", "Industry-Specific Manufacturing Equipment"),
    ("46", "Water and Wastewater Equipment"),
    ("48", "Electrical Power Generation"),
];

/// The name of division `d` ("09" → "Finishes").
pub fn division_name(d: &str) -> &'static str {
    DIVISIONS.iter().find(|(n, _)| *n == d).map_or("", |(_, name)| *name)
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("spec.library.scan", "Scan Master Library", LOC, scan_cmd).params(r#"{"folder": string (the library folder)}"#).pure(),
        CommandSpec::new("spec.library.info", "Master Library", LOC, info_cmd).params(r#"{"folder": string (the library folder)}"#).pure(),
        CommandSpec::new("spec.library.search", "Search Master Library", LOC, search_cmd)
            .params(r#"{"folder": string (the library folder), "query"?: string (number, title words, text, tags), "division"?: "09", "tag"?: string, "approved"?: bool}"#)
            .pure(),
        CommandSpec::new("spec.library.preview", "Preview Master", LOC, preview_cmd)
            .params(r#"{"folder": string (the library folder), "file": string (as the index lists it)}"#)
            .pure(),
        CommandSpec::new("spec.library.tag", "Tag Master", LOC, tag_cmd)
            .params(r#"{"folder": string (the library folder), "file": string, "tags"?: [string], "approved"?: bool, "note"?: string}"#)
            .pure(),
    ]
}

// ---------------------------------------------------------------------------------------------
// The index

/// One paragraph of a master as the index keeps it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexedPara {
    /// `title`, `part`, `article`, `para`, `end`, `note`, `other`.
    pub k: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub l: Option<u8>,
    pub t: String,
}

/// One master in the index.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Master {
    /// Path relative to the library folder, with `/`.
    pub file: String,
    #[serde(default)]
    pub number: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub division: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version: String,
    /// Size and modified time when read, to notice changes.
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub modified: u64,
    #[serde(default)]
    pub paras: Vec<IndexedPara>,
    /// The author's tags, approval and note: kept across rescans.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default)]
    pub approved: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// Why it couldn't be read, if it couldn't.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub error: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// The index file.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Index {
    pub format: String,
    pub version: u32,
    #[serde(default)]
    pub masters: Vec<Master>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn folder_param(v: &Value) -> Result<PathBuf, CmdError> {
    let f = PathBuf::from(p::req_str(v, "folder")?);
    if f.as_os_str().is_empty() {
        return Err(CmdError::Params("`folder` must name the library folder".into()));
    }
    Ok(f)
}

/// Read a library's index (an empty one if there is none yet).
pub fn load(folder: &Path) -> Result<Index, CmdError> {
    let path = folder.join(INDEX_FILE);
    let Ok(bytes) = crate::io::read_document(&path) else {
        return Ok(Index { format: FORMAT.into(), version: VERSION, ..Default::default() });
    };
    if bytes.len() > MAX_INDEX {
        return Err(CmdError::Failed(format!("{}: the library index is too large", path.display())));
    }
    let ix: Index =
        serde_json::from_slice(&bytes).map_err(|e| CmdError::Failed(format!("{}: not a Speccraft library index ({e})", path.display())))?;
    if ix.format != FORMAT || ix.version > VERSION {
        return Err(CmdError::Failed(format!("{}: not a library index this Speccraft reads", path.display())));
    }
    Ok(ix)
}

fn store(folder: &Path, ix: &Index) -> Result<(), CmdError> {
    let text = serde_json::to_string(ix).map_err(|e| CmdError::Failed(e.to_string()))?;
    crate::io::write_file(&folder.join(INDEX_FILE).to_string_lossy(), text.as_bytes()).map_err(CmdError::Failed)
}

/// The `.docx` files under `folder` (up to [`MAX_DEPTH`] levels), as (relative path, full path,
/// size, modified), skipping Word's lock files.
#[cfg(not(target_arch = "wasm32"))]
fn docx_files(folder: &Path) -> Vec<(String, PathBuf, u64, u64)> {
    let mut out = Vec::new();
    let mut stack = vec![(folder.to_path_buf(), String::new(), 0usize)];
    while let Some((dir, rel, depth)) = stack.pop() {
        let Ok((entries, _)) = crate::io::list_dir(&dir, 20_000) else { continue };
        for e in entries {
            let rel_name = if rel.is_empty() { e.name.clone() } else { format!("{rel}/{}", e.name) };
            if e.dir {
                if depth < MAX_DEPTH && !e.name.starts_with('.') && e.name != "Published" {
                    stack.push((e.path, rel_name, depth + 1));
                }
            } else if Path::new(&e.name).extension().is_some_and(|x| x.eq_ignore_ascii_case("docx")) && !e.name.starts_with("~$") {
                out.push((rel_name, e.path, e.len, e.modified.unwrap_or(0)));
            }
            if out.len() >= MAX_MASTERS {
                return out;
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[cfg(target_arch = "wasm32")]
fn docx_files(_folder: &Path) -> Vec<(String, PathBuf, u64, u64)> {
    Vec::new()
}

/// What the index keeps of a master document. `None` for a project copy.
fn read_master(doc: &Document) -> Option<(String, String, String, Vec<IndexedPara>)> {
    if doc.custom_prop(project::ROLE_PROP) == Some("project") {
        return None;
    }
    let mut paras = Vec::new();
    let mut title_text = String::new();
    for b in doc.body.iter() {
        let Some(pa) = b.as_para() else { continue };
        let text = pa.plain_text().replace(OBJ, "");
        if text.trim().is_empty() {
            continue;
        }
        let (kind, level) = sections::classify(pa.props.style.as_deref(), &text);
        if kind == Kind::Title && title_text.is_empty() {
            title_text = text.clone();
        }
        if paras.len() < MAX_PARAS {
            paras.push(IndexedPara { k: kind.name().into(), l: level, t: text.trim().chars().take(2_000).collect() });
        }
    }
    let (number, title) = if title_text.is_empty() { (String::new(), String::new()) } else { split_title(&title_text) };
    Some((number, title, project::version_stamp(doc), paras))
}

/// Bring the index up to date with the folder: new and changed masters are read, removed ones
/// dropped; tags, approval and notes are kept. Returns (index, read, unchanged, skipped copies).
pub fn scan(folder: &Path) -> Result<(Index, usize, usize, usize), CmdError> {
    let old = load(folder)?;
    let by_file: BTreeMap<String, Master> = old.masters.iter().map(|m| (m.file.clone(), m.clone())).collect();
    let mut ix = Index { format: FORMAT.into(), version: VERSION, masters: Vec::new(), extra: old.extra };
    let (mut read, mut same, mut copies) = (0, 0, 0);
    for (rel, full, size, modified) in docx_files(folder) {
        let prev = by_file.get(&rel);
        if let Some(m) = prev.filter(|m| m.size == size && m.modified == modified && m.error.is_empty()) {
            ix.masters.push(m.clone());
            same += 1;
            continue;
        }
        let mut m = Master { file: rel.clone(), size, modified, ..Default::default() };
        if let Some(prev) = prev {
            m.tags = prev.tags.clone();
            m.approved = prev.approved;
            m.note = prev.note.clone();
        }
        match crate::io::read_document(&full).and_then(|b| crate::io::open_bytes(&rel, &b)) {
            Ok(doc) => match read_master(&doc) {
                Some((number, title, version, paras)) => {
                    m.division = number_key(&number).get(..2).unwrap_or("").to_string();
                    m.number = number;
                    m.title = title;
                    m.version = version;
                    m.paras = paras;
                }
                None => {
                    copies += 1;
                    continue;
                }
            },
            Err(e) => m.error = e,
        }
        read += 1;
        ix.masters.push(m);
    }
    ix.masters.sort_by(|a, b| {
        let (ka, kb) = (number_key(&a.number), number_key(&b.number));
        (ka.is_empty(), ka, &a.file).cmp(&(kb.is_empty(), kb, &b.file))
    });
    store(folder, &ix)?;
    Ok((ix, read, same, copies))
}

/// A master as the commands list it (without its paragraphs).
fn master_json(m: &Master) -> Value {
    json!({
        "file": m.file,
        "number": m.number,
        "title": m.title,
        "division": m.division,
        "divisionName": division_name(&m.division),
        "version": m.version,
        "paragraphs": m.paras.len(),
        "tags": m.tags,
        "approved": m.approved,
        "note": m.note,
        "error": m.error,
    })
}

// ---------------------------------------------------------------------------------------------
// Search

/// How well `m` matches `query`, and a snippet: the paragraph that matched, if one did. Zero is
/// no match. Every word of the query must match somewhere.
fn score(m: &Master, query: &str) -> (u32, Option<String>) {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return (1, None);
    }
    let digits: String = q.chars().filter(char::is_ascii_digit).collect();
    let key = number_key(&m.number);
    let number_query = !digits.is_empty() && q.chars().all(|c| c.is_ascii_digit() || c == ' ' || c == '.');
    if number_query {
        return if !key.is_empty() && key.starts_with(&digits) {
            (1_000 - (key.len().saturating_sub(digits.len()) as u32).min(999), None)
        } else {
            (0, None)
        };
    }
    let title = m.title.to_lowercase();
    let tags: Vec<String> = m.tags.iter().map(|t| t.to_lowercase()).collect();
    let mut total = 0u32;
    let mut snippet = None;
    for w in q.split_whitespace() {
        let mut best = 0u32;
        if title.split(|c: char| !c.is_alphanumeric()).any(|t| t == w) {
            best = best.max(300);
        } else if title.contains(w) {
            best = best.max(200);
        }
        if tags.iter().any(|t| t == w || t.contains(w)) {
            best = best.max(150);
        }
        if m.note.to_lowercase().contains(w) {
            best = best.max(60);
        }
        if best < 100
            && let Some(p) = m.paras.iter().find(|p| p.k != "note" && p.t.to_lowercase().contains(w))
        {
            best = best.max(50);
            if snippet.is_none() {
                snippet = Some(p.t.chars().take(160).collect());
            }
        }
        if best == 0 {
            return (0, None);
        }
        total = total.saturating_add(best);
    }
    (total + u32::from(m.approved) * 5, snippet)
}

/// The masters matching `query` (and filters), best first.
pub fn search<'a>(
    ix: &'a Index,
    query: &str,
    division: Option<&str>,
    tag: Option<&str>,
    approved: Option<bool>,
) -> Vec<(u32, &'a Master, Option<String>)> {
    let mut out: Vec<(u32, &Master, Option<String>)> = ix
        .masters
        .iter()
        .filter(|m| division.is_none_or(|d| m.division == d.trim()))
        .filter(|m| tag.is_none_or(|t| m.tags.iter().any(|x| x.eq_ignore_ascii_case(t.trim()))))
        .filter(|m| approved.is_none_or(|a| m.approved == a))
        .filter_map(|m| {
            let (s, snip) = score(m, query);
            (s > 0).then_some((s, m, snip))
        })
        .collect();
    out.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| number_key(&a.1.number).cmp(&number_key(&b.1.number))));
    out.truncate(MAX_RESULTS);
    out
}

// ---------------------------------------------------------------------------------------------
// Commands

fn scan_cmd(_: &mut Session, v: &Value) -> CmdResult {
    let folder = folder_param(v)?;
    let (ix, read, same, copies) = scan(&folder)?;
    let errors: Vec<Value> = ix.masters.iter().filter(|m| !m.error.is_empty()).map(|m| json!({"file": m.file, "error": m.error})).collect();
    let untitled = ix.masters.iter().filter(|m| m.error.is_empty() && number_key(&m.number).is_empty()).count();
    Ok(
        json!({"masters": ix.masters.len(), "read": read, "unchanged": same, "skippedProjectCopies": copies, "withoutNumber": untitled, "errors": errors}),
    )
}

fn info_cmd(_: &mut Session, v: &Value) -> CmdResult {
    let folder = folder_param(v)?;
    let ix = load(&folder)?;
    let mut divisions: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for m in &ix.masters {
        divisions.entry(m.division.clone()).or_default().push(master_json(m));
    }
    let list: Vec<Value> = divisions.into_iter().map(|(d, masters)| json!({"division": d, "name": division_name(&d), "masters": masters})).collect();
    let mut tags: Vec<String> = ix.masters.iter().flat_map(|m| m.tags.iter().cloned()).collect();
    tags.sort();
    tags.dedup();
    Ok(
        json!({"folder": folder.to_string_lossy(), "count": ix.masters.len(), "divisions": list, "tags": tags, "indexed": folder.join(INDEX_FILE).is_file()}),
    )
}

fn search_cmd(_: &mut Session, v: &Value) -> CmdResult {
    let folder = folder_param(v)?;
    let ix = load(&folder)?;
    let query = p::str(v, "query").unwrap_or("");
    if query.chars().count() > 200 {
        return Err(CmdError::Params("`query` is longer than 200 characters".into()));
    }
    let results: Vec<Value> = search(&ix, query, p::str(v, "division"), p::str(v, "tag"), p::bool(v, "approved"))
        .into_iter()
        .map(|(s, m, snip)| {
            let mut j = master_json(m);
            j["score"] = json!(s);
            j["snippet"] = json!(snip);
            j
        })
        .collect();
    Ok(json!({"count": results.len(), "results": results}))
}

fn find<'a>(ix: &'a Index, v: &Value) -> Result<&'a Master, CmdError> {
    let file = p::req_str(v, "file")?.trim();
    ix.masters.iter().find(|m| m.file == file).ok_or_else(|| CmdError::Failed(format!("{file:?} is not in the library index (scan the library?)")))
}

fn preview_cmd(_: &mut Session, v: &Value) -> CmdResult {
    let folder = folder_param(v)?;
    let ix = load(&folder)?;
    let m = find(&ix, v)?;
    let mut j = master_json(m);
    j["path"] = json!(folder.join(&m.file).to_string_lossy());
    j["paras"] = json!(m.paras);
    Ok(j)
}

fn tag_cmd(_: &mut Session, v: &Value) -> CmdResult {
    let folder = folder_param(v)?;
    let mut ix = load(&folder)?;
    let file = p::req_str(v, "file")?.trim().to_string();
    let m = ix.masters.iter_mut().find(|m| m.file == file).ok_or_else(|| CmdError::Failed(format!("{file:?} is not in the library index")))?;
    if let Some(tags) = v.get("tags") {
        let list = tags.as_array().ok_or_else(|| CmdError::Params("`tags` must be a list of strings".into()))?;
        let mut out: Vec<String> =
            list.iter().filter_map(Value::as_str).map(|t| t.trim().chars().take(60).collect::<String>()).filter(|t| !t.is_empty()).take(50).collect();
        out.sort();
        out.dedup();
        m.tags = out;
    }
    if let Some(a) = p::bool(v, "approved") {
        m.approved = a;
    }
    if let Some(n) = p::str(v, "note") {
        m.note = n.trim().chars().take(1_000).collect();
    }
    let j = master_json(m);
    store(&folder, &ix)?;
    Ok(j)
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

    fn master(number: &str, title: &str, body: &[&str]) -> Document {
        let mut d = Document::new();
        d.body = vec![para("SCT", &format!("SECTION {number} - {title}")), para("PRT", "GENERAL")];
        d.body.extend(body.iter().map(|t| para("PR1", t)));
        d.body.push(para("CMT", "Edit to suit the Project; mention gypsum here only in a note."));
        d.body.push(para("EOS", &format!("END OF SECTION {number}")));
        d
    }

    fn run(id: &str, v: Value) -> Value {
        let mut s = Session::new(Document::new());
        s.run(id, &v).unwrap_or_else(|e| panic!("{id}: {e}"))
    }

    fn library(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("speccraft-library-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("Division 09")).unwrap();
        let save = |doc: &Document, rel: &str| crate::io::save_path(&d.join(rel), doc).unwrap();
        save(&master("09 2900", "GYPSUM BOARD", &["Provide moisture-resistant panels in wet areas."]), "Division 09/092900 Gypsum Board.docx");
        save(&master("09 2116", "GYPSUM BOARD ASSEMBLIES", &["Provide steel framing."]), "Division 09/092116 Gypsum Board Assemblies.docx");
        save(&master("07 9200", "JOINT SEALANTS", &["Provide silicone sealant at wet areas."]), "079200 Joint Sealants.docx");
        save(&Document::new(), "notes.docx");
        // A project copy in the library folder is not a master.
        let mut copy = master("08 7100", "DOOR HARDWARE", &[]);
        copy.set_custom_prop(super::super::project::ROLE_PROP, "project");
        save(&copy, "copy.docx");
        std::fs::write(d.join("~$lock.docx"), b"x").unwrap();
        d
    }

    #[test]
    fn scanning_indexes_masters_by_division_and_keeps_tags() {
        let d = library("scan");
        let f = d.to_string_lossy().to_string();
        let r = run("spec.library.scan", json!({"folder": f}));
        assert_eq!(
            (r["masters"].clone(), r["read"].clone(), r["skippedProjectCopies"].clone(), r["withoutNumber"].clone()),
            (json!(4), json!(4), json!(1), json!(1)),
            "{r}"
        );
        let info = run("spec.library.info", json!({"folder": f}));
        let divs: Vec<(String, String)> = info["divisions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| (d["division"].as_str().unwrap().into(), d["name"].as_str().unwrap().into()))
            .collect();
        assert_eq!(divs, [("".into(), "".into()), ("07".into(), "Thermal and Moisture Protection".into()), ("09".into(), "Finishes".into())]);
        run(
            "spec.library.tag",
            json!({"folder": f, "file": "Division 09/092900 Gypsum Board.docx", "tags": ["office standard", "interiors"], "approved": true, "note": "UH 12.2021"}),
        );
        // Rescanning reads nothing unchanged and keeps the tags; a changed master is read again.
        assert_eq!(run("spec.library.scan", json!({"folder": f}))["read"], 0);
        crate::io::save_path(
            &d.join("Division 09/092900 Gypsum Board.docx"),
            &master("09 2900", "GYPSUM BOARD", &["Changed text, long enough to change the size."]),
        )
        .unwrap();
        let r = run("spec.library.scan", json!({"folder": f}));
        assert_eq!(r["read"], 1);
        let p = run("spec.library.preview", json!({"folder": f, "file": "Division 09/092900 Gypsum Board.docx"}));
        assert_eq!((p["approved"].clone(), p["tags"][0].clone()), (json!(true), json!("interiors")));
        assert_eq!(p["paras"][2]["t"], "Changed text, long enough to change the size.");
        assert_eq!(p["paras"][0]["k"], "title");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn search_by_number_title_text_and_tags() {
        let d = library("search");
        let f = d.to_string_lossy().to_string();
        run("spec.library.scan", json!({"folder": f}));
        let files = |q: Value| -> Vec<String> {
            let mut v = json!({"folder": f});
            if let Value::Object(m) = q {
                for (k, x) in m {
                    v[k] = x;
                }
            }
            run("spec.library.search", v)["results"].as_array().unwrap().iter().map(|r| r["number"].as_str().unwrap().to_string()).collect()
        };
        assert_eq!(files(json!({"query": "0929"})), ["09 2900"]);
        assert_eq!(files(json!({"query": "09 2"})), ["09 2116", "09 2900"]);
        assert_eq!(files(json!({"query": "gypsum"})), ["09 2116", "09 2900"], "title matches, not the note");
        assert_eq!(files(json!({"query": "gypsum board assemblies"})), ["09 2116"]);
        assert_eq!(files(json!({"query": "wet areas"})), ["07 9200", "09 2900"], "text matches");
        assert_eq!(files(json!({"query": "", "division": "07"})), ["07 9200"]);
        let r = run("spec.library.search", json!({"folder": f, "query": "silicone"}));
        assert_eq!(r["results"][0]["snippet"], "Provide silicone sealant at wet areas.");
        run("spec.library.tag", json!({"folder": f, "file": "079200 Joint Sealants.docx", "tags": ["exterior"]}));
        assert_eq!(files(json!({"query": "exterior"})), ["07 9200"]);
        assert_eq!(files(json!({"tag": "Exterior"})), ["07 9200"]);
        assert!(files(json!({"query": "nothing like this"})).is_empty());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_missing_index_is_empty_and_a_bad_one_is_refused() {
        let d = std::env::temp_dir().join(format!("speccraft-library-empty-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(run("spec.library.info", json!({"folder": d.to_string_lossy()}))["count"], 0);
        std::fs::write(d.join(INDEX_FILE), br#"{"format": "other", "version": 1}"#).unwrap();
        let mut s = Session::new(Document::new());
        assert!(s.run("spec.library.info", &json!({"folder": d.to_string_lossy()})).is_err());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
