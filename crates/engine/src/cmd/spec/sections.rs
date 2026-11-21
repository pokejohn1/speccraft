//! Specs › Sections: a spec section as a library of ready-made paragraphs that are switched on
//! and off (milestone M2). Added by the Speccraft fork; not part of the original WordCraft.
//!
//! `spec.import` wraps every top-level body paragraph in a block spec item (`spec:p0001`, … see
//! [`super`]). What an item is comes from its paragraph style (SCT, PRT, ART, PR1…PR5, CMT, EOS)
//! or the [`NOTE_PREFIX`]; which items are on is the custom document property [`STATE_PROP`], a
//! comma-separated list of ids, so a project section carries its own state inside the .docx.
//!
//! The parent rule: an item's parent is the nearest earlier item with a lower level. An item can
//! only be on while its parent is on, switching one off switches off everything under it, and
//! notes are never on. [`printable`] is the one place that says what prints.

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};
use wordcraft_doc::para::OBJ;
use wordcraft_doc::{Block, ControlKind, ControlRange, Document, Paragraph, Path, Pos, StoryRef, Table};

use super::{MAX_ID, MAX_ITEMS, TAG_PREFIX, check_id, controls, items};
use crate::{CmdError, CmdResult, CommandSpec, Selection, Session};

const LOC: &str = "Specs › Sections";
const IDS: &str = r#"{"id": string} | {"ids": [string]}"#;
/// A paragraph whose text starts with this (after leading spaces) is an author note.
pub const NOTE_PREFIX: &str = "//";
/// The custom document property holding the ids of the items that are on.
pub const STATE_PROP: &str = "Speccraft.On";
/// Document property: ids the author tried to switch on while their parent was off (SpecLink's
/// "refused" state). They come on by themselves once their parent prints.
pub const REFUSED_PROP: &str = "Speccraft.Refused";
/// Document property: ids the author switched off although a rule turned them on (M6: the
/// author has the last say). Switching one on again lifts it.
pub const OVERRIDDEN_PROP: &str = "Speccraft.Overridden";
/// Characters of a paragraph's text `spec.outline` returns.
const OUTLINE_TEXT: usize = 160;
/// Longest state property read, in bytes: every item's id at its longest, plus the commas.
const MAX_STATE: usize = MAX_ITEMS * (MAX_ID + 1);
/// Characters of a paragraph's text shown with a warning.
const WARN_CHARS: usize = 40;
/// Deepest table nesting counted when reporting skipped table paragraphs.
const MAX_TABLE_DEPTH: usize = 8;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("spec.import", "Import Section Paragraphs", LOC, import),
        CommandSpec::new("spec.activate", "Switch Paragraph On", LOC, activate).params(IDS),
        CommandSpec::new("spec.deactivate", "Switch Paragraph Off", LOC, deactivate).params(IDS),
        CommandSpec::new("spec.printable", "List Printable Paragraphs", LOC, printable_cmd).pure(),
        CommandSpec::new("spec.toggle", "Switch Paragraph On or Off", LOC, toggle)
            .key("Mod+Alt+J")
            .params(r#"{"id"?: string (default: the paragraph at the caret)}"#),
        CommandSpec::new("spec.outline", "Spec Outline", LOC, outline_cmd).pure(),
    ]
}

/// What a section paragraph is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Title,
    Part,
    End,
    Article,
    Para,
    Note,
    Other,
}

impl Kind {
    const ALL: [Kind; 7] = [Kind::Title, Kind::Part, Kind::End, Kind::Article, Kind::Para, Kind::Note, Kind::Other];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Title => "title",
            Kind::Part => "part",
            Kind::End => "end",
            Kind::Article => "article",
            Kind::Para => "para",
            Kind::Note => "note",
            Kind::Other => "other",
        }
    }
}

/// The kind and outline level of a paragraph, from its style id and its text. A paragraph with
/// no recognised style is `Other` at level 1, so it can still be switched on and off; [`outline`]
/// never makes it the parent of later paragraphs.
pub fn classify(style: Option<&str>, text: &str) -> (Kind, Option<u8>) {
    if text.trim_start_matches(|c: char| c.is_whitespace() || c == OBJ).starts_with(NOTE_PREFIX) {
        return (Kind::Note, None);
    }
    match style.unwrap_or("") {
        "SCT" => (Kind::Title, Some(0)),
        "PRT" => (Kind::Part, Some(1)),
        "EOS" => (Kind::End, Some(1)),
        "ART" => (Kind::Article, Some(2)),
        "PR1" => (Kind::Para, Some(3)),
        "PR2" => (Kind::Para, Some(4)),
        "PR3" => (Kind::Para, Some(5)),
        "PR4" => (Kind::Para, Some(6)),
        "PR5" => (Kind::Para, Some(7)),
        "CMT" => (Kind::Note, None),
        other => match style_level(other) {
            // Masters that number their levels in the style name ("CSILevel3", "Level 3"):
            // 0 title, 1 Part, 2 Article, 3 and down paragraphs; the end line by its text.
            Some(_) if is_end_line(text) => (Kind::End, Some(1)),
            Some(0) => (Kind::Title, Some(0)),
            Some(1) => (Kind::Part, Some(1)),
            Some(2) => (Kind::Article, Some(2)),
            Some(n) => (Kind::Para, Some(n.min(9))),
            None if is_end_line(text) => (Kind::End, Some(1)),
            None => (Kind::Other, Some(1)),
        },
    }
}

/// The level a style's name gives, for names like `CSILevel3`, `Level 3`, `SpecLevel-3`: the
/// digit after "level", at the end of the name.
pub fn style_level(style: &str) -> Option<u8> {
    let low = style.to_ascii_lowercase();
    let at = low.rfind("level")?;
    let rest = low.get(at + 5..)?.trim_start_matches([' ', '-', '_']);
    if rest.is_empty() || rest.len() > 1 {
        return None;
    }
    rest.parse::<u8>().ok()
}

/// "END OF SECTION …" closes a section, whatever its style.
fn is_end_line(text: &str) -> bool {
    text.trim_start_matches(|c: char| c.is_whitespace() || c == OBJ).get(..14).is_some_and(|h| h.eq_ignore_ascii_case("end of section"))
}

/// One section paragraph: a spec item that is a block control around one top-level body paragraph.
#[derive(Clone, Debug)]
pub struct Entry {
    pub id: String,
    pub kind: Kind,
    pub level: Option<u8>,
    /// Index of the parent in the same outline.
    pub parent: Option<usize>,
    /// On, and so printing (the parent rule and the section's rules applied).
    pub on: bool,
    /// Switched on by the author (stored in `Speccraft.On`).
    pub manual: bool,
    /// Turned on by a rule (M6), whether or not the author also switched it on.
    pub linked: bool,
    pub range: ControlRange,
}

/// The paragraph a spec item wraps, if the item is a section paragraph.
fn section_para<'a>(doc: &'a Document, r: &ControlRange) -> Option<&'a Paragraph> {
    let whole = r.control.block && r.start.story == StoryRef::Body && r.start.path == r.end.path && r.start.path.0.len() == 1;
    if whole { doc.para_at(&r.start) } else { None }
}

/// The ids the state property names (unknown ones are ignored by [`outline`]).
fn stored_on(doc: &Document) -> HashSet<&str> {
    let v = doc.custom_prop(STATE_PROP).unwrap_or("");
    let end = (0..=v.len().min(MAX_STATE)).rev().find(|i| v.is_char_boundary(*i)).unwrap_or(0);
    v.get(..end).unwrap_or("").split(',').map(str::trim).filter(|id| !id.is_empty()).take(MAX_ITEMS).collect()
}

/// Every section paragraph in document order, with its kind, parent and state.
pub fn outline(doc: &Document) -> Vec<Entry> {
    let stored = stored_on(doc);
    let mut out: Vec<Entry> = Vec::new();
    // Items that can still be parents: (level, index), levels rising.
    let mut open: Vec<(u8, usize)> = Vec::new();
    for (id, range) in items(doc) {
        let Some(para) = section_para(doc, &range) else { continue };
        let (kind, level) = classify(para.props.style.as_deref(), &para.text);
        let parent = level.and_then(|l| {
            if kind == Kind::Other {
                // Looks its parent up without closing anything: it is a leaf (see below).
                return open.iter().rev().find(|(pl, _)| *pl < l).map(|(_, i)| *i);
            }
            while open.last().is_some_and(|(pl, _)| *pl >= l) {
                open.pop();
            }
            open.last().map(|(_, i)| *i)
        });
        let manual = kind != Kind::Note && stored.contains(id.as_str());
        // A paragraph with no spec style is a leaf: if it became a parent, switching off one stray
        // paragraph would take every later Article of its Part with it.
        if let Some(l) = level.filter(|_| kind != Kind::Other) {
            open.push((l, out.len()));
        }
        out.push(Entry { id, kind, level, parent, on: false, manual, linked: false, range });
    }
    let manual: Vec<bool> = out.iter().map(|e| e.manual).collect();
    let (on, linked) = super::rules::evaluate(doc, &out, &manual, &stored_overridden(doc));
    for ((e, on), linked) in out.iter_mut().zip(on).zip(linked) {
        e.on = on;
        e.linked = linked;
    }
    out
}

/// The ids the overridden property names.
pub fn stored_overridden(doc: &Document) -> HashSet<String> {
    let v = doc.custom_prop(OVERRIDDEN_PROP).unwrap_or("");
    let end = (0..=v.len().min(MAX_STATE)).rev().find(|i| v.is_char_boundary(*i)).unwrap_or(0);
    v.get(..end).unwrap_or("").split(',').map(str::trim).filter(|id| !id.is_empty()).take(MAX_ITEMS).map(str::to_string).collect()
}

/// Write the overridden property: those still section paragraphs, in document order.
fn write_overridden(doc: &mut Document, entries: &[Entry], overridden: &HashSet<String>) {
    let mut seen = HashSet::new();
    let ids: Vec<&str> = entries.iter().filter(|e| overridden.contains(&e.id) && seen.insert(e.id.as_str())).map(|e| e.id.as_str()).collect();
    if ids.is_empty() {
        doc.remove_custom_prop(OVERRIDDEN_PROP);
    } else {
        doc.set_custom_prop(OVERRIDDEN_PROP, &ids.join(","));
    }
}

/// Apply the parent rule to what the author switched on: a paragraph whose parent doesn't print
/// is switched off too. Returns what prints afterwards.
fn tidy(doc: &Document, entries: &[Entry], manual: &mut [bool], overridden: &HashSet<String>) -> Vec<bool> {
    for _ in 0..8 {
        let (on, _) = super::rules::evaluate(doc, entries, manual, overridden);
        let mut changed = false;
        for (i, e) in entries.iter().enumerate() {
            let parent_prints = e.parent.is_none_or(|p| on.get(p).copied().unwrap_or(false));
            if let Some(m) = manual.get_mut(i)
                && *m
                && (!parent_prints || e.kind == Kind::Note)
            {
                *m = false;
                changed = true;
            }
        }
        if !changed {
            return on;
        }
    }
    super::rules::evaluate(doc, entries, manual, overridden).0
}

/// The paragraphs that print, in document order.
pub fn printable(doc: &Document) -> Vec<Entry> {
    outline(doc).into_iter().filter(|e| e.on).collect()
}

/// Write the state property: the ids that are on, in document order, once each.
fn write_state(doc: &mut Document, entries: &[Entry], on: &[bool]) {
    let mut seen = HashSet::new();
    let ids: Vec<&str> = entries.iter().zip(on).filter(|(e, on)| **on && seen.insert(e.id.as_str())).map(|(e, _)| e.id.as_str()).collect();
    if ids.is_empty() {
        doc.remove_custom_prop(STATE_PROP);
    } else {
        doc.set_custom_prop(STATE_PROP, &ids.join(","));
    }
}

/// The ids the refused property names.
fn stored_refused(doc: &Document) -> HashSet<String> {
    let v = doc.custom_prop(REFUSED_PROP).unwrap_or("");
    let end = (0..=v.len().min(MAX_STATE)).rev().find(|i| v.is_char_boundary(*i)).unwrap_or(0);
    v.get(..end).unwrap_or("").split(',').map(str::trim).filter(|id| !id.is_empty()).take(MAX_ITEMS).map(str::to_string).collect()
}

/// Write the refused property: the refused ids that are still section paragraphs and off, in
/// document order.
fn write_refused(doc: &mut Document, entries: &[Entry], on: &[bool], refused: &HashSet<String>) {
    let mut seen = HashSet::new();
    let ids: Vec<&str> = entries
        .iter()
        .zip(on)
        .filter(|(e, on)| !**on && e.kind != Kind::Note && refused.contains(&e.id) && seen.insert(e.id.as_str()))
        .map(|(e, _)| e.id.as_str())
        .collect();
    if ids.is_empty() {
        doc.remove_custom_prop(REFUSED_PROP);
    } else {
        doc.set_custom_prop(REFUSED_PROP, &ids.join(","));
    }
}

/// Switch on refused items whose parent now prints and take them off the refused list (again
/// until none is left, so chains come on).
fn release_refused(doc: &Document, entries: &[Entry], manual: &mut [bool], overridden: &HashSet<String>, refused: &mut HashSet<String>) {
    for _ in 0..16 {
        let (on, _) = super::rules::evaluate(doc, entries, manual, overridden);
        let mut changed = false;
        for (i, e) in entries.iter().enumerate() {
            if !refused.contains(&e.id) || e.kind == Kind::Note {
                continue;
            }
            if e.parent.is_none_or(|p| on.get(p).copied().unwrap_or(false)) {
                if let Some(x) = manual.get_mut(i) {
                    *x = true;
                }
                refused.remove(&e.id);
                changed = true;
            }
        }
        if !changed {
            return;
        }
    }
}

/// How a section paragraph shows: printing, off, refused (wanted, but its parent is off), or a note.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    On,
    /// On because a rule turned it on (M6).
    Linked,
    Off,
    Refused,
    /// Off because the author switched it off against a rule (M6).
    Overridden,
    Note,
}

impl State {
    pub fn name(self) -> &'static str {
        match self {
            State::On => "on",
            State::Linked => "linked",
            State::Off => "off",
            State::Overridden => "overridden",
            State::Refused => "refused",
            State::Note => "note",
        }
    }
}

/// Every section paragraph with how it shows.
pub fn states(doc: &Document) -> Vec<(Entry, State)> {
    let refused = stored_refused(doc);
    let overridden = stored_overridden(doc);
    outline(doc)
        .into_iter()
        .map(|e| {
            let st = if e.kind == Kind::Note {
                State::Note
            } else if e.on && e.linked && !e.manual {
                State::Linked
            } else if e.on {
                State::On
            } else if refused.contains(&e.id) {
                State::Refused
            } else if overridden.contains(&e.id) {
                State::Overridden
            } else {
                State::Off
            };
            (e, st)
        })
        .collect()
}

/// The section paragraph the caret is in (body only).
pub fn at_caret(s: &Session, entries: &[Entry]) -> Option<usize> {
    let f = &s.sel.focus;
    if f.story != StoryRef::Body {
        return None;
    }
    let block = *f.path.0.first()?;
    entries.iter().position(|e| e.range.start.path.0.first() == Some(&block))
}

/// The section paragraph index of each `id` / `ids` param, in document order, once each.
fn targets(v: &Value, entries: &[Entry]) -> Result<Vec<usize>, CmdError> {
    let mut ids = Vec::new();
    if v.get("id").is_some() {
        ids.push(super::id_param(v)?);
    }
    if let Some(list) = v.get("ids") {
        let list = list.as_array().ok_or_else(|| CmdError::Params("`ids` must be a list of strings".into()))?;
        if list.len() > MAX_ITEMS {
            return Err(CmdError::Params(format!("at most {MAX_ITEMS} ids at once")));
        }
        for x in list {
            ids.push(check_id(x.as_str().ok_or_else(|| CmdError::Params("`ids` must be a list of strings".into()))?)?);
        }
    }
    if ids.is_empty() {
        return Err(CmdError::Params("`id` (string) or `ids` (list of strings) is required".into()));
    }
    let mut index: HashMap<&str, usize> = HashMap::new();
    for (i, e) in entries.iter().enumerate() {
        index.entry(e.id.as_str()).or_insert(i);
    }
    let mut out = Vec::new();
    for id in &ids {
        let i = index.get(id.as_str()).ok_or_else(|| CmdError::Failed(format!("no section paragraph called {id:?} (run spec.import first?)")))?;
        out.push(*i);
    }
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

/// What `spec.activate` and `spec.deactivate` return: the ids whose state changed and how many are on.
fn change_result(entries: &[Entry], on: &[bool]) -> Value {
    let changed: Vec<&str> = entries.iter().zip(on).filter(|(e, on)| e.on != **on).map(|(e, _)| e.id.as_str()).collect();
    json!({"changed": changed, "onCount": on.iter().filter(|x| **x).count()})
}

/// Switch items on, in document order. Refused (changing nothing) for a note, or when an item's
/// parent is off; switching on does not switch on children.
fn activate(s: &mut Session, v: &Value) -> CmdResult {
    let entries = outline(&s.doc);
    let targets = targets(v, &entries)?;
    let mut on: Vec<bool> = entries.iter().map(|e| e.on).collect();
    let mut manual: Vec<bool> = entries.iter().map(|e| e.manual).collect();
    let mut overridden = stored_overridden(&s.doc);
    for i in targets {
        let Some(e) = entries.get(i) else { continue };
        if e.kind == Kind::Note {
            return Err(CmdError::Failed(format!("{:?} is an author note: notes never print, so they can't be switched on", e.id)));
        }
        if let Some(p) = e.parent
            && !on.get(p).copied().unwrap_or(false)
        {
            let parent = entries.get(p).map_or("?", |x| x.id.as_str());
            return Err(CmdError::Failed(format!("can't switch on {:?}: its parent paragraph {parent:?} is off", e.id)));
        }
        if !e.on
            && let Some(why) = super::rules::blocking(&s.doc, &entries, &on, i)
        {
            return Err(CmdError::Failed(format!("can't switch on {:?}: {why}", e.id)));
        }
        if let Some(x) = on.get_mut(i) {
            *x = true;
        }
        if let Some(x) = manual.get_mut(i) {
            *x = true;
        }
        overridden.remove(&e.id);
    }
    let mut refused = stored_refused(&s.doc);
    release_refused(&s.doc, &entries, &mut manual, &overridden, &mut refused);
    let on = tidy(&s.doc, &entries, &mut manual, &overridden);
    write_state(&mut s.doc, &entries, &manual);
    write_overridden(&mut s.doc, &entries, &overridden);
    write_refused(&mut s.doc, &entries, &on, &refused);
    Ok(change_result(&entries, &on))
}

/// Switch items off, and everything under them.
fn deactivate(s: &mut Session, v: &Value) -> CmdResult {
    let entries = outline(&s.doc);
    let mut manual: Vec<bool> = entries.iter().map(|e| e.manual).collect();
    let mut overridden = stored_overridden(&s.doc);
    let mut refused = stored_refused(&s.doc);
    let targets = targets(v, &entries)?;
    for i in &targets {
        if let Some(x) = manual.get_mut(*i) {
            *x = false;
        }
        if let Some(e) = entries.get(*i) {
            refused.remove(&e.id);
        }
    }
    // A paragraph a rule would keep on is overridden: the author has the last say.
    let (still, _) = super::rules::evaluate(&s.doc, &entries, &manual, &overridden);
    for i in &targets {
        if still.get(*i).copied().unwrap_or(false)
            && let Some(e) = entries.get(*i)
        {
            overridden.insert(e.id.clone());
        }
    }
    let on = tidy(&s.doc, &entries, &mut manual, &overridden);
    write_state(&mut s.doc, &entries, &manual);
    write_overridden(&mut s.doc, &entries, &overridden);
    write_refused(&mut s.doc, &entries, &on, &refused);
    Ok(change_result(&entries, &on))
}

/// Switch one paragraph (`id`, or the one at the caret) on or off. A paragraph whose parent is
/// off is marked refused instead, and comes on when its parent does; toggling a refused
/// paragraph clears it. Notes can't be switched.
fn toggle(s: &mut Session, v: &Value) -> CmdResult {
    let entries = outline(&s.doc);
    let i = match v.get("id") {
        Some(_) => targets(v, &entries)?.first().copied().ok_or_else(|| CmdError::Params("`id` is required".into()))?,
        None => at_caret(s, &entries).ok_or_else(|| CmdError::Failed("the caret is not in a section paragraph (run spec.import first?)".into()))?,
    };
    let Some(e) = entries.get(i) else { return Err(CmdError::Failed("no such paragraph".into())) };
    if e.kind == Kind::Note {
        return Err(CmdError::Failed(format!("{:?} is an author note: notes never print", e.id)));
    }
    let id = e.id.clone();
    let mut refused = stored_refused(&s.doc);
    if e.on || refused.contains(&id) {
        let r = deactivate(s, &json!({"id": id}))?;
        return Ok(json!({"id": id, "state": "off", "changed": r["changed"]}));
    }
    let parent_prints = e.parent.is_none_or(|p| entries.get(p).is_some_and(|x| x.on));
    if parent_prints {
        let r = activate(s, &json!({"id": id}))?;
        return Ok(json!({"id": id, "state": "on", "changed": r["changed"]}));
    }
    let on_now: Vec<bool> = entries.iter().map(|e| e.on).collect();
    if let Some(why) = super::rules::blocking(&s.doc, &entries, &on_now, i) {
        return Err(CmdError::Failed(format!("can't switch on {id:?}: {why}")));
    }
    refused.insert(id.clone());
    let on: Vec<bool> = entries.iter().map(|e| e.on).collect();
    write_refused(&mut s.doc, &entries, &on, &refused);
    let parent = e.parent.and_then(|p| entries.get(p)).map(|p| p.id.clone());
    Ok(json!({"id": id, "state": "refused", "parent": parent, "changed": []}))
}

/// Every section paragraph with its state, for the Spec Outline pane and the page's marks.
fn outline_cmd(s: &mut Session, _v: &Value) -> CmdResult {
    let st = states(&s.doc);
    let entries: Vec<Entry> = st.iter().map(|(e, _)| e.clone()).collect();
    let caret = at_caret(s, &entries).and_then(|i| entries.get(i)).map(|e| e.id.clone());
    let mut counts: HashMap<&str, usize> = HashMap::new();
    let items: Vec<Value> = st
        .iter()
        .map(|(e, state)| {
            *counts.entry(state.name()).or_default() += 1;
            let text: String = s.doc.control_text(&e.range).replace(OBJ, "").chars().take(OUTLINE_TEXT).collect();
            json!({
                "id": e.id,
                "kind": e.kind.name(),
                "level": e.level,
                "parent": e.parent.and_then(|p| entries.get(p)).map(|p| p.id.clone()),
                "state": state.name(),
                "block": e.range.start.path.0.first(),
                "text": text,
            })
        })
        .collect();
    Ok(json!({"items": items, "caret": caret, "counts": counts}))
}

fn printable_cmd(s: &mut Session, _v: &Value) -> CmdResult {
    let rows: Vec<Value> = printable(&s.doc).iter().map(|e| json!({"id": e.id, "text": s.doc.control_text(&e.range)})).collect();
    Ok(json!({"count": rows.len(), "items": rows}))
}

/// The extra `spec.list` fields of each section paragraph, by id.
pub fn list_fields(doc: &Document) -> HashMap<String, Value> {
    let entries = outline(doc);
    let mut out = HashMap::new();
    for e in &entries {
        let parent = e.parent.and_then(|p| entries.get(p)).map(|p| p.id.clone());
        out.entry(e.id.clone()).or_insert_with(|| json!({"kind": e.kind.name(), "level": e.level, "on": e.on, "parent": parent}));
    }
    out
}

/// The number in an id `spec.import` gives (`p0042` is 42).
fn import_number(id: &str) -> Option<u32> {
    let digits = id.strip_prefix('p')?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Paragraphs in a table, nested tables included (bounded).
fn table_paras(t: &Table, depth: usize) -> usize {
    if depth > MAX_TABLE_DEPTH {
        return 0;
    }
    t.rows
        .iter()
        .flat_map(|r| &r.cells)
        .flat_map(|c| &c.blocks)
        .map(|b| match b.as_ref() {
            Block::Para(_) => 1,
            Block::Table(t) => table_paras(t, depth.saturating_add(1)),
        })
        .fold(0, usize::saturating_add)
}

/// Wrap every top-level body paragraph not yet in a spec item as one (`spec:p0001`, …, numbered
/// after the highest existing), so running it again only adds new paragraphs. New items start
/// off, except the section title and end, which start on.
fn import(s: &mut Session, _v: &Value) -> CmdResult {
    let existing = items(&s.doc);
    let mut next = u64::from(existing.iter().filter_map(|(id, _)| import_number(id)).max().unwrap_or(0)).saturating_add(1);
    // Top-level paragraphs a spec item already starts in or covers.
    let mut carried = HashSet::new();
    for (_, r) in existing.iter().filter(|(_, r)| r.start.story == StoryRef::Body) {
        let (Some(a), Some(b)) = (r.start.path.0.first(), r.end.path.0.first()) else { continue };
        if r.start.path.0.len() == 1 {
            carried.extend(*a..=*b);
        }
    }
    let mut room = MAX_ITEMS.saturating_sub(existing.len());
    let (mut in_tables, mut empty, mut limited) = (0usize, 0usize, false);
    let mut wrapped = HashSet::new();
    let mut new_ids = Vec::new();
    let sel = s.sel.clone();
    for i in 0..s.doc.body.len() {
        let Some(block) = s.doc.body.get(i) else { break };
        let len = match block.as_ref() {
            Block::Table(t) => {
                in_tables = in_tables.saturating_add(table_paras(t, 0));
                continue;
            }
            Block::Para(p) => p.len(),
        };
        let Ok(top) = u32::try_from(i) else { break };
        if carried.contains(&top) {
            continue;
        }
        // An empty paragraph would get the control's placeholder text.
        if len == 0 {
            empty += 1;
            continue;
        }
        if room == 0 {
            limited = true;
            break;
        }
        let id = format!("p{next:04}");
        next = next.saturating_add(1);
        let path = Path::top(i);
        s.sel = Selection { anchor: Pos::new(StoryRef::Body, path.clone(), 0), focus: Pos::new(StoryRef::Body, path, len) };
        controls::insert(s, &json!({"tag": format!("{TAG_PREFIX}{id}")}), ControlKind::RichText)?;
        wrapped.insert(top);
        new_ids.push(id);
        room -= 1;
    }
    // The caret stays where it was: a wrapped paragraph's text moved past the start marker.
    let shift = |mut p: Pos| {
        if p.story == StoryRef::Body && p.path.0.len() == 1 && p.path.0.first().is_some_and(|b| wrapped.contains(b)) {
            p.off = p.off.saturating_add(OBJ.len_utf8());
        }
        p
    };
    s.sel = Selection { anchor: shift(sel.anchor), focus: shift(sel.focus) };

    let entries = outline(&s.doc);
    let fresh: HashSet<&str> = new_ids.iter().map(String::as_str).collect();
    let mut manual: Vec<bool> =
        entries.iter().map(|e| e.manual || (fresh.contains(e.id.as_str()) && matches!(e.kind, Kind::Title | Kind::End))).collect();
    let on = tidy(&s.doc, &entries, &mut manual, &stored_overridden(&s.doc));
    write_state(&mut s.doc, &entries, &manual);
    // The master's wording, recorded once at the first import (M8: origin and restore).
    super::edits::record_originals(&mut s.doc);

    let kinds: serde_json::Map<String, Value> =
        Kind::ALL.iter().map(|k| (k.name().to_string(), json!(entries.iter().filter(|e| e.kind == *k).count()))).collect();
    let warnings: Vec<Value> = entries
        .iter()
        .filter(|e| e.kind == Kind::Other)
        .map(|e| {
            let text: String = s.doc.control_text(&e.range).chars().take(WARN_CHARS).collect();
            json!({"id": e.id, "text": text, "message": "no recognised spec style: switched on and off on its own, under the section title"})
        })
        .collect();
    Ok(json!({
        "imported": new_ids.len(),
        "items": entries.len(),
        "kinds": kinds,
        "warnings": warnings,
        "skippedTableParagraphs": in_tables,
        "skippedEmpty": empty,
        "limitReached": limited,
        "on": on.iter().filter(|x| **x).count(),
    }))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::{Value, json};
    use wordcraft_doc::{Block, Document, Paragraph, Table};

    use super::{Kind, STATE_PROP, classify};
    use crate::Session;

    /// A small section: (style, text) per paragraph; "" is no style.
    const SECTION: [(&str, &str); 12] = [
        ("SCT", "SECTION 09 2900 - GYPSUM BOARD"), // p0001
        ("PRT", "GENERAL"),                        // p0002
        ("ART", "SUMMARY"),                        // p0003
        ("CMT", "Retain or delete this article."), // p0004
        ("PR1", "Section includes:"),              // p0005
        ("PR2", "Interior gypsum board."),         // p0006
        ("PR2", "Tile backing panels."),           // p0007
        ("PR1", "  // a note written inline"),     // p0008
        ("ART", "SUBMITTALS"),                     // p0009
        ("PRT", "PRODUCTS"),                       // p0010
        ("", "Loose text with no spec style"),     // p0011
        ("EOS", "END OF SECTION 09 2900"),         // p0012
    ];

    fn para(style: &str, text: &str) -> Arc<Block> {
        let p = Paragraph::with_text(text, Default::default());
        Arc::new(Block::Para(if style.is_empty() { p } else { p.styled(style) }))
    }

    fn doc(paras: &[(&str, &str)]) -> Document {
        let mut d = Document::new();
        d.body = paras.iter().map(|(style, text)| para(style, text)).collect();
        d
    }

    fn run(s: &mut Session, id: &str, v: Value) -> Value {
        s.run(id, &v).unwrap_or_else(|e| panic!("{id}: {e}"))
    }

    fn imported() -> Session {
        let mut s = Session::new(doc(&SECTION));
        run(&mut s, "spec.import", json!({}));
        s
    }

    fn printable(s: &mut Session) -> Vec<String> {
        let r = run(s, "spec.printable", json!({}));
        r["items"].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap().to_string()).collect()
    }

    /// Item `id`'s row in `spec.list`.
    fn row(s: &mut Session, id: &str) -> Value {
        let r = run(s, "spec.list", json!({}));
        r["items"].as_array().unwrap().iter().find(|i| i["id"] == id).cloned().unwrap_or_else(|| panic!("no {id}"))
    }

    fn styles(s: &Session) -> Vec<Option<String>> {
        s.doc.body.iter().map(|b| b.as_para().and_then(|p| p.props.style.clone())).collect()
    }

    #[test]
    fn styles_give_kinds_and_levels() {
        assert_eq!(classify(Some("SCT"), "x"), (Kind::Title, Some(0)));
        assert_eq!(classify(Some("PRT"), "x"), (Kind::Part, Some(1)));
        assert_eq!(classify(Some("EOS"), "x"), (Kind::End, Some(1)));
        assert_eq!(classify(Some("ART"), "x"), (Kind::Article, Some(2)));
        for (n, style) in ["PR1", "PR2", "PR3", "PR4", "PR5"].iter().enumerate() {
            assert_eq!(classify(Some(style), "x"), (Kind::Para, Some(n as u8 + 3)), "{style}");
        }
        assert_eq!(classify(Some("CMT"), "x"), (Kind::Note, None));
        // Level-numbered style names (I3: the author's CSILevel masters).
        assert_eq!(classify(Some("CSILevel0"), "Section 099123 Interior Painting"), (Kind::Title, Some(0)));
        assert_eq!(classify(Some("CSILevel0"), "END OF SECTION  099123"), (Kind::End, Some(1)));
        assert_eq!(classify(Some("CSILevel1"), "GENERAL"), (Kind::Part, Some(1)));
        assert_eq!(classify(Some("CSILevel2"), "SECTION INCLUDES"), (Kind::Article, Some(2)));
        assert_eq!(classify(Some("CSILevel3"), "Surface preparation."), (Kind::Para, Some(3)));
        assert_eq!(classify(Some("CSILevel5"), "x"), (Kind::Para, Some(5)));
        assert_eq!(classify(Some("Level 4"), "x"), (Kind::Para, Some(4)));
        assert_eq!(classify(Some("Normal"), "END OF SECTION"), (Kind::End, Some(1)));
        assert_eq!(classify(Some("Heading1"), "x"), (Kind::Other, Some(1)), "not a level name");
        assert_eq!(classify(Some("Level12"), "x"), (Kind::Other, Some(1)));
        assert_eq!(classify(Some("PR3"), " \t// note"), (Kind::Note, None));
        assert_eq!(classify(None, "x"), (Kind::Other, Some(1)));
        assert_eq!(classify(Some("Normal"), "a // b"), (Kind::Other, Some(1)));
    }

    #[test]
    fn import_reports_kinds_parents_and_warnings_and_keeps_styles() {
        let mut s = Session::new(doc(&SECTION));
        let before = (styles(&s), s.doc.plain_text(wordcraft_doc::StoryRef::Body));
        let r = run(&mut s, "spec.import", json!({}));
        assert_eq!(r["imported"], 12);
        assert_eq!(r["items"], 12);
        assert_eq!(r["kinds"], json!({"title": 1, "part": 2, "end": 1, "article": 2, "para": 3, "note": 2, "other": 1}));
        assert_eq!(r["skippedTableParagraphs"], 0);
        assert_eq!(r["warnings"].as_array().unwrap().len(), 1);
        assert_eq!(r["warnings"][0]["id"], "p0011");
        assert_eq!(r["warnings"][0]["text"], "Loose text with no spec style");
        assert_eq!(styles(&s), before.0, "styles never change");
        assert_eq!(s.doc.plain_text(wordcraft_doc::StoryRef::Body).replace(wordcraft_doc::para::OBJ, ""), before.1);

        let parents = [
            ("p0001", None, Some(0)),
            ("p0002", Some("p0001"), Some(1)),
            ("p0003", Some("p0002"), Some(2)),
            ("p0004", None, None),
            ("p0005", Some("p0003"), Some(3)),
            ("p0006", Some("p0005"), Some(4)),
            ("p0007", Some("p0005"), Some(4)),
            ("p0008", None, None),
            ("p0009", Some("p0002"), Some(2)),
            ("p0010", Some("p0001"), Some(1)),
            ("p0011", Some("p0001"), Some(1)),
            ("p0012", Some("p0001"), Some(1)),
        ];
        let listed = run(&mut s, "spec.list", json!({}));
        for (n, (id, parent, level)) in parents.iter().enumerate() {
            let item = &listed["items"][n];
            assert_eq!(item["id"], *id);
            assert_eq!(item["parent"], json!(parent), "{id}");
            assert_eq!(item["level"], json!(level), "{id}");
            assert!(item["text"].is_string() && item["title"].is_string() && item["start"].is_object(), "M1 keys stay: {item}");
        }
        assert_eq!(listed["items"][3]["kind"], "note");
        assert_eq!(listed["items"][7]["kind"], "note");
        assert_eq!(listed["items"][10]["kind"], "other");
    }

    #[test]
    fn only_title_and_end_start_on() {
        let mut s = imported();
        assert_eq!(printable(&mut s), ["p0001", "p0012"]);
        assert_eq!(s.doc.custom_prop(STATE_PROP), Some("p0001,p0012"));
        assert_eq!(row(&mut s, "p0002")["on"], false);
        assert_eq!(row(&mut s, "p0012")["on"], true);
    }

    #[test]
    fn notes_never_print_and_cannot_be_switched_on() {
        let mut s = imported();
        run(&mut s, "spec.activate", json!({"ids": ["p0002", "p0003", "p0005"]}));
        let before = printable(&mut s);
        for note in ["p0004", "p0008"] {
            let e = s.run("spec.activate", &json!({"id": note})).unwrap_err().to_string();
            assert!(e.contains("note"), "{e}");
            assert_eq!(row(&mut s, note)["on"], false);
        }
        run(&mut s, "spec.deactivate", json!({"id": "p0004"}));
        // Hand-edited state naming a note still doesn't print it.
        let state = format!("{},p0004,p0008", s.doc.custom_prop(STATE_PROP).unwrap());
        s.doc.set_custom_prop(STATE_PROP, &state);
        assert_eq!(printable(&mut s), before);
        assert!(!before.iter().any(|id| id == "p0004" || id == "p0008"));
    }

    #[test]
    fn parent_rule() {
        let mut s = imported();
        // A child of a paragraph that is off is refused, and nothing changes.
        let e = s.run("spec.activate", &json!({"id": "p0003"})).unwrap_err().to_string();
        assert!(e.contains("parent") && e.contains("p0002") && e.contains("off"), "{e}");
        assert_eq!(printable(&mut s), ["p0001", "p0012"]);
        // All or nothing: p0002 is fine, p0006's parent p0005 is off.
        assert!(s.run("spec.activate", &json!({"ids": ["p0002", "p0006"]})).is_err());
        assert_eq!(row(&mut s, "p0002")["on"], false);

        // Switching on doesn't switch on the children.
        run(&mut s, "spec.activate", json!({"id": "p0002"}));
        assert_eq!(row(&mut s, "p0003")["on"], false);
        // Several at once go in document order, so a parent and its child work together.
        let r = run(&mut s, "spec.activate", json!({"ids": ["p0006", "p0005", "p0003"]}));
        assert_eq!(r["changed"], json!(["p0003", "p0005", "p0006"]));
        run(&mut s, "spec.activate", json!({"id": "p0010"}));
        assert_eq!(printable(&mut s), ["p0001", "p0002", "p0003", "p0005", "p0006", "p0010", "p0012"]);

        // Again: allowed, nothing changes.
        let r = run(&mut s, "spec.activate", json!({"id": "p0006"}));
        assert_eq!(r["changed"], json!([]));
        let r = run(&mut s, "spec.deactivate", json!({"id": "p0007"}));
        assert_eq!(r["changed"], json!([]));

        // Off takes everything under it along.
        let r = run(&mut s, "spec.deactivate", json!({"id": "p0002"}));
        assert_eq!(r["changed"], json!(["p0002", "p0003", "p0005", "p0006"]));
        assert_eq!(printable(&mut s), ["p0001", "p0010", "p0012"]);
        // Turning the parent back on brings none of them back.
        run(&mut s, "spec.activate", json!({"id": "p0002"}));
        assert_eq!(printable(&mut s), ["p0001", "p0002", "p0010", "p0012"]);
    }

    #[test]
    fn unstyled_paragraph_is_a_warning_but_still_works() {
        let mut s = imported();
        run(&mut s, "spec.activate", json!({"id": "p0011"}));
        assert_eq!(printable(&mut s), ["p0001", "p0011", "p0012"]);
        run(&mut s, "spec.deactivate", json!({"id": "p0001"}));
        assert_eq!(printable(&mut s), Vec::<String>::new(), "the title takes everything under it along");
    }

    #[test]
    fn toggling_a_paragraph_under_an_off_parent_marks_it_refused_until_the_parent_comes_on() {
        let mut s = imported();
        // p0006 (PR2) sits under p0005 (PR1), under SUMMARY (p0003), under GENERAL (p0002): all off.
        let r = run(&mut s, "spec.toggle", json!({"id": "p0006"}));
        assert_eq!((r["state"].clone(), r["parent"].clone()), (json!("refused"), json!("p0005")));
        assert_eq!(s.doc.custom_prop(super::REFUSED_PROP), Some("p0006"));
        let st = |s: &mut Session, id: &str| {
            let o = run(s, "spec.outline", json!({}));
            o["items"].as_array().unwrap().iter().find(|i| i["id"] == id).unwrap()["state"].as_str().unwrap().to_string()
        };
        assert_eq!(st(&mut s, "p0006"), "refused");
        assert_eq!(st(&mut s, "p0004"), "note");
        // Switching the chain on brings the refused paragraph with it.
        run(&mut s, "spec.activate", json!({"ids": ["p0002", "p0003", "p0005"]}));
        assert_eq!(st(&mut s, "p0006"), "on");
        assert_eq!(s.doc.custom_prop(super::REFUSED_PROP), None);
        // Toggling switches off (children too) and on again.
        assert_eq!(run(&mut s, "spec.toggle", json!({"id": "p0005"}))["state"], "off");
        assert_eq!(st(&mut s, "p0006"), "off");
        assert_eq!(run(&mut s, "spec.toggle", json!({"id": "p0005"}))["state"], "on");
        // Toggling a refused paragraph clears it; notes can't be toggled.
        run(&mut s, "spec.deactivate", json!({"id": "p0003"}));
        run(&mut s, "spec.toggle", json!({"id": "p0007"}));
        assert_eq!(st(&mut s, "p0007"), "refused");
        assert_eq!(run(&mut s, "spec.toggle", json!({"id": "p0007"}))["state"], "off");
        assert_eq!(st(&mut s, "p0007"), "off");
        assert!(s.run("spec.toggle", &json!({"id": "p0004"})).is_err());
        // Undo takes a toggle back.
        run(&mut s, "spec.toggle", json!({"id": "p0003"}));
        assert_eq!(st(&mut s, "p0003"), "on");
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(st(&mut s, "p0003"), "off");
    }

    #[test]
    fn toggle_without_an_id_uses_the_paragraph_at_the_caret() {
        let mut s = imported();
        let at = |_s: &Session, block: u32| wordcraft_doc::Pos::new(wordcraft_doc::StoryRef::Body, wordcraft_doc::Path(vec![block]), 0);
        s.sel = crate::Selection::caret(at(&s, 1));
        assert_eq!(run(&mut s, "spec.outline", json!({}))["caret"], "p0002");
        assert_eq!(run(&mut s, "spec.toggle", json!({}))["state"], "on");
        assert_eq!(run(&mut s, "spec.outline", json!({}))["counts"]["on"], 3, "title, GENERAL and end of section");
    }

    #[test]
    fn an_unstyled_paragraph_is_never_a_parent() {
        // A stray unstyled paragraph between two Articles of the same Part.
        let mut s = Session::new(doc(&[
            ("SCT", "SECTION 07 9200 - JOINT SEALANTS"),               // p0001
            ("PRT", "EXECUTION"),                                      // p0002
            ("ART", "PREPARATION"),                                    // p0003
            ("", "Unstyled paragraph left in the master by mistake."), // p0004
            ("ART", "INSTALLATION"),                                   // p0005
            ("PR1", "Install sealants."),                              // p0006
            ("EOS", "END OF SECTION 07 9200"),                         // p0007
        ]));
        run(&mut s, "spec.import", json!({}));
        assert_eq!(row(&mut s, "p0005")["parent"], "p0002", "the Article stays under its Part");
        run(&mut s, "spec.activate", json!({"ids": ["p0002", "p0003", "p0004", "p0005", "p0006"]}));
        run(&mut s, "spec.deactivate", json!({"id": "p0004"}));
        assert_eq!(printable(&mut s), ["p0001", "p0002", "p0003", "p0005", "p0006", "p0007"]);
    }

    #[test]
    fn import_again_only_adds_new_paragraphs() {
        let mut s = imported();
        run(&mut s, "spec.activate", json!({"id": "p0002"}));
        let state = s.doc.custom_prop(STATE_PROP).map(str::to_string);
        let r = run(&mut s, "spec.import", json!({}));
        assert_eq!(r["imported"], 0);
        assert_eq!(r["items"], 12);
        assert_eq!(s.doc.custom_prop(STATE_PROP).map(str::to_string), state);
        assert_eq!(s.doc.control_ranges(wordcraft_doc::StoryRef::Body).len(), 12, "nothing wrapped twice");

        s.doc.body.push(para("PR1", "A new paragraph."));
        let r = run(&mut s, "spec.import", json!({}));
        assert_eq!(r["imported"], 1);
        assert_eq!(row(&mut s, "p0013")["text"], "A new paragraph.");
        assert_eq!(row(&mut s, "p0013")["on"], false);
    }

    #[test]
    fn ids_continue_after_the_highest() {
        let mut s = Session::new(doc(&SECTION[..3]));
        run(&mut s, "caret.set", json!({"pos": {"block": 1, "off": 0}}));
        run(&mut s, "spec.tag", json!({"id": "p0040"}));
        let r = run(&mut s, "spec.import", json!({}));
        assert_eq!(r["imported"], 2);
        let listed = run(&mut s, "spec.list", json!({}));
        let ids: Vec<&str> = listed["items"].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["p0041", "p0040", "p0042"]);
    }

    #[test]
    fn tables_and_empty_paragraphs_are_skipped() {
        let mut d = doc(&SECTION[..2]);
        d.body.insert(1, Arc::new(Block::Table(Table::new(2, 2, 100.0))));
        d.body.push(para("PR1", ""));
        let mut s = Session::new(d);
        let r = run(&mut s, "spec.import", json!({}));
        assert_eq!(r["imported"], 2);
        assert_eq!(r["skippedTableParagraphs"], 4);
        assert_eq!(r["skippedEmpty"], 1);
        assert_eq!(s.doc.plain_text(wordcraft_doc::StoryRef::Body).matches("Type here").count(), 0, "no placeholder text added");
    }

    #[test]
    fn state_survives_saving_to_docx_and_reopening() {
        let mut a = imported();
        run(&mut a, "spec.activate", json!({"ids": ["p0002", "p0003", "p0005", "p0007", "p0011"]}));
        let listed = run(&mut a, "spec.list", json!({}));
        let shown = printable(&mut a);

        let bytes = crate::io::save_bytes("x.docx", &a.doc).unwrap();
        let mut b = Session::new(crate::io::open_bytes("x.docx", &bytes).unwrap());
        assert_eq!(b.doc.custom_prop(STATE_PROP), a.doc.custom_prop(STATE_PROP));
        assert_eq!(styles(&b), styles(&a));
        let again = run(&mut b, "spec.list", json!({}));
        for key in ["id", "kind", "level", "on", "parent", "text"] {
            let pick = |l: &Value| l["items"].as_array().unwrap().iter().map(|i| i[key].clone()).collect::<Vec<_>>();
            assert_eq!(pick(&again), pick(&listed), "{key}");
        }
        assert_eq!(printable(&mut b), shown);
        assert_eq!(run(&mut b, "spec.import", json!({}))["imported"], 0, "reopened items are recognised");
    }

    #[test]
    fn printable_gives_text_in_order_and_tidies_hand_edited_state() {
        let mut s = imported();
        run(&mut s, "spec.activate", json!({"ids": ["p0002", "p0003", "p0005", "p0006"]}));
        let r = run(&mut s, "spec.printable", json!({}));
        assert_eq!(r["count"], 6);
        let texts: Vec<&str> = r["items"].as_array().unwrap().iter().map(|i| i["text"].as_str().unwrap()).collect();
        assert_eq!(
            texts,
            ["SECTION 09 2900 - GYPSUM BOARD", "GENERAL", "SUMMARY", "Section includes:", "Interior gypsum board.", "END OF SECTION 09 2900"]
        );

        // Unknown ids, blanks, repeats and a child whose parent is off are ignored, then tidied away.
        s.doc.set_custom_prop(STATE_PROP, " p0007 ,bogus,,p0001,p0001,p0012,p0012");
        assert_eq!(printable(&mut s), ["p0001", "p0012"]);
        run(&mut s, "spec.deactivate", json!({"id": "p0010"}));
        assert_eq!(s.doc.custom_prop(STATE_PROP), Some("p0001,p0012"));
        // Everything off: the property goes.
        run(&mut s, "spec.deactivate", json!({"ids": ["p0001", "p0012"]}));
        assert_eq!(s.doc.custom_prop(STATE_PROP), None);
    }

    #[test]
    fn bad_ids_and_hostile_params_change_nothing() {
        let mut s = imported();
        run(&mut s, "spec.activate", json!({"id": "p0002"}));
        let before = (s.doc.custom_prop(STATE_PROP).map(str::to_string), s.doc.plain_text(wordcraft_doc::StoryRef::Body));
        let long = "x".repeat(65);
        let many: Vec<String> = (0..20_000).map(|i| format!("p{i:04}")).collect();
        let bad = [
            json!(null),
            json!({}),
            json!({"id": ""}),
            json!({"id": "has space"}),
            json!({"id": long}),
            json!({"id": 5}),
            json!({"id": "nope"}),
            json!({"ids": "p0002"}),
            json!({"ids": []}),
            json!({"ids": [5]}),
            json!({"ids": ["p0003", null]}),
            json!({"ids": ["p0003", "nope"]}),
            json!({"ids": many}),
            json!({"id": "p0002", "ids": [{"id": "p0003"}]}),
        ];
        for cmd in ["spec.activate", "spec.deactivate"] {
            for v in &bad {
                assert!(s.run(cmd, v).is_err(), "{cmd} {v}");
                assert_eq!((s.doc.custom_prop(STATE_PROP).map(str::to_string), s.doc.plain_text(wordcraft_doc::StoryRef::Body)), before, "{cmd} {v}");
            }
        }
        // A huge, junk state property is read safely.
        s.doc.set_custom_prop(STATE_PROP, &"p0001,é,".repeat(200_000));
        assert_eq!(printable(&mut s), ["p0001"]);
        for v in &bad {
            let _ = s.run("spec.import", v);
            let _ = s.run("spec.printable", v);
        }
    }
}
