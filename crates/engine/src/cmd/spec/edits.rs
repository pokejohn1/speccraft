//! Specs › Edits: the project editing layer (milestone M8). Added by the Speccraft fork; not part
//! of the original WordCraft.
//!
//! Every spec paragraph has an origin, as in SpecLink's origin column:
//!
//! | Origin | Meaning |
//! |---|---|
//! | `master` | the master's wording, unchanged |
//! | `modified` | the master's paragraph, reworded in this project (SpecLink's M\*) |
//! | `added` | written in this project (SpecLink's U) |
//!
//! and master paragraphs deleted from the project are listed as `removed`. Fill-ins don't count:
//! a paragraph is compared with each fill-in put back to the master's wording.
//!
//! The master's wording of each paragraph (its style and text) is recorded once, when the section
//! is first imported, in the document property `Speccraft.Original`, so it travels with the file.
//! A project copy made before that property existed reads it back from its master (the path in
//! `Speccraft.Master`), whose import gives the same paragraph ids.
//!
//! Restoring puts the master's wording back (or the deleted paragraph, switched off). Master
//! paragraphs are never deleted by these commands: switch them off instead. Paragraphs added
//! here can be duplicated, added and removed freely.

use std::collections::{BTreeMap, HashMap};
use std::path::Path as FsPath;
use std::sync::Arc;

use serde_json::{Value, json};
use wordcraft_doc::para::OBJ;
use wordcraft_doc::{Block, ControlKind, Document, Paragraph, Path, Pos, StoryRef};

use super::fields;
use super::sections::{self, Entry, Kind};
use crate::{CmdError, CmdResult, CommandSpec, Selection, Session, p};

const LOC: &str = "Specs › Edits";
/// Document property: the master's wording of each spec paragraph, JSON `{"p0001": ["SCT", "SECTION …"], …}`.
pub const ORIGINALS_PROP: &str = "Speccraft.Original";
/// Longest paragraph text `spec.addParagraph` takes, in characters.
const MAX_TEXT: usize = 20_000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("spec.origin", "Paragraph Origins", LOC, origin_cmd).pure(),
        CommandSpec::new("spec.restore", "Restore Master Wording", LOC, restore_cmd)
            .params(r#"{"id"?: string (default: the paragraph at the caret; a deleted master paragraph's id puts it back)}"#),
        CommandSpec::new("spec.duplicate", "Duplicate Paragraph", LOC, duplicate_cmd)
            .params(r#"{"id"?: string (default: the paragraph at the caret)}"#),
        CommandSpec::new("spec.addParagraph", "Add Paragraph", LOC, add_cmd)
            .params(r#"{"after"?: string (default: the paragraph at the caret), "text"?: string, "child"?: bool (one level down)}"#),
        CommandSpec::new("spec.removeParagraph", "Delete Added Paragraph(s)", LOC, remove_cmd)
            .params(r#"{"id"?: string, "ids"?: [string] (default: the added paragraphs the selection touches, or the paragraph at the caret)}"#),
    ]
}

/// Where a paragraph's wording comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Master,
    Modified,
    Added,
}

impl Origin {
    pub fn name(self) -> &'static str {
        match self {
            Origin::Master => "master",
            Origin::Modified => "modified",
            Origin::Added => "added",
        }
    }
}

/// The master's wording of a paragraph: style id and text.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Original(pub String, pub String);

/// The originals property, read.
fn stored(doc: &Document) -> Option<BTreeMap<String, Original>> {
    doc.custom_prop(ORIGINALS_PROP).and_then(|v| serde_json::from_str(v).ok())
}

/// The originals: the property, else read back from the master file.
pub fn originals(doc: &Document) -> BTreeMap<String, Original> {
    if let Some(o) = stored(doc) {
        return o;
    }
    from_master(doc).unwrap_or_default()
}

/// The last master read by [`from_master`]: its path, the fingerprint of its bytes, its originals.
type MasterCache = Option<(String, String, BTreeMap<String, Original>)>;
static MASTER_CACHE: std::sync::Mutex<MasterCache> = std::sync::Mutex::new(None);

/// A project copy's originals from its master: imported in memory, so the ids match. The result
/// is kept while the master file is unchanged, so the page can ask on every edit.
fn from_master(doc: &Document) -> Option<BTreeMap<String, Original>> {
    let m: Value = serde_json::from_str(doc.custom_prop(super::project::MASTER_PROP)?).ok()?;
    let path = m.get("path")?.as_str()?.to_string();
    let bytes = crate::io::read_document(FsPath::new(&path)).ok()?;
    let fp = super::project::fingerprint(&bytes);
    // A master changed since the copy was made no longer has the copy's original wording (M11).
    if m.get("fingerprint").and_then(Value::as_str).is_some_and(|f| f != fp) {
        return None;
    }
    if let Ok(cache) = MASTER_CACHE.lock()
        && let Some((p, f, map)) = cache.as_ref()
        && *p == path
        && *f == fp
    {
        return Some(map.clone());
    }
    let master = crate::io::open_bytes(&path, &bytes).ok()?;
    let mut s = Session::new(master);
    s.run("spec.import", &json!({})).ok()?;
    let map = stored(&s.doc)?;
    if let Ok(mut cache) = MASTER_CACHE.lock() {
        *cache = Some((path, fp, map.clone()));
    }
    Some(map)
}

/// A paragraph's text as the comparison sees it: fill-ins put back to the master's wording,
/// objects left out, whitespace runs as one space.
pub fn base_text(doc: &Document, e: &Entry) -> String {
    base_text_with(doc, e, &super::terms::originals_list(doc))
}

/// [`base_text`] with the document's fields already listed.
fn base_text_with(doc: &Document, e: &Entry, all: &[(wordcraft_doc::control::ControlRange, String)]) -> String {
    let Some(para) = doc.para_at(&e.range.start) else { return String::new() };
    let block = e.range.start.path.0.first().copied();
    let (from, to) = (e.range.content_start().off, e.range.end.off);
    let mut fs: Vec<&(wordcraft_doc::control::ControlRange, String)> = all.iter().filter(|(r, _)| r.start.path.0.first().copied() == block).collect();
    fs.sort_by_key(|(r, _)| r.start.off);
    let mut out = String::new();
    let mut at = from;
    for (r, original) in fs.iter().filter(|(r, _)| r.start.off >= from && r.end.off <= to) {
        // Nested controls (a term inside a choice) are covered by the outer one.
        if r.start.off < at {
            continue;
        }
        out.push_str(para.text.get(at..r.start.off).unwrap_or(""));
        out.push_str(original);
        at = r.end.off + OBJ.len_utf8();
    }
    out.push_str(para.text.get(at.min(to)..to).unwrap_or(""));
    normal(&out)
}

/// Text compared: objects out, whitespace runs as one space, trimmed.
fn normal(t: &str) -> String {
    t.replace(OBJ, "").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Record the master's wording of every spec paragraph, unless it already is. Run by
/// `spec.import`, so the first import of a master (or of a project copy) sets it.
pub fn record_originals(doc: &mut Document) {
    if doc.custom_prop(ORIGINALS_PROP).is_some() {
        return;
    }
    let entries = sections::outline(doc);
    if entries.is_empty() {
        return;
    }
    let all = super::terms::originals_list(doc);
    let map: BTreeMap<String, Original> = entries
        .iter()
        .map(|e| {
            let style = doc.para_at(&e.range.start).and_then(|p| p.props.style.clone()).unwrap_or_default();
            (e.id.clone(), Original(style, base_text_with(doc, e, &all)))
        })
        .collect();
    if let Ok(v) = serde_json::to_string(&map) {
        doc.set_custom_prop(ORIGINALS_PROP, &v);
    }
}

/// The fingerprints of `Speccraft.Baseline`, by id.
pub(crate) fn baseline(doc: &Document) -> HashMap<String, String> {
    doc.custom_prop(super::project::BASELINE_PROP)
        .unwrap_or("")
        .split(',')
        .filter_map(|x| x.split_once(':'))
        .map(|(a, b)| (a.trim().to_string(), b.trim().to_string()))
        .collect()
}

/// Each spec paragraph's origin, and the master paragraphs no longer in the document.
pub fn origins(doc: &Document) -> (Vec<(Entry, Origin)>, Vec<(String, Original)>) {
    let orig = originals(doc);
    let base = baseline(doc);
    let entries = sections::outline(doc);
    let all = super::terms::originals_list(doc);
    let present: std::collections::HashSet<&str> = entries.iter().map(|e| e.id.as_str()).collect();
    let mut out = Vec::new();
    for e in &entries {
        let now = base_text_with(doc, e, &all);
        let origin = match (orig.get(&e.id), base.get(&e.id)) {
            (Some(o), _) => {
                if normal(&o.1) == now {
                    Origin::Master
                } else {
                    Origin::Modified
                }
            }
            // A copy whose wording can't be found: the fingerprint still tells.
            (None, Some(fp)) => {
                let raw = doc.para_at(&e.range.start).map(|p| p.plain_text().replace(OBJ, "")).unwrap_or_default();
                let fps = [super::project::fingerprint(raw.as_bytes()), super::project::fingerprint(now.as_bytes())];
                if fps.contains(fp) { Origin::Master } else { Origin::Modified }
            }
            (None, None) if orig.is_empty() && base.is_empty() => Origin::Master,
            (None, None) => Origin::Added,
        };
        out.push((e.clone(), origin));
    }
    let removed = orig.into_iter().filter(|(id, _)| !present.contains(id.as_str())).collect();
    (out, removed)
}

// ---------------------------------------------------------------------------------------------
// Commands

/// The paragraph `id` names, else the one at the caret.
fn target(s: &Session, v: &Value, key: &str) -> Result<(usize, Vec<Entry>), CmdError> {
    let entries = sections::outline(&s.doc);
    let i = match p::str(v, key) {
        Some(id) => entries.iter().position(|e| e.id == id.trim()).ok_or_else(|| CmdError::Failed(format!("no spec paragraph called {id:?}")))?,
        None => sections::at_caret(s, &entries).ok_or_else(|| CmdError::Failed("the caret is not in a spec paragraph".into()))?,
    };
    Ok((i, entries))
}

fn origin_cmd(s: &mut Session, _v: &Value) -> CmdResult {
    let (items, removed) = origins(&s.doc);
    let orig = originals(&s.doc);
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let rows: Vec<Value> = items
        .iter()
        .map(|(e, o)| {
            *counts.entry(o.name()).or_default() += 1;
            let mut row = json!({"id": e.id, "origin": o.name()});
            if *o == Origin::Modified
                && let Some(m) = orig.get(&e.id)
            {
                row["original"] = json!(m.1);
            }
            row
        })
        .collect();
    let removed: Vec<Value> = removed.iter().map(|(id, o)| json!({"id": id, "style": o.0, "text": o.1})).collect();
    Ok(json!({"items": rows, "removed": removed, "counts": counts, "recorded": s.doc.custom_prop(ORIGINALS_PROP).is_some()}))
}

/// Replace a spec paragraph's content (between its markers) with `text`, in the formatting of its first character.
pub(crate) fn set_paragraph_text(doc: &mut Document, e: &Entry, text: &str) -> Result<(), CmdError> {
    let a = e.range.content_start();
    let props = doc.para_at(&a).map(|p| p.props_of_char(a.off).clone()).unwrap_or_default();
    if e.range.end.off > a.off {
        doc.delete_range(&a, &e.range.end).map_err(|x| CmdError::Failed(x.to_string()))?;
    }
    doc.insert_text(&a, text, &props).map_err(|x| CmdError::Failed(x.to_string()))?;
    Ok(())
}

/// Insert a new paragraph at body index `at` and make it spec paragraph `id`.
pub(crate) fn insert_item(s: &mut Session, at: usize, style: &str, text: &str, id: &str, props: wordcraft_doc::CharProps) -> Result<(), CmdError> {
    // An empty paragraph gets a space to wrap (a control around nothing would be inline), taken
    // out again below.
    let empty = text.is_empty();
    let mut para = Paragraph::with_text(if empty { " " } else { text }, props);
    if !style.is_empty() {
        para = para.styled(style);
    }
    let at = at.min(s.doc.body.len());
    s.doc.body.insert(at, Arc::new(Block::Para(para)));
    let len = s.doc.body.get(at).and_then(|b| b.as_para()).map_or(0, Paragraph::len);
    let path = Path::top(at);
    s.sel = Selection { anchor: Pos::new(StoryRef::Body, path.clone(), 0), focus: Pos::new(StoryRef::Body, path.clone(), len) };
    super::super::controls::insert(s, &json!({"tag": format!("{}{id}", super::TAG_PREFIX)}), ControlKind::RichText)?;
    if empty {
        let a = Pos::new(StoryRef::Body, path.clone(), OBJ.len_utf8());
        let b = Pos::new(StoryRef::Body, path.clone(), OBJ.len_utf8() + 1);
        s.doc.delete_range(&a, &b).map_err(|e| CmdError::Failed(e.to_string()))?;
    }
    s.sel = Selection::caret(Pos::new(StoryRef::Body, path, OBJ.len_utf8()));
    Ok(())
}

/// A fresh id for a paragraph written in the project: `u0001`, `u0002`, …
pub(crate) fn new_user_id(entries: &[Entry]) -> String {
    let top = entries.iter().filter_map(|e| e.id.strip_prefix('u').and_then(|n| n.parse::<u32>().ok())).max().unwrap_or(0);
    format!("u{:04}", top.saturating_add(1))
}

/// The body index after paragraph `i` and everything under it.
pub(crate) fn after_branch(entries: &[Entry], i: usize) -> usize {
    let mut last = i;
    let mut under = vec![i];
    for (k, e) in entries.iter().enumerate().skip(i + 1) {
        if e.parent.is_some_and(|p| under.contains(&p)) {
            under.push(k);
            last = k;
        } else {
            break;
        }
    }
    entries.get(last).and_then(|e| e.range.start.path.0.first()).map_or(0, |b| *b as usize + 1)
}

/// Switch paragraph `id` on if its parent prints (otherwise it stays off).
fn switch_on_if_possible(s: &mut Session, id: &str) -> bool {
    sections::activate(s, &json!({"id": id})).is_ok()
}

fn restore_cmd(s: &mut Session, v: &Value) -> CmdResult {
    let orig = originals(&s.doc);
    // A deleted master paragraph: put it back after the nearest earlier master paragraph still here.
    if let Some(id) = p::str(v, "id").map(str::trim)
        && !sections::outline(&s.doc).iter().any(|e| e.id == id)
    {
        let o = orig.get(id).cloned().ok_or_else(|| CmdError::Failed(format!("no spec paragraph or deleted master paragraph called {id:?}")))?;
        let entries = sections::outline(&s.doc);
        // In the master's order (M11), which new master paragraphs' ids don't follow.
        let order: Vec<String> = super::update::master_order(&s.doc).into_iter().map(|(id, _)| id).collect();
        let pos = order.iter().position(|k| k.as_str() == id).unwrap_or(0);
        let before = order.get(..pos).unwrap_or(&[]).iter().rev().find_map(|k| entries.iter().find(|e| &e.id == k));
        let at = before
            .and_then(|e| e.range.start.path.0.first())
            .map_or_else(|| entries.first().and_then(|e| e.range.start.path.0.first()).map_or(0, |b| *b as usize), |b| *b as usize + 1);
        insert_item(s, at, &o.0, &o.1, id, Default::default())?;
        if s.doc.custom_prop(fields::ORIGINALS_PROP).is_some() {
            fields::detect(&mut s.doc)?;
        }
        return Ok(json!({"id": id, "restored": "paragraph", "on": false}));
    }
    let (i, entries) = target(s, v, "id")?;
    let Some(e) = entries.get(i) else { return Err(CmdError::Failed("no such paragraph".into())) };
    let o = orig.get(&e.id).ok_or_else(|| CmdError::Failed(format!("{:?} was written in this project: it has no master wording", e.id)))?;
    if normal(&o.1) == base_text(&s.doc, e) {
        return Ok(json!({"id": e.id, "restored": "nothing", "unchanged": true}));
    }
    set_paragraph_text(&mut s.doc, e, &o.1)?;
    if s.doc.custom_prop(fields::ORIGINALS_PROP).is_some() {
        fields::detect(&mut s.doc)?;
    }
    Ok(json!({"id": e.id, "restored": "wording", "text": o.1}))
}

fn duplicate_cmd(s: &mut Session, v: &Value) -> CmdResult {
    let (i, entries) = target(s, v, "id")?;
    let Some(e) = entries.get(i) else { return Err(CmdError::Failed("no such paragraph".into())) };
    if matches!(e.kind, Kind::Title | Kind::End) {
        return Err(CmdError::Failed("a section has one title and one end".into()));
    }
    let para = s.doc.para_at(&e.range.start).cloned().ok_or_else(|| CmdError::Failed("no paragraph".into()))?;
    let a = e.range.content_start();
    let text = s.doc.control_text(&e.range).replace(OBJ, "");
    let props = para.props_of_char(a.off).clone();
    let id = new_user_id(&entries);
    let at = after_branch(&entries, i);
    let was_on = e.on;
    insert_item(s, at, para.props.style.as_deref().unwrap_or(""), &text, &id, props)?;
    let on = was_on && switch_on_if_possible(s, &id);
    Ok(json!({"id": id, "on": on}))
}

/// The style of the next level down: Part → Article → PR1 … PR5.
pub(crate) fn child_style(style: &str) -> Option<String> {
    let known = match style.to_ascii_uppercase().as_str() {
        "SCT" => Some("PRT"),
        "PRT" => Some("ART"),
        "ART" => Some("PR1"),
        "PR1" => Some("PR2"),
        "PR2" => Some("PR3"),
        "PR3" => Some("PR4"),
        "PR4" => Some("PR5"),
        _ => None,
    };
    if let Some(k) = known {
        return Some(k.to_string());
    }
    // A level-numbered style ("CSILevel3"): the same name, one level down.
    let n = sections::style_level(style)?;
    (n < 9).then(|| format!("{}{}", style.get(..style.len().saturating_sub(1)).unwrap_or(""), n + 1))
}

fn add_cmd(s: &mut Session, v: &Value) -> CmdResult {
    let (i, entries) = target(s, v, "after")?;
    let Some(e) = entries.get(i) else { return Err(CmdError::Failed("no such paragraph".into())) };
    let text = p::str(v, "text").unwrap_or("New paragraph.").to_string();
    if text.chars().count() > MAX_TEXT || text.contains(['\n', '\r', OBJ]) {
        return Err(CmdError::Params(format!("`text` must be one paragraph of at most {MAX_TEXT} characters")));
    }
    let para = s.doc.para_at(&e.range.start).cloned().ok_or_else(|| CmdError::Failed("no paragraph".into()))?;
    let style = para.props.style.clone().unwrap_or_default();
    let child = p::bool(v, "child") == Some(true);
    let (style, at) = if child {
        let st = child_style(&style).ok_or_else(|| CmdError::Failed(format!("a {style:?} paragraph has no level below it")))?;
        (st, e.range.start.path.0.first().map_or(0, |b| *b as usize + 1))
    } else if matches!(e.kind, Kind::Title) {
        let part = child_style(&style).unwrap_or_else(|| "PRT".to_string());
        (part, e.range.start.path.0.first().map_or(0, |b| *b as usize + 1))
    } else if matches!(e.kind, Kind::End) {
        return Err(CmdError::Failed("nothing comes after the end of section".into()));
    } else {
        (style, after_branch(&entries, i))
    };
    let props = para.props_of_char(e.range.content_start().off).clone();
    let id = new_user_id(&entries);
    insert_item(s, at, &style, &text, &id, props)?;
    // Switched on unless the author turned Auto-Select New off (M18).
    let on = super::paragraphs::auto_select(&s.doc) && switch_on_if_possible(s, &id);
    Ok(json!({"id": id, "style": style, "on": on}))
}

/// Delete added paragraphs (M18): `id`, `ids`, or with neither the paragraph at the caret, or
/// every added paragraph the selection touches when it spans several (master paragraphs in it
/// are kept: they are switched off, never deleted). A paragraph goes only with everything under it.
fn remove_cmd(s: &mut Session, v: &Value) -> CmdResult {
    let entries = sections::outline(&s.doc);
    let (items, _) = origins(&s.doc);
    let added = |i: usize| items.get(i).map(|(_, o)| *o) == Some(Origin::Added);
    let by_id = |id: &str| entries.iter().position(|e| e.id == id.trim()).ok_or_else(|| CmdError::Failed(format!("no spec paragraph called {id:?}")));
    let mut chosen: Vec<usize> = Vec::new();
    let mut kept = 0usize;
    let explicit = if let Some(list) = v.get("ids") {
        let list = list.as_array().ok_or_else(|| CmdError::Params("`ids` must be a list of strings".into()))?;
        if list.len() > super::MAX_ITEMS {
            return Err(CmdError::Params(format!("at most {} ids at once", super::MAX_ITEMS)));
        }
        for x in list {
            chosen.push(by_id(x.as_str().ok_or_else(|| CmdError::Params("`ids` must be a list of strings".into()))?)?);
        }
        true
    } else if let Some(id) = p::str(v, "id") {
        chosen.push(by_id(id)?);
        true
    } else {
        let (a, b) = (s.sel.anchor.clone(), s.sel.focus.clone());
        let spans = a.story == StoryRef::Body && b.story == StoryRef::Body && a.path.0.first() != b.path.0.first();
        if spans {
            let (x, y) = (a.path.0.first().copied().unwrap_or(0), b.path.0.first().copied().unwrap_or(0));
            let (lo, hi) = (x.min(y), x.max(y));
            for (i, e) in entries.iter().enumerate() {
                if e.range.start.path.0.first().is_some_and(|blk| (lo..=hi).contains(blk)) {
                    if added(i) {
                        chosen.push(i);
                    } else {
                        kept += 1;
                    }
                }
            }
            if chosen.is_empty() {
                return Err(CmdError::Failed("the selection has no added paragraphs: master paragraphs are switched off, never deleted".into()));
            }
            false
        } else {
            chosen.push(sections::at_caret(s, &entries).ok_or_else(|| CmdError::Failed("the caret is not in a spec paragraph".into()))?);
            true
        }
    };
    chosen.sort_unstable();
    chosen.dedup();
    let set: std::collections::HashSet<usize> = chosen.iter().copied().collect();
    let mut blocks: Vec<usize> = Vec::new();
    for &i in &chosen {
        let Some(e) = entries.get(i) else { return Err(CmdError::Failed("no such paragraph".into())) };
        if explicit && !added(i) {
            return Err(CmdError::Failed(format!("{:?} is a master paragraph: switch it off instead (master paragraphs are never deleted)", e.id)));
        }
        if entries.iter().enumerate().any(|(j, x)| x.parent == Some(i) && !set.contains(&j)) {
            return Err(CmdError::Failed(format!("{:?} has paragraphs under it: delete them too, or first", e.id)));
        }
        let b = e.range.start.path.0.first().map_or(usize::MAX, |b| *b as usize);
        if b >= s.doc.body.len() {
            return Err(CmdError::Failed("no paragraph".into()));
        }
        if s.doc.body.get(b).and_then(|x| x.as_para()).is_some_and(|p| p.section.is_some()) {
            return Err(CmdError::Failed("this paragraph ends a page layout section: it can't be removed".into()));
        }
        blocks.push(b);
    }
    blocks.sort_unstable();
    blocks.dedup();
    for b in blocks.iter().rev() {
        s.doc.body.remove(*b);
    }
    s.doc.ensure_nonempty();
    let first = blocks.first().copied().unwrap_or(0);
    let back = first.saturating_sub(1).min(s.doc.body.len().saturating_sub(1));
    s.sel = Selection::caret(Pos::new(StoryRef::Body, Path::top(back), 0));
    let ids: Vec<&str> = chosen.iter().filter_map(|i| entries.get(*i)).map(|e| e.id.as_str()).collect();
    let removed = if ids.len() == 1 { json!(ids.first()) } else { json!(null) };
    Ok(json!({"removed": removed, "ids": ids, "kept": kept}))
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

    /// p0001 title, p0002 Part, p0003 Article, p0004–p0005 paragraphs (one with a choice), p0006 end.
    fn section() -> Session {
        let mut d = Document::new();
        d.body = vec![
            para("SCT", "SECTION 09 2900 - GYPSUM BOARD"),
            para("PRT", "GENERAL"),
            para("ART", "SUMMARY"),
            para("PR1", "Thickness: [1/2 inch] [5/8 inch]."),
            para("PR1", "Deliver panels dry."),
            para("EOS", "END OF SECTION 09 2900"),
        ];
        let mut s = Session::new(d);
        run(&mut s, "spec.import", json!({}));
        run(&mut s, "spec.fields.detect", json!({}));
        s
    }

    fn origin_of(s: &mut Session, id: &str) -> String {
        let r = run(s, "spec.origin", json!({}));
        r["items"].as_array().unwrap().iter().find(|i| i["id"] == id).map(|i| i["origin"].as_str().unwrap().to_string()).unwrap_or_default()
    }

    fn text(s: &Session, block: usize) -> String {
        s.doc.body[block].as_para().unwrap().plain_text().replace(OBJ, "")
    }

    #[test]
    fn import_records_the_master_wording_and_fill_ins_dont_count_as_changes() {
        let mut s = section();
        let r = run(&mut s, "spec.origin", json!({}));
        assert_eq!(r["recorded"], true);
        assert_eq!(r["counts"], json!({"master": 6}));
        run(&mut s, "spec.fields.set", json!({"id": "f0001", "option": 1}));
        assert_eq!(origin_of(&mut s, "p0004"), "master", "choosing a fill-in isn't rewording");
    }

    #[test]
    fn rewording_shows_as_modified_and_restore_puts_the_master_wording_back() {
        let mut s = section();
        let at = Pos::new(StoryRef::Body, Path::top(4), OBJ.len_utf8() + "Deliver panels".len());
        s.sel = Selection::caret(at);
        run(&mut s, "text.insert", json!({"text": " and trim"}));
        assert_eq!(text(&s, 4), "Deliver panels and trim dry.");
        assert_eq!(origin_of(&mut s, "p0005"), "modified");
        let r = run(&mut s, "spec.origin", json!({}));
        let row = r["items"].as_array().unwrap().iter().find(|i| i["id"] == "p0005").unwrap().clone();
        assert_eq!(row["original"], "Deliver panels dry.");
        run(&mut s, "spec.restore", json!({"id": "p0005"}));
        assert_eq!(text(&s, 4), "Deliver panels dry.");
        assert_eq!(origin_of(&mut s, "p0005"), "master");
        // A reworded paragraph with a fill-in gets its fill-in back.
        run(&mut s, "spec.fields.set", json!({"id": "f0001", "option": 0}));
        s.sel = Selection::caret(Pos::new(StoryRef::Body, Path::top(3), OBJ.len_utf8()));
        run(&mut s, "text.insert", json!({"text": "Panel "}));
        assert_eq!(origin_of(&mut s, "p0004"), "modified");
        run(&mut s, "spec.restore", json!({"id": "p0004"}));
        assert_eq!(text(&s, 3), "Thickness: [1/2 inch] [5/8 inch].");
        assert_eq!(run(&mut s, "spec.fields.list", json!({}))["count"], 1);
        // Undo takes the restore back.
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(origin_of(&mut s, "p0004"), "modified");
    }

    #[test]
    fn paragraphs_are_added_duplicated_and_removed_but_master_paragraphs_are_not() {
        let mut s = section();
        run(&mut s, "spec.activate", json!({"ids": ["p0002", "p0003", "p0004"]}));
        let r = run(&mut s, "spec.duplicate", json!({"id": "p0004"}));
        assert_eq!((r["id"].clone(), r["on"].clone()), (json!("u0001"), json!(true)));
        assert_eq!(text(&s, 4), "Thickness: [1/2 inch] [5/8 inch].");
        assert_eq!(origin_of(&mut s, "u0001"), "added");
        let r = run(&mut s, "spec.addParagraph", json!({"after": "p0004", "text": "Provide corner bead.", "child": true}));
        assert_eq!((r["id"].clone(), r["style"].clone(), r["on"].clone()), (json!("u0002"), json!("PR2"), json!(true)));
        assert_eq!(text(&s, 4), "Provide corner bead.");
        let r = run(&mut s, "spec.addParagraph", json!({"after": "p0003", "text": "Store flat."}));
        assert_eq!(r["style"], "ART");
        assert!(sections::printable(&s.doc).iter().any(|e| e.id == "u0002"), "an added paragraph under a printing one prints");
        // Master paragraphs can't be removed; added ones can, children first.
        assert!(s.run("spec.removeParagraph", &json!({"id": "p0005"})).unwrap_err().to_string().contains("switch it off"));
        run(&mut s, "spec.removeParagraph", json!({"id": "u0002"}));
        run(&mut s, "spec.removeParagraph", json!({"id": "u0001"}));
        assert_eq!(run(&mut s, "spec.origin", json!({}))["counts"], json!({"added": 1, "master": 6}));
    }

    #[test]
    fn several_added_paragraphs_are_deleted_from_a_selection_and_master_ones_kept() {
        let mut s = section();
        run(&mut s, "spec.activate", json!({"ids": ["p0002", "p0003", "p0004"]}));
        run(&mut s, "spec.addParagraph", json!({"after": "p0004", "text": "One."}));
        run(&mut s, "spec.addParagraph", json!({"after": "u0001", "text": "Two."}));
        run(&mut s, "spec.addParagraph", json!({"after": "u0002", "text": "Under two.", "child": true}));
        // p0004 (block 3) through u0003: one master paragraph and three added ones.
        let blocks: Vec<u32> = sections::outline(&s.doc)
            .iter()
            .filter(|e| ["p0004", "u0003"].contains(&e.id.as_str()))
            .filter_map(|e| e.range.start.path.0.first().copied())
            .collect();
        s.sel = Selection {
            anchor: Pos::new(StoryRef::Body, Path::top(blocks[0] as usize), 1),
            focus: Pos::new(StoryRef::Body, Path::top(blocks[1] as usize), 2),
        };
        let r = run(&mut s, "spec.removeParagraph", json!({}));
        assert_eq!((r["ids"].clone(), r["kept"].clone()), (json!(["u0001", "u0002", "u0003"]), json!(1)));
        assert_eq!(run(&mut s, "spec.origin", json!({}))["counts"], json!({"master": 6}));
        // A parent goes only with what is under it.
        run(&mut s, "spec.addParagraph", json!({"after": "p0004", "text": "A."}));
        run(&mut s, "spec.addParagraph", json!({"after": "u0001", "text": "B.", "child": true}));
        assert!(s.run("spec.removeParagraph", &json!({"ids": ["u0001"]})).is_err());
        run(&mut s, "spec.removeParagraph", json!({"ids": ["u0001", "u0002"]}));
        // A selection of master paragraphs only deletes nothing.
        s.sel = Selection { anchor: Pos::new(StoryRef::Body, Path::top(1), 0), focus: Pos::new(StoryRef::Body, Path::top(2), 0) };
        assert!(s.run("spec.removeParagraph", &json!({})).is_err());
    }

    #[test]
    fn a_deleted_master_paragraph_is_listed_and_put_back() {
        let mut s = section();
        // Delete p0005 the way a person would: select the paragraph and its mark and delete.
        s.doc.body.remove(4);
        let r = run(&mut s, "spec.origin", json!({}));
        assert_eq!(r["removed"], json!([{"id": "p0005", "style": "PR1", "text": "Deliver panels dry."}]));
        run(&mut s, "spec.restore", json!({"id": "p0005"}));
        assert_eq!(text(&s, 4), "Deliver panels dry.");
        assert_eq!(s.doc.body[4].as_para().unwrap().props.style.as_deref(), Some("PR1"));
        assert_eq!(run(&mut s, "spec.origin", json!({}))["removed"], json!([]));
    }

    #[test]
    fn the_wording_survives_saving_and_comes_from_the_master_for_older_copies() {
        let mut s = section();
        let bytes = crate::io::save_bytes("x.docx", &s.doc).unwrap();
        let mut b = Session::new(crate::io::open_bytes("x.docx", &bytes).unwrap());
        assert_eq!(run(&mut b, "spec.origin", json!({}))["counts"], json!({"master": 6}));
        // A copy without the property reads its master.
        let dir = std::env::temp_dir().join(format!("speccraft-edits-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut m = Document::new();
        m.body = (0..3).map(|k| para(["SCT", "PRT", "EOS"][k], ["SECTION 01 0000 - TEST", "GENERAL", "END OF SECTION"][k])).collect();
        let mp = dir.join("m.docx");
        crate::io::save_path(&mp, &m).unwrap();
        let mut c = Session::new(m);
        run(&mut c, "spec.import", json!({}));
        c.doc.remove_custom_prop(ORIGINALS_PROP);
        c.doc.set_custom_prop(super::super::project::MASTER_PROP, &json!({"path": mp.to_string_lossy()}).to_string());
        assert_eq!(originals(&c.doc).get("p0002"), Some(&Original("PRT".into(), "GENERAL".into())));
        s.doc.remove_custom_prop(ORIGINALS_PROP);
        let _ = run(&mut s, "spec.origin", json!({}));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
