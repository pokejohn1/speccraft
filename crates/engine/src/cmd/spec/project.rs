//! Specs › Project: the project file that ties section files together (milestone M4). Added by
//! the Speccraft fork; not part of the original WordCraft.
//!
//! A project is a JSON file with the `.speccraft` extension in the same folder as its section
//! files. It holds the project's information (name, number, issue, date), named values used to
//! fill placeholders such as `<Insert Project Name>`, and the list of section files in project
//! order, each with the sections it requires and where it was copied from.
//!
//! Masters are never written. [`add_section`] copies a master into the project folder, imports
//! it, and records in the copy where it came from (`Speccraft.Master`) and a fingerprint of every
//! master paragraph (`Speccraft.Baseline`), the baseline that restoring original wording (M8)
//! and master updates (M11) compare against.
//!
//! The commands never change the open document: each takes the project file's `path` and reads
//! or rewrites that file. Fields this version doesn't know are kept when the file is rewritten.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use wordcraft_doc::{Block, Document, Paragraph};

use super::sections::{self, Kind};
use crate::{CmdError, CmdResult, CommandSpec, Session, p};

const LOC: &str = "Specs › Project";
/// The project file's extension.
pub const EXT: &str = "speccraft";
/// What the project file's `format` field says.
const FORMAT: &str = "speccraft-project";
/// The newest project file version this code writes and understands.
const VERSION: u32 = 1;
/// Largest project file read, in bytes.
const MAX_FILE: u64 = 4 << 20;
/// Most sections in one project.
const MAX_SECTIONS: usize = 2_000;
/// Most named values in one project.
const MAX_VALUES: usize = 500;
/// Longest project field, value name or value, in characters.
const MAX_FIELD: usize = 500;
/// Section file property: what this file is (`project` for a project copy).
pub const ROLE_PROP: &str = "Speccraft.Role";
/// Section file property: the master it was copied from (JSON: file, version, fingerprint).
pub const MASTER_PROP: &str = "Speccraft.Master";
/// Section file property: `id:fingerprint` of each spec item's text when it was copied.
pub const BASELINE_PROP: &str = "Speccraft.Baseline";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("spec.project.new", "New Project", LOC, new_project)
            .params(r#"{"path": string (the project's .speccraft file), "name": string, "number"?: string, "issue"?: string, "date"?: string}"#)
            .pure(),
        CommandSpec::new("spec.project.info", "Project Information", LOC, info).params(r#"{"path": string (the project's .speccraft file)}"#).pure(),
        CommandSpec::new("spec.project.set", "Set Project Information", LOC, set)
            .params(r#"{"path": string (the project's .speccraft file), "name"?: string, "number"?: string, "issue"?: string, "date"?: string, "values"?: {name: string or null (removes)}, "library"?: string or null (the master library folder)}"#)
            .pure(),
        CommandSpec::new("spec.project.addSection", "Add Section to Project", LOC, add_section)
            .params(r#"{"path": string (the project's .speccraft file), "master"?: string (a master .docx to copy into the project folder), "file"?: string (file name in the project folder: the copy's name, or an existing section file to add as it is)}"#)
            .pure(),
        CommandSpec::new("spec.project.removeSection", "Remove Section from Project", LOC, remove_section)
            .params(r#"{"path": string (the project's .speccraft file), "file": string} (the file itself is kept)"#)
            .pure(),
        CommandSpec::new("spec.project.include", "Include Section", LOC, include)
            .params(r#"{"path": string (the project's .speccraft file), "file": string, "included": bool}"#)
            .pure(),
        CommandSpec::new("spec.project.order", "Order Sections", LOC, order)
            .params(r#"{"path": string (the project's .speccraft file), "files"?: [string] (the new order; omitted: by section number)}"#)
            .pure(),
        CommandSpec::new("spec.project.requires", "Set Required Sections", LOC, requires)
            .params(r#"{"path": string (the project's .speccraft file), "file": string, "requires": [string] (section numbers, e.g. "07 9200")}"#)
            .pure(),
        CommandSpec::new("spec.project.addRule", "Add Rule Between Sections", LOC, add_rule)
            .params(r#"{"path": string (the project's .speccraft file), "kind": "require"|"recommend"|"warn"|"exclude", "if": [{"section": "09 2900", "on"?: id} | {"value": name, "is"?: string}], "target": {"section": "07 9200", "on"?: id}, "note"?: string}"#)
            .pure(),
        CommandSpec::new("spec.project.removeRule", "Remove Rule Between Sections", LOC, remove_rule)
            .params(r#"{"path": string (the project's .speccraft file), "id": string}"#)
            .pure(),
        CommandSpec::new("spec.project.validate", "Check Project", LOC, validate).params(r#"{"path": string (the project's .speccraft file)}"#).pure(),
        CommandSpec::new("spec.project.publish", "Publish Project Sections", LOC, publish)
            .params(r#"{"path": string (the project's .speccraft file), "folder"?: string (default: Published beside the project file), "force"?: bool (publish even when Check Project finds errors), "format"?: "pdf" (default), "docx" or "both", "form"?: "full" (default), "short" or "outline", "depth"?: n}"#)
            .pure(),
        CommandSpec::new("spec.project.acknowledge", "Acknowledge Finding", LOC, acknowledge)
            .params(r#"{"path": string (the project's .speccraft file), "key": string (a warning or notice's key from spec.project.validate), "note"?: string, "undo"?: bool}"#)
            .pure(),
    ]
}

// ---------------------------------------------------------------------------------------------
// The project file

/// The project file. `extra` keeps fields this version doesn't know.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProjectFile {
    pub format: String,
    pub version: u32,
    #[serde(default)]
    pub project: Info,
    /// Named values for placeholders: `"Architect Name": "…"` fills `<Insert Architect Name>`.
    #[serde(default)]
    pub values: BTreeMap<String, String>,
    #[serde(default)]
    pub sections: Vec<SectionRef>,
    /// The master library folder this project draws from (M9).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library: Option<String>,
    /// Rules between sections (M6), checked by `spec.project.validate`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<ProjectRule>,
    /// Warnings and notices the author has looked at and accepted (M10), by finding key.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acknowledged: Vec<Acknowledged>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A finding the author accepted, with why.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Acknowledged {
    pub key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A condition or target of a rule between sections: a section (by number) is in the project,
/// optionally with one of its paragraphs printing; or a project value equals something.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    /// A paragraph id in that section that must print.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<String>,
    /// A project value's name (or "Project Name", "Issue", …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is: Option<String>,
}

/// A rule between sections: the same kinds as a section's rules.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRule {
    pub id: String,
    pub kind: super::rules::RuleKind,
    #[serde(rename = "if")]
    pub when: Vec<ProjectRef>,
    pub target: ProjectRef,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// Project-wide information.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Info {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub number: String,
    /// Issue designation, e.g. "Issued for Bid".
    #[serde(default)]
    pub issue: String,
    /// Issue date, as the author writes it.
    #[serde(default)]
    pub date: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One section file of the project.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SectionRef {
    /// File name in the project folder (no folders).
    pub file: String,
    /// Section number as read from the section title, e.g. "09 2900".
    #[serde(default)]
    pub number: String,
    #[serde(default)]
    pub title: String,
    #[serde(default = "yes")]
    pub included: bool,
    /// Numbers of sections this one requires.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub master: Option<MasterRef>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn yes() -> bool {
    true
}

/// Where a project copy came from.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MasterRef {
    /// The master's path as given when the copy was made.
    pub path: String,
    /// The master's version stamp, if one was found in its headers or footers.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version: String,
    /// Fingerprint of the master file's bytes, to notice a newer master.
    #[serde(default)]
    pub fingerprint: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// The `path` param: a `.speccraft` file.
fn project_path(v: &Value) -> Result<PathBuf, CmdError> {
    let path = PathBuf::from(p::req_str(v, "path")?);
    if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case(EXT)) || path.file_stem().is_none() {
        return Err(CmdError::Params(format!("`path` must name a .{EXT} project file")));
    }
    Ok(path)
}

/// The folder a project's section files are in.
fn folder(path: &Path) -> PathBuf {
    path.parent().map(Path::to_path_buf).unwrap_or_default()
}

/// Read and check a project file.
pub fn load(path: &Path) -> Result<ProjectFile, CmdError> {
    let bytes = crate::io::read_document(path).map_err(CmdError::Failed)?;
    if bytes.len() as u64 > MAX_FILE {
        return Err(CmdError::Failed(format!("{}: project file is larger than 4 MB", path.display())));
    }
    let f: ProjectFile =
        serde_json::from_slice(&bytes).map_err(|e| CmdError::Failed(format!("{}: not a Speccraft project file ({e})", path.display())))?;
    if f.format != FORMAT {
        return Err(CmdError::Failed(format!("{}: not a Speccraft project file", path.display())));
    }
    if f.version > VERSION {
        return Err(CmdError::Failed(format!(
            "{}: made by a newer Speccraft (project file version {}); this one reads version {VERSION}",
            path.display(),
            f.version
        )));
    }
    if f.sections.len() > MAX_SECTIONS || f.values.len() > MAX_VALUES {
        return Err(CmdError::Failed(format!("{}: too many sections or values", path.display())));
    }
    Ok(f)
}

/// Write a project file (pretty JSON, so it reads well and diffs well).
fn store(path: &Path, f: &ProjectFile) -> Result<(), CmdError> {
    let text = serde_json::to_string_pretty(f).map_err(|e| CmdError::Failed(e.to_string()))?;
    crate::io::write_file(&path.to_string_lossy(), format!("{text}\n").as_bytes()).map_err(CmdError::Failed)
}

/// A project field param, trimmed and length-checked.
fn field(v: &Value, k: &str) -> Result<Option<String>, CmdError> {
    match v.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.chars().count() <= MAX_FIELD => Ok(Some(s.trim().to_string())),
        Some(Value::String(_)) => Err(CmdError::Params(format!("`{k}` is longer than {MAX_FIELD} characters"))),
        Some(_) => Err(CmdError::Params(format!("`{k}` must be a string"))),
    }
}

/// A section file name: a bare `.docx` name, no folders.
fn file_name(s: &str) -> Result<String, CmdError> {
    let s = s.trim();
    let bare = !s.is_empty()
        && s.chars().count() <= 255
        && !s.contains(['/', '\\', ':'])
        && s != "."
        && s != ".."
        && Path::new(s).extension().is_some_and(|e| e.eq_ignore_ascii_case("docx"));
    if bare { Ok(s.to_string()) } else { Err(CmdError::Params(format!("{s:?} must be a .docx file name with no folders"))) }
}

/// The section entry for `file`.
fn entry<'a>(f: &'a mut ProjectFile, file: &str) -> Result<&'a mut SectionRef, CmdError> {
    f.sections
        .iter_mut()
        .find(|s| s.file.eq_ignore_ascii_case(file))
        .ok_or_else(|| CmdError::Failed(format!("{file:?} is not a section of this project")))
}

// ---------------------------------------------------------------------------------------------
// Section numbers, fingerprints, placeholders

/// A section number's digits, so "09 2900", "092900" and "09 29 00" compare equal. Empty when
/// `s` has fewer than five digits.
pub fn number_key(s: &str) -> String {
    let d: String = s.chars().filter(char::is_ascii_digit).take(8).collect();
    if d.len() >= 5 { d } else { String::new() }
}

/// The number and title of a section title such as "SECTION 09 2900 - GYPSUM BOARD".
pub fn split_title(text: &str) -> (String, String) {
    let t = text.trim();
    let rest = match t.get(..8) {
        Some(head) if head.eq_ignore_ascii_case("section ") => t.get(8..).unwrap_or(""),
        _ => t,
    };
    let num_len = rest.char_indices().find(|(_, c)| !(c.is_ascii_digit() || *c == ' ' || *c == '.')).map_or(rest.len(), |(i, _)| i);
    let number = rest.get(..num_len).unwrap_or("").trim().to_string();
    if number_key(&number).is_empty() {
        return (String::new(), t.to_string());
    }
    let title = rest.get(num_len..).unwrap_or("").trim_start_matches(|c: char| c == '-' || c == '–' || c == '—' || c.is_whitespace());
    (number, title.trim().to_string())
}

/// 64-bit FNV-1a, as 16 hex digits: a fingerprint to notice changes, not a security hash.
pub fn fingerprint(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// The text of a spec item, as its fingerprint sees it.
fn entry_text(doc: &Document, e: &sections::Entry) -> String {
    let i = e.range.start.path.0.first().copied().unwrap_or(u32::MAX) as usize;
    doc.body.get(i).and_then(|b| b.as_para()).map(|p| p.plain_text().replace(wordcraft_doc::para::OBJ, "")).unwrap_or_default()
}

/// Every paragraph of every story (body, headers, footers, notes, text boxes), table cells included.
fn all_paragraphs(doc: &Document) -> Vec<&Paragraph> {
    fn walk<'a>(blocks: &'a [Arc<Block>], out: &mut Vec<&'a Paragraph>) {
        for b in blocks {
            match b.as_ref() {
                Block::Para(p) => out.push(p),
                Block::Table(t) => t.rows.iter().flat_map(|r| &r.cells).for_each(|c| walk(&c.blocks, out)),
            }
        }
    }
    let mut out = Vec::new();
    walk(&doc.body, &mut out);
    doc.parts.values().for_each(|part| walk(&part.blocks, &mut out));
    out
}

/// The text of the section's headers and footers.
fn furniture_text(doc: &Document) -> Vec<String> {
    let mut out = Vec::new();
    for part in doc.parts.values() {
        if matches!(part.kind, wordcraft_doc::PartKind::Header | wordcraft_doc::PartKind::Footer) {
            for b in &part.blocks {
                if let Some(p) = b.as_para() {
                    out.push(p.plain_text().replace(wordcraft_doc::para::OBJ, ""));
                }
            }
        }
    }
    out
}

/// A master's version stamp: the first header or footer piece mentioning "master", "version" or
/// "rev" (the University of Houston sample has "UH Master: 12.2021").
pub fn version_stamp(doc: &Document) -> String {
    for line in furniture_text(doc) {
        for piece in line.split('\t') {
            let low = piece.to_lowercase();
            if ["master", "version", "rev."].iter().any(|w| low.contains(w)) {
                return piece.trim().chars().take(120).collect();
            }
        }
    }
    String::new()
}

/// The placeholder values of a project: its named values, then the built-in fields under their
/// usual names. Keys are lowercased.
pub fn placeholder_values(f: &ProjectFile) -> BTreeMap<String, String> {
    let mut m: BTreeMap<String, String> = f.values.iter().map(|(k, v)| (k.trim().to_lowercase(), v.clone())).collect();
    let builtin = [
        ("project name", &f.project.name),
        ("project number", &f.project.number),
        ("issue", &f.project.issue),
        ("issue date", &f.project.date),
        ("date", &f.project.date),
    ];
    for (k, v) in builtin {
        if !v.is_empty() {
            m.entry(k.to_string()).or_insert_with(|| v.clone());
        }
    }
    m
}

/// `<Insert …>` placeholders in `text`: (byte range, the name inside, lowercased).
fn insert_placeholders(text: &str) -> Vec<(std::ops::Range<usize>, String)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(a) = text.get(from..).and_then(|t| t.find('<')).map(|i| i + from) {
        let Some(b) = text.get(a..).and_then(|t| t.find('>')).map(|i| i + a) else { break };
        let inner = text.get(a + 1..b).unwrap_or("");
        let name = inner.trim().strip_prefix("Insert ").or_else(|| inner.trim().strip_prefix("insert ")).map(str::trim);
        match name {
            Some(n) if !n.is_empty() && n.chars().count() <= 80 && !n.contains(['<', wordcraft_doc::para::OBJ]) => {
                out.push((a..b + 1, n.to_lowercase()));
                from = b + 1;
            }
            _ => from = a + 1,
        }
    }
    out
}

/// Whether a fill-in's original wording is a placeholder the project fills (`<Insert Project
/// Name>` with a project name set, `[Owner Name]` with that value set).
pub fn filled_by_project(original: &str, values: &BTreeMap<String, String>) -> bool {
    let t = original.trim();
    let name = t
        .strip_prefix('<')
        .and_then(|x| x.strip_suffix('>'))
        .and_then(|x| x.trim().strip_prefix("Insert ").or_else(|| x.trim().strip_prefix("insert ")))
        .or_else(|| t.strip_prefix('[').and_then(|x| x.strip_suffix(']')));
    name.is_some_and(|n| values.get(&n.trim().to_lowercase()).is_some_and(|v| !v.is_empty()))
}

/// `[Name]` placeholders in `text` whose name is a project value: (byte range, name lowercased).
fn bracket_placeholders(text: &str, values: &BTreeMap<String, String>) -> Vec<(std::ops::Range<usize>, String)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(a) = text.get(from..).and_then(|t| t.find('[')).map(|i| i + from) {
        let Some(b) = text.get(a..).and_then(|t| t.find(']')).map(|i| i + a) else { break };
        let name = text.get(a + 1..b).unwrap_or("").trim().to_lowercase();
        if values.contains_key(&name) {
            out.push((a..b + 1, name));
            from = b + 1;
        } else {
            from = a + 1;
        }
    }
    out
}

/// Replace placeholders that have a value in every story of `doc`, keeping each placeholder's
/// formatting. Returns how many were filled and the names left without a value.
pub fn fill_placeholders(doc: &mut Document, values: &BTreeMap<String, String>) -> (usize, BTreeSet<String>) {
    fn walk(blocks: &mut [Arc<Block>], values: &BTreeMap<String, String>, filled: &mut usize, missing: &mut BTreeSet<String>) {
        for b in blocks {
            match Arc::make_mut(b) {
                Block::Para(p) => fill_para(p, values, filled, missing),
                Block::Table(t) => {
                    for c in t.rows.iter_mut().flat_map(|r| r.cells.iter_mut()) {
                        walk(&mut c.blocks, values, filled, missing);
                    }
                }
            }
        }
    }
    let (mut filled, mut missing) = (0, BTreeSet::new());
    walk(&mut doc.body, values, &mut filled, &mut missing);
    for part in doc.parts.values_mut() {
        walk(&mut part.blocks, values, &mut filled, &mut missing);
    }
    (filled, missing)
}

fn fill_para(p: &mut Paragraph, values: &BTreeMap<String, String>, filled: &mut usize, missing: &mut BTreeSet<String>) {
    let mut found = insert_placeholders(&p.text);
    found.extend(bracket_placeholders(&p.text, values));
    found.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
    let mut changed = false;
    for (r, name) in found {
        match values.get(&name) {
            Some(v) if !v.is_empty() => {
                let props = p.props_at(r.start).clone();
                if p.delete(r.start, r.end).is_ok() && p.insert_text(r.start, v, &props).is_ok() {
                    *filled += 1;
                    changed = true;
                }
            }
            _ => {
                missing.insert(name);
            }
        }
    }
    if changed {
        p.touch();
    }
}

/// The project file for a section at `section`: `explicit` if given, else the only `.speccraft`
/// file in the section's folder. `Ok(None)` when there is none (or more than one).
pub fn project_for(section: Option<&Path>, explicit: Option<&str>) -> Result<Option<PathBuf>, CmdError> {
    if let Some(p) = explicit {
        return Ok(Some(PathBuf::from(p)));
    }
    let Some(dir) = section.and_then(Path::parent) else { return Ok(None) };
    let dir = if dir.as_os_str().is_empty() { Path::new(".") } else { dir };
    #[cfg(not(target_arch = "wasm32"))]
    {
        let Ok((entries, _)) = crate::io::list_dir(dir, 10_000) else { return Ok(None) };
        let found: Vec<PathBuf> = entries
            .into_iter()
            .filter(|e| !e.dir && Path::new(&e.name).extension().is_some_and(|x| x.eq_ignore_ascii_case(EXT)))
            .map(|e| e.path)
            .take(2)
            .collect();
        Ok(if found.len() == 1 { found.into_iter().next() } else { None })
    }
    #[cfg(target_arch = "wasm32")]
    Ok(None)
}

// ---------------------------------------------------------------------------------------------
// Commands

fn new_project(_: &mut Session, v: &Value) -> CmdResult {
    let path = project_path(v)?;
    if path.exists() {
        return Err(CmdError::Failed(format!("{} already exists", path.display())));
    }
    let name = field(v, "name")?.filter(|n| !n.is_empty()).ok_or_else(|| CmdError::Params("give the project a `name`".into()))?;
    let f = ProjectFile {
        format: FORMAT.into(),
        version: VERSION,
        project: Info {
            name,
            number: field(v, "number")?.unwrap_or_default(),
            issue: field(v, "issue")?.unwrap_or_default(),
            date: field(v, "date")?.unwrap_or_default(),
            extra: Map::new(),
        },
        ..Default::default()
    };
    store(&path, &f)?;
    Ok(summary(&path, &f))
}

/// What `spec.project.info` and the editing commands return.
fn summary(path: &Path, f: &ProjectFile) -> Value {
    let dir = folder(path);
    let sections: Vec<Value> = f
        .sections
        .iter()
        .map(|s| {
            json!({
                "file": s.file,
                "number": s.number,
                "title": s.title,
                "included": s.included,
                "requires": s.requires,
                "exists": dir.join(&s.file).is_file(),
                "master": s.master.as_ref().map(|m| json!({"path": m.path, "version": m.version})),
            })
        })
        .collect();
    json!({
        "path": path.to_string_lossy(),
        "folder": dir.to_string_lossy(),
        "name": f.project.name,
        "number": f.project.number,
        "issue": f.project.issue,
        "date": f.project.date,
        "values": f.values,
        "sections": sections,
        "rules": f.rules,
        "library": f.library,
    })
}

fn info(_: &mut Session, v: &Value) -> CmdResult {
    let path = project_path(v)?;
    let f = load(&path)?;
    Ok(summary(&path, &f))
}

fn set(_: &mut Session, v: &Value) -> CmdResult {
    let path = project_path(v)?;
    let mut f = load(&path)?;
    for (k, slot) in
        [("name", &mut f.project.name), ("number", &mut f.project.number), ("issue", &mut f.project.issue), ("date", &mut f.project.date)]
    {
        if let Some(x) = field(v, k)? {
            *slot = x;
        }
    }
    if f.project.name.is_empty() {
        return Err(CmdError::Params("the project `name` can't be empty".into()));
    }
    match v.get("library") {
        None => {}
        Some(Value::Null) => f.library = None,
        Some(Value::String(l)) if l.chars().count() <= 1_000 => f.library = Some(l.trim().to_string()).filter(|l| !l.is_empty()),
        Some(_) => return Err(CmdError::Params("`library` must be a folder path or null".into())),
    }
    match v.get("values") {
        None | Some(Value::Null) => {}
        Some(Value::Object(m)) => {
            for (k, x) in m {
                let k = k.trim();
                if k.is_empty() || k.chars().count() > MAX_FIELD || k.contains(['<', '>', '[', ']']) {
                    return Err(CmdError::Params(format!("value name {k:?} must be 1 to {MAX_FIELD} characters without < > [ ]")));
                }
                match x {
                    Value::Null => {
                        f.values.remove(k);
                    }
                    Value::String(s) if s.chars().count() <= MAX_FIELD => {
                        f.values.insert(k.to_string(), s.trim().to_string());
                    }
                    _ => return Err(CmdError::Params(format!("value {k:?} must be a string of at most {MAX_FIELD} characters, or null"))),
                }
            }
            if f.values.len() > MAX_VALUES {
                return Err(CmdError::Params(format!("a project holds at most {MAX_VALUES} values")));
            }
        }
        Some(_) => return Err(CmdError::Params("`values` must be an object".into())),
    }
    store(&path, &f)?;
    Ok(summary(&path, &f))
}

/// The number and title of an imported section, from its title paragraph.
fn number_and_title(doc: &Document) -> (String, String) {
    let entries = sections::outline(doc);
    entries.iter().find(|e| e.kind == Kind::Title).map(|e| split_title(&entry_text(doc, e))).unwrap_or_default()
}

/// Refuse a section number the project already has.
fn duplicate_check(number: &str, taken: &[SectionRef]) -> Result<(), CmdError> {
    let key = number_key(number);
    match taken.iter().find(|s| !key.is_empty() && number_key(&s.number) == key) {
        Some(other) => Err(CmdError::Failed(format!("the project already has Section {number} ({}); remove it first to add another", other.file))),
        None => Ok(()),
    }
}

/// Copy `master` into the project folder as `target`, import it, and record where it came from.
fn copy_master(master: &Path, target: &Path, taken: &[SectionRef]) -> Result<(Document, MasterRef), CmdError> {
    let bytes = crate::io::read_document(master).map_err(CmdError::Failed)?;
    let doc = crate::io::open_bytes(&master.to_string_lossy(), &bytes).map_err(CmdError::Failed)?;
    if doc.custom_prop(ROLE_PROP) == Some("project") {
        return Err(CmdError::Failed(format!("{} is already a project copy, not a master: add it with `file` instead", master.display())));
    }
    let mut s = Session::new(doc);
    s.run("spec.import", &json!({}))?;
    // A section the project already has is refused before anything is written.
    duplicate_check(&number_and_title(&s.doc).0, taken)?;
    // Choices, blanks and fill-ins become fields (M7); a section with none is fine.
    super::fields::detect(&mut s.doc)?;
    let baseline: Vec<String> =
        sections::outline(&s.doc).iter().map(|e| format!("{}:{}", e.id, fingerprint(entry_text(&s.doc, e).as_bytes()))).collect();
    let m = MasterRef { path: master.to_string_lossy().into(), version: version_stamp(&s.doc), fingerprint: fingerprint(&bytes), extra: Map::new() };
    s.doc.set_custom_prop(ROLE_PROP, "project");
    s.doc.set_custom_prop(MASTER_PROP, &json!({"path": m.path, "version": m.version, "fingerprint": m.fingerprint}).to_string());
    s.doc.set_custom_prop(BASELINE_PROP, &baseline.join(","));
    // The master's order, for comparing with a newer master (M11).
    let order: Vec<String> = sections::outline(&s.doc).into_iter().map(|e| e.id).collect();
    s.doc.set_custom_prop(super::update::ORDER_PROP, &order.join(","));
    crate::io::save_path(target, &s.doc).map_err(CmdError::Failed)?;
    Ok((s.doc, m))
}

fn add_section(_: &mut Session, v: &Value) -> CmdResult {
    let path = project_path(v)?;
    let mut f = load(&path)?;
    if f.sections.len() >= MAX_SECTIONS {
        return Err(CmdError::Failed(format!("a project holds at most {MAX_SECTIONS} sections")));
    }
    let dir = folder(&path);
    let master = p::str(v, "master").map(PathBuf::from);
    let named = p::str(v, "file").map(file_name).transpose()?;
    let (file, doc, master_ref) = match master {
        Some(master) => {
            let file = match named {
                Some(n) => n,
                None => file_name(&master.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())?,
            };
            let target = dir.join(&file);
            if target.exists() {
                return Err(CmdError::Failed(format!("{} already exists in the project folder: give another `file` name", file)));
            }
            if master.canonicalize().ok() == target.canonicalize().ok() && master.exists() {
                return Err(CmdError::Failed("the copy can't replace its master".into()));
            }
            let (doc, m) = copy_master(&master, &target, &f.sections)?;
            (file, doc, Some(m))
        }
        None => {
            let file =
                named.ok_or_else(|| CmdError::Params("give `master` (a master to copy) or `file` (a section file in the project folder)".into()))?;
            let bytes = crate::io::read_document(&dir.join(&file)).map_err(CmdError::Failed)?;
            let doc = crate::io::open_bytes(&file, &bytes).map_err(CmdError::Failed)?;
            let m = doc.custom_prop(MASTER_PROP).and_then(|s| serde_json::from_str::<MasterRef>(s).ok());
            (file, doc, m)
        }
    };
    if f.sections.iter().any(|s| s.file.eq_ignore_ascii_case(&file)) {
        return Err(CmdError::Failed(format!("{file:?} is already in this project")));
    }
    let (number, title) = number_and_title(&doc);
    duplicate_check(&number, &f.sections)?;
    f.sections.push(SectionRef { file: file.clone(), number, title, included: true, requires: Vec::new(), master: master_ref, extra: Map::new() });
    sort_by_number(&mut f.sections);
    store(&path, &f)?;
    let mut out = summary(&path, &f);
    if let Some(o) = out.as_object_mut() {
        o.insert("added".into(), json!(file));
    }
    Ok(out)
}

/// Sections in section-number order; sections without a number go last, in their order.
fn sort_by_number(sections: &mut [SectionRef]) {
    sections.sort_by(|a, b| {
        let (ka, kb) = (number_key(&a.number), number_key(&b.number));
        (ka.is_empty(), ka).cmp(&(kb.is_empty(), kb))
    });
}

fn remove_section(_: &mut Session, v: &Value) -> CmdResult {
    let path = project_path(v)?;
    let mut f = load(&path)?;
    let file = p::req_str(v, "file")?;
    let n = f.sections.len();
    f.sections.retain(|s| !s.file.eq_ignore_ascii_case(file.trim()));
    if f.sections.len() == n {
        return Err(CmdError::Failed(format!("{file:?} is not a section of this project")));
    }
    store(&path, &f)?;
    Ok(summary(&path, &f))
}

fn include(_: &mut Session, v: &Value) -> CmdResult {
    let path = project_path(v)?;
    let mut f = load(&path)?;
    let on = p::bool(v, "included").ok_or_else(|| CmdError::Params("`included` must be true or false".into()))?;
    entry(&mut f, p::req_str(v, "file")?.trim())?.included = on;
    store(&path, &f)?;
    Ok(summary(&path, &f))
}

fn order(_: &mut Session, v: &Value) -> CmdResult {
    let path = project_path(v)?;
    let mut f = load(&path)?;
    match v.get("files") {
        None | Some(Value::Null) => sort_by_number(&mut f.sections),
        Some(Value::Array(files)) => {
            let names: Vec<&str> = files.iter().filter_map(Value::as_str).map(str::trim).collect();
            let mut seen = BTreeSet::new();
            let ok = names.len() == files.len()
                && names.len() == f.sections.len()
                && names.iter().all(|n| seen.insert(n.to_lowercase()) && f.sections.iter().any(|s| s.file.eq_ignore_ascii_case(n)));
            if !ok {
                return Err(CmdError::Params("`files` must list every section file of the project exactly once".into()));
            }
            let mut old = std::mem::take(&mut f.sections);
            for n in names {
                if let Some(i) = old.iter().position(|s| s.file.eq_ignore_ascii_case(n)) {
                    f.sections.push(old.remove(i));
                }
            }
        }
        Some(_) => return Err(CmdError::Params("`files` must be a list of file names".into())),
    }
    store(&path, &f)?;
    Ok(summary(&path, &f))
}

fn requires(_: &mut Session, v: &Value) -> CmdResult {
    let path = project_path(v)?;
    let mut f = load(&path)?;
    let Some(list) = v.get("requires").and_then(Value::as_array) else {
        return Err(CmdError::Params("`requires` must be a list of section numbers".into()));
    };
    let mut numbers = Vec::new();
    for x in list.iter().take(200) {
        let n = x.as_str().map(str::trim).unwrap_or("");
        if number_key(n).is_empty() || n.chars().count() > 20 {
            return Err(CmdError::Params(format!("{x} is not a section number such as \"07 9200\"")));
        }
        numbers.push(n.to_string());
    }
    entry(&mut f, p::req_str(v, "file")?.trim())?.requires = numbers;
    store(&path, &f)?;
    Ok(summary(&path, &f))
}

/// One finding of [`validate`].
fn finding(severity: &str, code: &str, file: Option<&str>, message: String) -> Value {
    json!({"severity": severity, "code": code, "file": file, "message": message})
}

/// Section numbers referred to in `text` as "Section 07 9200" (or "Section 079200").
pub fn referenced_sections(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let low = text.to_lowercase();
    let mut from = 0;
    while let Some(i) = low.get(from..).and_then(|t| t.find("section ")).map(|i| i + from) {
        let rest = text.get(i + 8..).unwrap_or("");
        let len = rest.char_indices().find(|(_, c)| !(c.is_ascii_digit() || *c == ' ')).map_or(rest.len(), |(k, _)| k);
        let num = rest.get(..len).unwrap_or("").trim();
        if num.chars().filter(char::is_ascii_digit).count() == 6 {
            out.push(num.to_string());
        }
        from = i + 8;
    }
    out
}

fn add_rule(_: &mut Session, v: &Value) -> CmdResult {
    let path = project_path(v)?;
    let mut f = load(&path)?;
    let bad = || CmdError::Params("a rule needs `kind`, `if` (1 to 20 conditions) and `target` (a section)".into());
    let kind: super::rules::RuleKind = serde_json::from_value(v.get("kind").cloned().unwrap_or(Value::Null)).map_err(|_| bad())?;
    let when: Vec<ProjectRef> = serde_json::from_value(v.get("if").cloned().unwrap_or(Value::Null)).map_err(|_| bad())?;
    let target: ProjectRef = serde_json::from_value(v.get("target").cloned().unwrap_or(Value::Null)).map_err(|_| bad())?;
    if when.is_empty() || when.len() > 20 || f.rules.len() >= 2_000 {
        return Err(bad());
    }
    for c in when.iter().chain(std::iter::once(&target)) {
        let ok = match (&c.section, &c.value) {
            (Some(sec), None) => !number_key(sec).is_empty() && c.is.is_none(),
            (None, Some(name)) => !name.trim().is_empty() && c.on.is_none(),
            _ => false,
        };
        if !ok {
            return Err(CmdError::Params(
                "each condition is a section number (with an optional paragraph `on`) or a project `value` (with an optional `is`)".into(),
            ));
        }
    }
    if target.section.is_none() {
        return Err(CmdError::Params("the target must be a section (with an optional paragraph `on`)".into()));
    }
    let note = p::str(v, "note").unwrap_or("").trim().chars().take(500).collect::<String>();
    let next = f.rules.iter().filter_map(|r| r.id.strip_prefix('R').and_then(|n| n.parse::<u32>().ok())).max().unwrap_or(0).saturating_add(1);
    let rule = ProjectRule { id: format!("R{next:03}"), kind, when, target, note };
    f.rules.push(rule.clone());
    store(&path, &f)?;
    Ok(json!({"added": rule.id, "rules": f.rules}))
}

fn remove_rule(_: &mut Session, v: &Value) -> CmdResult {
    let path = project_path(v)?;
    let mut f = load(&path)?;
    let id = p::req_str(v, "id")?.trim().to_string();
    let n = f.rules.len();
    f.rules.retain(|r| r.id != id);
    if f.rules.len() == n {
        return Err(CmdError::Failed(format!("no rule called {id:?}")));
    }
    store(&path, &f)?;
    Ok(json!({"removed": id, "rules": f.rules}))
}

/// Findings of the rules between sections, given what is in the project and what prints.
fn check_project_rules(
    f: &ProjectFile,
    values: &BTreeMap<String, String>,
    have: &BTreeSet<String>,
    printing_by_number: &BTreeMap<String, std::collections::HashSet<String>>,
    out: &mut Vec<Value>,
) {
    let holds = |r: &ProjectRef| -> bool {
        if let Some(name) = &r.value {
            let v = values.get(&name.trim().to_lowercase()).map(|v| v.trim().to_string()).unwrap_or_default();
            return match &r.is {
                Some(want) => v.eq_ignore_ascii_case(want.trim()),
                None => !v.is_empty(),
            };
        }
        let Some(sec) = &r.section else { return false };
        let key = number_key(sec);
        if !have.contains(&key) {
            return false;
        }
        match &r.on {
            Some(id) => printing_by_number.get(&key).is_some_and(|p| p.contains(id)),
            None => true,
        }
    };
    let describe = |r: &ProjectRef| match (&r.section, &r.on, &r.value, &r.is) {
        (Some(sec), Some(id), _, _) => format!("{id} of Section {sec}"),
        (Some(sec), None, _, _) => format!("Section {sec}"),
        (_, _, Some(v), Some(is)) => format!("{v} = {is}"),
        (_, _, Some(v), None) => v.clone(),
        _ => "?".into(),
    };
    for r in &f.rules {
        if r.when.is_empty() || !r.when.iter().all(holds) {
            continue;
        }
        let because = r.when.iter().map(describe).collect::<Vec<_>>().join(" and ");
        let note = if r.note.is_empty() { String::new() } else { format!(" ({})", r.note) };
        let target = describe(&r.target);
        let present = holds(&r.target);
        use super::rules::RuleKind;
        let found = match (r.kind, present) {
            (RuleKind::Require, false) => Some(("error", format!("rule {}: {because}, so {target} is required{note}", r.id))),
            (RuleKind::Recommend, false) => Some(("notice", format!("rule {}: {because}, so {target} is recommended{note}", r.id))),
            (RuleKind::Warn, true) => Some(("warning", format!("rule {}: {target} is in, but {because}{note}", r.id))),
            (RuleKind::Exclude, true) => Some(("error", format!("rule {}: {target} must not be in while {because}{note}", r.id))),
            _ => None,
        };
        if let Some((sev, m)) = found {
            out.push(json!({"severity": sev, "code": "rule", "rule": r.id, "file": Value::Null, "message": m}));
        }
    }
}

fn validate(_: &mut Session, v: &Value) -> CmdResult {
    let path = project_path(v)?;
    let f = load(&path)?;
    let dir = folder(&path);
    let values = placeholder_values(&f);
    let mut out = Vec::new();
    if f.project.name.is_empty() {
        out.push(finding("warning", "projectName", None, "the project has no name".into()));
    }
    let included: Vec<&SectionRef> = f.sections.iter().filter(|s| s.included).collect();
    let mut by_number: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for s in &included {
        by_number.entry(number_key(&s.number)).or_default().push(&s.file);
    }
    for (k, files) in &by_number {
        if !k.is_empty() && files.len() > 1 {
            out.push(finding("error", "duplicateNumber", None, format!("section number {k} is used by {}", files.join(", "))));
        }
    }
    let have: BTreeSet<String> = by_number.keys().filter(|k| !k.is_empty()).cloned().collect();
    // What prints in each included section, by section number (for rules between sections).
    let mut printing_by_number: BTreeMap<String, std::collections::HashSet<String>> = BTreeMap::new();
    for s in &included {
        let file = Some(s.file.as_str());
        for r in &s.requires {
            if !have.contains(&number_key(r)) {
                out.push(finding("error", "requiredMissing", file, format!("requires section {r}, which is not in the project")));
            }
        }
        let full = dir.join(&s.file);
        let doc = match crate::io::read_document(&full).and_then(|b| crate::io::open_bytes(&s.file, &b)) {
            Ok(d) => d,
            Err(e) => {
                out.push(finding("error", "missingFile", file, format!("can't open the section file: {e}")));
                continue;
            }
        };
        if number_key(&s.number).is_empty() {
            out.push(finding("warning", "noNumber", file, "no section number found in the section title".into()));
        }
        let entries = sections::outline(&doc);
        printing_by_number.insert(number_key(&s.number), entries.iter().filter(|e| e.on).map(|e| e.id.clone()).collect());
        if entries.is_empty() {
            out.push(finding("warning", "notImported", file, "no spec paragraphs yet: open it and import it".into()));
            continue;
        }
        if doc.custom_prop(ROLE_PROP) != Some("project") {
            out.push(finding(
                "warning",
                "notACopy",
                file,
                "this file is not a project copy (it may be a master): masters should be added by copying".into(),
            ));
        }
        // References in what prints, to sections not in the project (at their first mention).
        let mut refs: BTreeMap<String, String> = BTreeMap::new();
        for e in sections::printable(&doc) {
            for r in referenced_sections(&entry_text(&doc, &e)) {
                if !have.contains(&number_key(&r)) && number_key(&r) != number_key(&s.number) {
                    refs.entry(r).or_insert_with(|| e.id.clone());
                }
            }
        }
        for (r, para) in refs {
            let mut x = finding("notice", "referenceMissing", file, format!("refers to Section {r}, which is not in the project"));
            x["para"] = json!(para);
            out.push(x);
        }
        // The section's own checks (M10): open fill-ins, its rules, empty headings, leftovers.
        for mut x in super::quality::check(&doc, &values) {
            x["file"] = json!(s.file);
            out.push(x);
        }
        // Placeholders with no value.
        let mut missing = BTreeSet::new();
        // Those that are fill-ins are reported with the fill-ins (unless the project fills them).
        let as_fields: BTreeSet<String> = super::fields::fields(&doc).into_iter().map(|f| f.original.to_lowercase()).collect();
        for p in all_paragraphs(&doc) {
            for (_, name) in insert_placeholders(&p.text) {
                if as_fields.contains(&format!("<insert {name}>")) {
                    continue;
                }
                if !values.get(&name).is_some_and(|v| !v.is_empty()) {
                    missing.insert(name);
                }
            }
        }
        for name in missing {
            let mut x = finding("warning", "placeholder", file, format!("<Insert {name}> has no project value"));
            // Where it is in the body, if it is; it may be in a header or footer.
            let needle = format!("<insert {name}>");
            x["para"] = json!(entries.iter().find(|e| entry_text(&doc, e).to_lowercase().contains(&needle)).map(|e| e.id.clone()));
            out.push(x);
        }
        // A newer master. The copy's own record is the current one (a master update rewrites it).
        let own = doc.custom_prop(MASTER_PROP).and_then(|m| serde_json::from_str::<MasterRef>(m).ok());
        if let Some(m) = own.as_ref().or(s.master.as_ref())
            && let Ok(bytes) = crate::io::read_document(Path::new(&m.path))
            && fingerprint(&bytes) != m.fingerprint
        {
            out.push(finding(
                "notice",
                "masterChanged",
                file,
                format!("the master {} has changed since this copy was made: Master Updates shows what changed", m.path),
            ));
        }
    }
    check_project_rules(&f, &values, &have, &printing_by_number, &mut out);
    Ok(report(&f, out))
}

/// A finding's key, to acknowledge it by: what it is and where, not its wording's details.
fn finding_key(x: &Value) -> String {
    let s = |k: &str| x.get(k).and_then(Value::as_str).unwrap_or("");
    format!("{}|{}|{}|{}|{}", s("severity"), s("code"), s("file"), s("para"), fingerprint(s("message").as_bytes()))
}

/// The findings with their keys and acknowledgements, and the counts of those not acknowledged.
fn report(f: &ProjectFile, mut out: Vec<Value>) -> Value {
    let acks: BTreeMap<&str, &str> = f.acknowledged.iter().map(|a| (a.key.as_str(), a.note.as_str())).collect();
    for x in &mut out {
        let key = finding_key(x);
        if x.get("para").is_none() {
            x["para"] = Value::Null;
        }
        if let Some(note) = acks.get(key.as_str()).filter(|_| x["severity"] != "error") {
            x["acknowledged"] = json!(true);
            x["note"] = json!(note);
        }
        x["key"] = json!(key);
    }
    let count = |sev: &str| out.iter().filter(|x| x["severity"] == sev && x.get("acknowledged").is_none()).count();
    let acked = out.iter().filter(|x| x.get("acknowledged").is_some()).count();
    json!({"errors": count("error"), "warnings": count("warning"), "notices": count("notice"), "acknowledged": acked, "findings": out})
}

fn acknowledge(_: &mut Session, v: &Value) -> CmdResult {
    let path = project_path(v)?;
    let mut f = load(&path)?;
    let key = p::req_str(v, "key")?.trim().to_string();
    if key.is_empty() || key.chars().count() > 600 || key.split('|').count() != 5 {
        return Err(CmdError::Params("`key` must be a finding's key from spec.project.validate".into()));
    }
    if key.starts_with("error|") {
        return Err(CmdError::Failed("errors can't be acknowledged: fix them, or publish with force".into()));
    }
    f.acknowledged.retain(|a| a.key != key);
    let undo = p::bool(v, "undo").unwrap_or(false);
    if !undo {
        if f.acknowledged.len() >= 10_000 {
            return Err(CmdError::Failed("too many acknowledged findings".into()));
        }
        let note = p::str(v, "note").unwrap_or("").trim().chars().take(500).collect();
        f.acknowledged.push(Acknowledged { key: key.clone(), note, extra: Map::new() });
    }
    store(&path, &f)?;
    Ok(json!({"key": key, "acknowledged": !undo, "count": f.acknowledged.len()}))
}

fn publish(s: &mut Session, v: &Value) -> CmdResult {
    let path = project_path(v)?;
    // Check first (M10): errors stop publishing unless the author says otherwise.
    let checked = validate(s, &json!({"path": path.to_string_lossy()}))?;
    let errors = checked.get("errors").and_then(Value::as_u64).unwrap_or(0);
    if errors > 0 && !p::bool(v, "force").unwrap_or(false) {
        return Err(CmdError::Failed(format!("Check Project found {errors} errors: fix them first, or publish with force")));
    }
    let formats: &[&str] = match p::str(v, "format").unwrap_or("pdf") {
        "pdf" => &["pdf"],
        "docx" => &["docx"],
        "both" => &["pdf", "docx"],
        other => return Err(CmdError::Params(format!("`format` must be \"pdf\", \"docx\" or \"both\", not {other:?}"))),
    };
    let form = super::forms::Form::from_params(v)?;
    let f = load(&path)?;
    let dir = folder(&path);
    let out_dir = match p::str(v, "folder") {
        Some(d) => PathBuf::from(d),
        None => dir.join("Published"),
    };
    if out_dir == dir {
        return Err(CmdError::Params("publish to a folder other than the project folder, so outputs and section files stay apart".into()));
    }
    crate::io::create_dir_all(&out_dir).map_err(CmdError::Failed)?;
    let mut done = Vec::new();
    for s in f.sections.iter().filter(|s| s.included) {
        let full = dir.join(&s.file);
        let opened = crate::io::read_document(&full).and_then(|b| crate::io::open_bytes(&s.file, &b)).map_err(CmdError::Failed);
        let mut row = json!({"file": s.file});
        match opened {
            Ok(doc) => {
                let mut sess = Session::new(doc);
                sess.path = Some(full.clone());
                for ext in formats {
                    // Word files go out under the section file's own name, in the Published folder.
                    let stem = Path::new(&s.file).file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let out = out_dir.join(format!("{stem}{}.{ext}", form.suffix()));
                    let cmd = if *ext == "pdf" { "spec.exportPdf" } else { "spec.exportDocx" };
                    let args =
                        json!({"path": out.to_string_lossy(), "project": path.to_string_lossy(), "form": form.name(), "depth": v.get("depth")});
                    match sess.run(cmd, &args) {
                        Ok(x) => {
                            row[*ext] = json!(out.to_string_lossy());
                            row["pages"] = x["pages"].clone();
                            row["warnings"] = x["warnings"].clone();
                        }
                        Err(e) => row["error"] = json!(e.to_string()),
                    }
                }
            }
            Err(e) => row["error"] = json!(e.to_string()),
        }
        done.push(row);
    }
    let failed = done.iter().filter(|d| d.get("error").is_some()).count();
    let check = json!({"errors": errors, "warnings": checked["warnings"], "notices": checked["notices"]});
    Ok(json!({"folder": out_dir.to_string_lossy(), "sections": done, "failed": failed, "check": check}))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use serde_json::{Value, json};
    use wordcraft_doc::PartKind;
    use wordcraft_doc::{Block, Document, Paragraph};

    use super::*;
    use crate::Session;

    fn para(style: &str, text: &str) -> Arc<Block> {
        let p = Paragraph::with_text(text, Default::default());
        Arc::new(Block::Para(if style.is_empty() { p } else { p.styled(style) }))
    }

    /// A small master: title, a Part, an Article, two paragraphs (one naming another section),
    /// end of section, and a header and footer with placeholders and a version stamp.
    fn master(number: &str, title: &str) -> Document {
        let mut d = Document::new();
        d.body = vec![
            para("SCT", &format!("SECTION {number} - {title}")),
            para("PRT", "GENERAL"),
            para("ART", "SUMMARY"),
            para("PR1", "Section includes the work of this Section."),
            para("PR1", "Related requirements: Section 07 9200 \"Joint Sealants\"."),
            para("EOS", &format!("END OF SECTION {number}")),
        ];
        let header = d.add_part(PartKind::Header, vec![para("", "<Insert Project Name>\t<Insert Project Number>")]);
        let footer = d.add_part(PartKind::Footer, vec![para("", "Test Master: 11.2026\tPage")]);
        d.last_section.headers.default = Some(header);
        d.last_section.footers.default = Some(footer);
        d
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("speccraft-project-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("masters")).unwrap();
        std::fs::create_dir_all(d.join("project")).unwrap();
        d
    }

    fn save(doc: &Document, path: &Path) {
        crate::io::save_path(path, doc).unwrap();
    }

    fn run(id: &str, v: Value) -> Value {
        let mut s = Session::new(Document::new());
        s.run(id, &v).unwrap_or_else(|e| panic!("{id}: {e}"))
    }

    fn fails(id: &str, v: Value) -> String {
        let mut s = Session::new(Document::new());
        s.run(id, &v).expect_err(id).to_string()
    }

    #[test]
    fn titles_split_into_number_and_title() {
        assert_eq!(split_title("SECTION 09 2900 - GYPSUM BOARD"), ("09 2900".into(), "GYPSUM BOARD".into()));
        assert_eq!(split_title("Section 092900 – Gypsum Board"), ("092900".into(), "Gypsum Board".into()));
        assert_eq!(split_title("SECTION 09 29 00 - GYPSUM BOARD"), ("09 29 00".into(), "GYPSUM BOARD".into()));
        assert_eq!(split_title("GYPSUM BOARD"), (String::new(), "GYPSUM BOARD".into()));
        assert_eq!(number_key("09 29 00"), number_key("092900"));
        assert_eq!(number_key("12"), "");
    }

    #[test]
    fn placeholders_are_found_and_filled_in_every_story() {
        let mut d = master("09 2900", "GYPSUM BOARD");
        d.body.push(para("PR1", "Owner: [Owner Name]; finish [Level 4] [Level 5]; <Insert colour>."));
        let f = ProjectFile {
            project: Info { name: "Riverside Library".into(), number: "2026-14".into(), ..Default::default() },
            values: [("Owner Name".to_string(), "City of Riverside".to_string())].into(),
            ..Default::default()
        };
        let (filled, missing) = fill_placeholders(&mut d, &placeholder_values(&f));
        assert_eq!(filled, 3);
        assert_eq!(missing.into_iter().collect::<Vec<_>>(), ["colour"]);
        let header = all_paragraphs(&d).iter().map(|p| p.text.clone()).find(|t| t.contains("2026-14")).unwrap();
        assert_eq!(header, "Riverside Library\t2026-14");
        let last = d.body.last().unwrap().as_para().unwrap().text.clone();
        assert_eq!(last, "Owner: City of Riverside; finish [Level 4] [Level 5]; <Insert colour>.", "choices are left alone");
    }

    #[test]
    fn fill_ins_the_project_fills_are_not_open() {
        let f = ProjectFile {
            project: Info { name: "Riverside Library".into(), ..Default::default() },
            values: [("Owner Name".to_string(), "City".to_string()), ("Empty".to_string(), String::new())].into(),
            ..Default::default()
        };
        let v = placeholder_values(&f);
        assert!(filled_by_project("<Insert Project Name>", &v));
        assert!(filled_by_project("[Owner Name]", &v));
        assert!(!filled_by_project("<Insert colour>", &v));
        assert!(!filled_by_project("<Insert Empty>", &v));
        assert!(!filled_by_project("[1/2 inch] [5/8 inch]", &v));
    }

    #[test]
    fn a_project_copies_masters_and_never_writes_them() {
        let dir = scratch("copy");
        let m = dir.join("masters").join("092900 Gypsum Board.docx");
        save(&master("09 2900", "GYPSUM BOARD"), &m);
        let before = std::fs::read(&m).unwrap();
        let project = dir.join("project").join("Riverside.speccraft");
        let path = project.to_string_lossy().to_string();

        run("spec.project.new", json!({"path": path, "name": "Riverside Library", "number": "2026-14"}));
        assert!(fails("spec.project.new", json!({"path": path, "name": "Again"})).contains("already exists"));
        let out = run("spec.project.addSection", json!({"path": path, "master": m.to_string_lossy()}));
        assert_eq!(out["added"], "092900 Gypsum Board.docx");
        assert_eq!(out["sections"][0]["number"], "09 2900");
        assert_eq!(out["sections"][0]["title"], "GYPSUM BOARD");
        assert_eq!(out["sections"][0]["master"]["version"], "Test Master: 11.2026");
        assert_eq!(std::fs::read(&m).unwrap(), before, "the master is untouched");

        // The copy is imported and knows where it came from.
        let copy = crate::io::open_path(&dir.join("project").join("092900 Gypsum Board.docx")).unwrap();
        assert_eq!(copy.custom_prop(ROLE_PROP), Some("project"));
        assert!(copy.custom_prop(MASTER_PROP).unwrap().contains("Test Master: 11.2026"));
        let baseline = copy.custom_prop(BASELINE_PROP).unwrap();
        assert_eq!(baseline.split(',').count(), 6);
        assert!(baseline.starts_with("p0001:"));
        assert_eq!(sections::outline(&copy).len(), 6);

        // Adding the same master again needs another name; a copy is not a master.
        assert!(fails("spec.project.addSection", json!({"path": path, "master": m.to_string_lossy()})).contains("already exists"));
        let copy_path = dir.join("project").join("092900 Gypsum Board.docx");
        assert!(
            fails("spec.project.addSection", json!({"path": path, "master": copy_path.to_string_lossy(), "file": "other.docx"}))
                .contains("already a project copy")
        );
        // A second copy of the same section, under another name, is refused and nothing is written.
        assert!(
            fails("spec.project.addSection", json!({"path": path, "master": m.to_string_lossy(), "file": "again.docx"}))
                .contains("already has Section 09 2900")
        );
        assert!(!dir.join("project").join("again.docx").exists());
        // The project remembers its library folder.
        let lib = dir.join("masters").to_string_lossy().to_string();
        assert_eq!(run("spec.project.set", json!({"path": path, "library": lib}))["library"], lib.as_str());
        assert_eq!(run("spec.project.set", json!({"path": path, "library": null}))["library"], Value::Null);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sections_are_ordered_included_and_checked() {
        let dir = scratch("check");
        let project = dir.join("project").join("Riverside.speccraft");
        let path = project.to_string_lossy().to_string();
        run("spec.project.new", json!({"path": path, "name": "Riverside Library"}));
        for (n, t, file) in [("09 2900", "GYPSUM BOARD", "gypsum.docx"), ("08 7100", "DOOR HARDWARE", "hardware.docx")] {
            let m = dir.join("masters").join(file);
            save(&master(n, t), &m);
            run("spec.project.addSection", json!({"path": path, "master": m.to_string_lossy()}));
        }
        let info = run("spec.project.info", json!({"path": path}));
        assert_eq!(info["sections"][0]["file"], "hardware.docx", "kept in section-number order");

        // Placeholders without values, a reference to 07 9200, and its requirement.
        run("spec.project.requires", json!({"path": path, "file": "gypsum.docx", "requires": ["07 9200"]}));
        let r = run("spec.project.validate", json!({"path": path}));
        let codes: Vec<&str> = r["findings"].as_array().unwrap().iter().map(|f| f["code"].as_str().unwrap()).collect();
        assert!(codes.contains(&"requiredMissing"), "{codes:?}");
        assert!(codes.contains(&"placeholder"), "{codes:?}");
        assert_eq!(r["errors"], 1);

        // Values fill the placeholders; the requirement is dropped; the reference is in text
        // that doesn't print until switched on, so it isn't reported.
        run("spec.project.set", json!({"path": path, "number": "2026-14", "values": {"Architect Name": "Studio North"}}));
        run("spec.project.requires", json!({"path": path, "file": "gypsum.docx", "requires": []}));
        let r = run("spec.project.validate", json!({"path": path}));
        // Nothing is chosen in either copy yet: one warning each, which the author can accept.
        assert_eq!((r["errors"].clone(), r["warnings"].clone()), (json!(0), json!(2)), "{r}");
        for x in r["findings"].as_array().unwrap() {
            assert_eq!(x["code"], "nothingPrints", "{x}");
            run("spec.project.acknowledge", json!({"path": path, "key": x["key"], "note": "choosing later"}));
        }
        let r = run("spec.project.validate", json!({"path": path}));
        assert_eq!((r["warnings"].clone(), r["acknowledged"].clone()), (json!(0), json!(2)), "{r}");
        assert_eq!(r["findings"][0]["note"], "choosing later");
        let key = r["findings"][0]["key"].clone();
        run("spec.project.acknowledge", json!({"path": path, "key": key, "undo": true}));
        assert_eq!(run("spec.project.validate", json!({"path": path}))["warnings"], 1);
        assert!(fails("spec.project.acknowledge", json!({"path": path, "key": "error|missingFile|a.docx||0"})).contains("can't be acknowledged"));

        // Switch everything on in one copy: now the reference to 07 9200 prints.
        let g = dir.join("project").join("gypsum.docx");
        let mut s = Session::new(crate::io::open_path(&g).unwrap());
        let ids: Vec<String> = sections::outline(&s.doc).iter().map(|e| e.id.clone()).collect();
        s.run("spec.activate", &json!({"ids": ids})).unwrap();
        crate::io::save_path(&g, &s.doc).unwrap();
        let r = run("spec.project.validate", json!({"path": path}));
        assert!(r["findings"].as_array().unwrap().iter().any(|f| f["code"] == "referenceMissing"), "{r}");

        // A file that disappears, and a duplicate number.
        run("spec.project.include", json!({"path": path, "file": "hardware.docx", "included": false}));
        std::fs::remove_file(dir.join("project").join("hardware.docx")).unwrap();
        let r = run("spec.project.validate", json!({"path": path}));
        assert!(!r["findings"].as_array().unwrap().iter().any(|f| f["code"] == "missingFile"), "excluded sections aren't checked");
        run("spec.project.include", json!({"path": path, "file": "hardware.docx", "included": true}));
        let r = run("spec.project.validate", json!({"path": path}));
        assert!(r["findings"].as_array().unwrap().iter().any(|f| f["code"] == "missingFile"), "{r}");

        // Order by hand, then remove (the file stays).
        run("spec.project.order", json!({"path": path, "files": ["gypsum.docx", "hardware.docx"]}));
        assert!(fails("spec.project.order", json!({"path": path, "files": ["gypsum.docx"]})).contains("exactly once"));
        let out = run("spec.project.removeSection", json!({"path": path, "file": "gypsum.docx"}));
        assert_eq!(out["sections"].as_array().unwrap().len(), 1);
        assert!(g.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn publishing_fills_placeholders_from_the_project() {
        let dir = scratch("publish");
        let project = dir.join("project").join("Riverside.speccraft");
        let path = project.to_string_lossy().to_string();
        run("spec.project.new", json!({"path": path, "name": "Riverside Library", "number": "2026-14"}));
        let m = dir.join("masters").join("gypsum.docx");
        save(&master("09 2900", "GYPSUM BOARD"), &m);
        run("spec.project.addSection", json!({"path": path, "master": m.to_string_lossy()}));
        // An error stops publishing until it is fixed or the author forces it.
        run("spec.project.requires", json!({"path": path, "file": "gypsum.docx", "requires": ["07 9200"]}));
        assert!(fails("spec.project.publish", json!({"path": path})).contains("1 errors"));
        assert_eq!(run("spec.project.publish", json!({"path": path, "force": true}))["check"]["errors"], 1);
        run("spec.project.requires", json!({"path": path, "file": "gypsum.docx", "requires": []}));
        let out = run("spec.project.publish", json!({"path": path}));
        assert_eq!(out["failed"], 0, "{out}");
        assert_eq!(out["check"]["errors"], 0);
        let pdf = dir.join("project").join("Published").join("gypsum.pdf");
        assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF"));
        // Word too (M13): the same name, a plain document.
        let out = run("spec.project.publish", json!({"path": path, "format": "both"}));
        let docx = dir.join("project").join("Published").join("gypsum.docx");
        assert_eq!(out["sections"][0]["docx"], docx.to_string_lossy().as_ref(), "{out}");
        let back = crate::io::open_bytes("x.docx", &std::fs::read(&docx).unwrap()).unwrap();
        assert!(back.custom_prop(ROLE_PROP).is_none());
        assert!(fails("spec.project.publish", json!({"path": path, "format": "rtf"})).contains("format"));

        // The section's own spec.exportPdf finds the project beside it; the file keeps its placeholders.
        let g = dir.join("project").join("gypsum.docx");
        let mut s = Session::new(crate::io::open_path(&g).unwrap());
        s.path = Some(g.clone());
        let r = s.run("spec.exportPdf", &json!({"path": dir.join("out.pdf").to_string_lossy()})).unwrap();
        assert_eq!(r["placeholdersFilled"], 2, "{r}");
        assert!(all_paragraphs(&s.doc).iter().any(|p| p.text.contains("<Insert Project Name>")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rules_between_sections_are_checked() {
        let dir = scratch("rules");
        let project = dir.join("project").join("Riverside.speccraft");
        let path = project.to_string_lossy().to_string();
        run("spec.project.new", json!({"path": path, "name": "Riverside Library"}));
        let m = dir.join("masters").join("gypsum.docx");
        save(&master("09 2900", "GYPSUM BOARD"), &m);
        run("spec.project.addSection", json!({"path": path, "master": m.to_string_lossy()}));
        // Gypsum board's GENERAL Part (p0002) needs joint sealants; LEED needs a recycled-content paragraph.
        run(
            "spec.project.addRule",
            json!({"path": path, "kind": "require", "if": [{"section": "09 2900", "on": "p0002"}], "target": {"section": "07 9200"}, "note": "sealant at board joints"}),
        );
        run(
            "spec.project.addRule",
            json!({"path": path, "kind": "require", "if": [{"value": "LEED", "is": "yes"}], "target": {"section": "09 2900", "on": "p0004"}}),
        );
        run("spec.project.addRule", json!({"path": path, "kind": "exclude", "if": [{"section": "09 2900"}], "target": {"section": "09 2116"}}));
        assert!(
            fails(
                "spec.project.addRule",
                json!({"path": path, "kind": "require", "if": [{"value": "LEED", "on": "p0001"}], "target": {"section": "07 9200"}})
            )
            .contains("each condition")
        );
        let rule_findings = |path: &str| -> Vec<String> {
            let r = run("spec.project.validate", json!({"path": path}));
            r["findings"].as_array().unwrap().iter().filter(|f| f["code"] == "rule").map(|f| f["message"].as_str().unwrap().to_string()).collect()
        };
        assert!(rule_findings(&path).is_empty(), "GENERAL is off and LEED isn't set: nothing applies");
        let g = dir.join("project").join("gypsum.docx");
        let mut s = Session::new(crate::io::open_path(&g).unwrap());
        s.run("spec.activate", &json!({"ids": ["p0002", "p0003", "p0004"]})).unwrap();
        s.run("spec.deactivate", &json!({"id": "p0004"})).unwrap();
        crate::io::save_path(&g, &s.doc).unwrap();
        run("spec.project.set", json!({"path": path, "values": {"LEED": "yes"}}));
        let found = rule_findings(&path);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].contains("R001: p0002 of Section 09 2900, so Section 07 9200 is required (sealant at board joints)"), "{found:?}");
        assert!(found[1].contains("R002: LEED = yes, so p0004 of Section 09 2900 is required"), "{found:?}");
        run("spec.project.removeRule", json!({"path": path, "id": "R001"}));
        assert_eq!(rule_findings(&path).len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bad_project_files_and_params_are_refused() {
        let dir = scratch("bad");
        let p = dir.join("project").join("x.speccraft");
        assert!(fails("spec.project.info", json!({"path": "notes.txt"})).contains(".speccraft"));
        std::fs::write(&p, b"{\"format\": \"something else\", \"version\": 1}").unwrap();
        assert!(fails("spec.project.info", json!({"path": p.to_string_lossy()})).contains("not a Speccraft project"));
        std::fs::write(&p, b"{\"format\": \"speccraft-project\", \"version\": 99}").unwrap();
        assert!(fails("spec.project.info", json!({"path": p.to_string_lossy()})).contains("newer Speccraft"));
        std::fs::write(&p, b"not json").unwrap();
        assert!(fails("spec.project.info", json!({"path": p.to_string_lossy()})).contains("not a Speccraft project"));

        std::fs::remove_file(&p).unwrap();
        let path = p.to_string_lossy().to_string();
        run("spec.project.new", json!({"path": path, "name": "X"}));
        assert!(fails("spec.project.addSection", json!({"path": path, "file": "../escape.docx"})).contains("no folders"));
        assert!(fails("spec.project.set", json!({"path": path, "values": {"<bad>": "x"}})).contains("without"));
        assert!(fails("spec.project.requires", json!({"path": path, "file": "a.docx", "requires": ["12"]})).contains("section number"));
        assert!(fails("spec.project.set", json!({"path": path, "name": ""})).contains("can't be empty"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unknown_fields_survive_a_rewrite() {
        let dir = scratch("extra");
        let p = dir.join("project").join("x.speccraft");
        std::fs::write(
            &p,
            br#"{"format": "speccraft-project", "version": 1, "project": {"name": "X", "client": "Kept"}, "future": [1, 2],
                "sections": [{"file": "a.docx", "number": "09 2900", "tags": ["kept"]}]}"#,
        )
        .unwrap();
        let path = p.to_string_lossy().to_string();
        run("spec.project.set", json!({"path": path, "issue": "Issued for Bid"}));
        let v: Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
        assert_eq!(
            (v["project"]["client"].clone(), v["future"].clone(), v["sections"][0]["tags"][0].clone()),
            (json!("Kept"), json!([1, 2]), json!("kept"))
        );
        assert_eq!(v["project"]["issue"], "Issued for Bid");
        assert_eq!(v["sections"][0]["included"], true);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
