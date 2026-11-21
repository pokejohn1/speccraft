//! Global terms, section references and reference standards (milestone M20). Added by the
//! Speccraft fork; not part of the original WordCraft.
//!
//! - A **global term** is a project value (Project Name, Project ID, Rev No, Deliverable Date,
//!   …) shown in the text as it is now. `spec.terms.apply` turns every `<Insert Name>` with a
//!   value (and every fill-in made from one) into an inline control tagged `term:t0001` holding
//!   the value, and brings existing ones up to date, so the author sees and checks the real
//!   wording while editing. A term without a value goes back to the master's placeholder.
//! - A **section reference** ("Section 07 9200 "Joint Sealants"") is wrapped as `sref:r0001`; its
//!   quoted title follows the referenced section's title (`spec.refs.apply`).
//! - **Reference standards** (ASTM C1396/C1396M, NFPA 80, …) are recognised in the text and
//!   listed (`spec.standards.list`); the project keeps their titles and editions.
//!
//! The master's wording of each term and reference is kept in [`INLINE_PROP`], so a paragraph
//! whose only change is a term's value still counts as the master's wording.

use std::collections::{BTreeMap, HashMap};

use serde_json::{Value, json};
use wordcraft_doc::control::{ContentControl, ControlKind, ControlRange};
use wordcraft_doc::para::{InlineObject, OBJ};
use wordcraft_doc::{Document, Path, Pos, StoryRef};

use super::fields;
use super::sections::{self, Kind};
use crate::{CmdError, CmdResult, CommandSpec, Session};

const LOC: &str = "Specs › Fields";
pub const TERM_PREFIX: &str = "term:";
pub const REF_PREFIX: &str = "sref:";
/// Document property: the master's wording of each term and reference, `{"t0001": "<Insert
/// Project Name>", "r0001": "Section 07 9200"}`.
pub const INLINE_PROP: &str = "Speccraft.Inline";
/// Most terms and references changed in one command (bounded work).
const MAX_CHANGES: usize = 5_000;
/// Longest quoted section title looked for, in characters.
const MAX_TITLE: usize = 160;
/// Longest value written, in characters.
const MAX_VALUE: usize = 2_000;

/// Organisations whose standards are recognised.
const ORGS: [&str; 44] = [
    "AAMA", "AASHTO", "ACI", "ADA", "AISC", "AISI", "ANSI", "APA", "ASCE", "ASHRAE", "ASME", "ASTM", "AWI", "AWPA", "AWS", "AWWA", "BHMA", "CSA",
    "DHI", "EPA", "FM", "GA", "IAPMO", "ICC", "IEEE", "IBC", "ISO", "MPI", "NAAMM", "NEMA", "NFPA", "NFRC", "NRCA", "NSF", "SCAQMD", "SDI", "SJI",
    "SMACNA", "SSPC", "TCNA", "TIA", "UL", "WDMA", "WWPA",
];

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("spec.terms.apply", "Show Global Terms", LOC, terms_apply)
            .params(r#"{"values": {name: value} (the project's values; a term without one goes back to its placeholder)}"#),
        CommandSpec::new("spec.terms.list", "List Global Terms", LOC, terms_list).pure(),
        CommandSpec::new("spec.refs.apply", "Mark Section References", LOC, refs_apply)
            .params(r#"{"titles"?: {section number: title} (quoted titles follow these)}"#),
        CommandSpec::new("spec.refs.list", "List Section References", LOC, refs_list).pure(),
        CommandSpec::new("spec.standards.list", "List Reference Standards", LOC, standards_list).pure(),
    ]
}

/// What an inline Speccraft control is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlineKind {
    Term,
    Ref,
}

/// A global term or section reference in the document.
#[derive(Clone, Debug)]
pub struct Inline {
    pub id: String,
    pub kind: InlineKind,
    /// The term's name, or the reference's section number as written.
    pub name: String,
    pub value: String,
    pub original: String,
    pub range: ControlRange,
}

fn stored(doc: &Document) -> BTreeMap<String, String> {
    doc.custom_prop(INLINE_PROP).and_then(|v| serde_json::from_str(v).ok()).unwrap_or_default()
}

fn store(doc: &mut Document, m: &BTreeMap<String, String>) -> Result<(), CmdError> {
    if m.is_empty() {
        doc.remove_custom_prop(INLINE_PROP);
    } else {
        doc.set_custom_prop(INLINE_PROP, &serde_json::to_string(m).map_err(|e| CmdError::Failed(e.to_string()))?);
    }
    Ok(())
}

/// Every global term and section reference in the body, in document order.
pub fn inline(doc: &Document) -> Vec<Inline> {
    let saved = stored(doc);
    let mut ranges = doc.control_ranges(StoryRef::Body);
    ranges.sort_by(|a, b| a.start.cmp(&b.start));
    ranges
        .into_iter()
        .filter(|r| !r.control.block)
        .filter_map(|r| {
            let (kind, id) = if let Some(id) = r.control.tag.strip_prefix(TERM_PREFIX) {
                (InlineKind::Term, id)
            } else {
                (InlineKind::Ref, r.control.tag.strip_prefix(REF_PREFIX)?)
            };
            let id = id.to_string();
            let original = saved.get(&id).cloned().unwrap_or_else(|| r.control.placeholder_text.clone());
            let value = doc.control_text(&r).replace(OBJ, "");
            Some(Inline { id, kind, name: r.control.title.clone(), value, original, range: r })
        })
        .collect()
}

/// Each inline Speccraft control's range and master wording: fill-ins, terms and references.
/// The comparison with the master puts these back ([`super::edits::base_text`]).
pub fn originals_list(doc: &Document) -> Vec<(ControlRange, String)> {
    let mut out: Vec<(ControlRange, String)> = fields::fields(doc).into_iter().map(|f| (f.range, f.original)).collect();
    out.extend(inline(doc).into_iter().map(|x| (x.range, x.original)));
    out
}

/// The byte ranges of inline controls in top-level body paragraph `block`.
fn busy(doc: &Document, block: u32) -> Vec<(usize, usize)> {
    doc.control_ranges(StoryRef::Body)
        .into_iter()
        .filter(|r| !r.control.block && r.start.path.0.first() == Some(&block) && r.start.path == r.end.path)
        .map(|r| (r.start.off, r.end.off + OBJ.len_utf8()))
        .collect()
}

/// The next free number for ids starting with `prefix`.
fn next_id(saved: &BTreeMap<String, String>, doc: &Document, prefix: char) -> u32 {
    let in_doc = inline(doc).into_iter().map(|x| x.id).chain(saved.keys().cloned()).collect::<Vec<_>>();
    in_doc.iter().filter_map(|id| id.strip_prefix(prefix).and_then(|n| n.parse::<u32>().ok())).max().unwrap_or(0).saturating_add(1)
}

/// Wrap bytes `a..b` of top-level body paragraph `block` in an inline text control.
/// `label`: the control's tag, title and the master's wording.
fn wrap(doc: &mut Document, block: u32, span: std::ops::Range<usize>, label: (&str, &str, &str), control_id: i64) -> Result<(), CmdError> {
    let (a, b) = (span.start, span.end);
    let (tag, title, original) = label;
    let mut c = ContentControl::new(ControlKind::Text { multi_line: false });
    c.id = Some(control_id);
    c.tag = tag.to_string();
    c.title = title.to_string();
    c.placeholder_text = original.to_string();
    let props = doc.body.get(block as usize).and_then(|x| x.as_para()).map(|p| p.props_of_char(a).clone()).unwrap_or_default();
    let at = |off: usize| Pos::new(StoryRef::Body, Path(vec![block]), off);
    doc.insert_object(&at(b), InlineObject::ControlEnd, &props).map_err(|e| CmdError::Failed(e.to_string()))?;
    doc.insert_object(&at(a), InlineObject::ControlStart { control: Box::new(c) }, &props).map_err(|e| CmdError::Failed(e.to_string()))?;
    Ok(())
}

/// A fresh control id.
fn fresh_control_id(doc: &Document) -> i64 {
    let (ids, _) = fields::used_ids(doc);
    let mut id = 3_000_000 + ids.len() as i64;
    while ids.contains(&id) {
        id += 1;
    }
    id
}

// ---------------------------------------------------------------------------------------------
// Global terms

/// `<Insert Name>` in `t`: (byte range, the name as written).
fn placeholders(t: &str) -> Vec<(std::ops::Range<usize>, String)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(a) = t.get(from..).and_then(|r| r.find("<Insert ")).map(|k| k + from) {
        let Some(b) = t.get(a..).and_then(|r| r.find('>')).map(|k| k + a) else { break };
        let name = t.get(a + 8..b).unwrap_or("").trim();
        if !name.is_empty() && name.chars().count() <= 80 && !name.contains(['<', OBJ, '\n']) {
            out.push((a..b + 1, name.to_string()));
            from = b + 1;
        } else {
            from = a + 1;
        }
    }
    out
}

/// A term's name from its placeholder: `<Insert Project Name>` → "Project Name".
fn term_name(original: &str) -> Option<&str> {
    original.trim().strip_prefix("<Insert ").and_then(|x| x.strip_suffix('>')).map(str::trim).filter(|x| !x.is_empty())
}

fn values_param(v: &Value) -> Result<HashMap<String, String>, CmdError> {
    let m = v.get("values").and_then(Value::as_object).ok_or_else(|| CmdError::Params("`values` must be an object of name: value".into()))?;
    let mut out = HashMap::new();
    for (k, x) in m.iter().take(MAX_CHANGES) {
        let x = x.as_str().ok_or_else(|| CmdError::Params(format!("value {k:?} must be a string")))?;
        if x.chars().count() > MAX_VALUE || x.contains(['\n', '\r', OBJ]) {
            return Err(CmdError::Params(format!("value {k:?} must be one line of at most {MAX_VALUE} characters")));
        }
        out.insert(k.trim().to_lowercase(), x.trim().to_string());
    }
    Ok(out)
}

/// Show the project's values in the text (see the module docs).
pub(crate) fn terms_apply(s: &mut Session, v: &Value) -> CmdResult {
    let values = values_param(v)?;
    let value_of = |name: &str| values.get(&name.trim().to_lowercase()).filter(|x| !x.is_empty()).cloned();
    let mut saved = stored(&s.doc);
    let (mut updated, mut added) = (0usize, 0usize);

    // Existing terms: the value, or back to the placeholder. Back to front, so ranges hold.
    let mut terms: Vec<Inline> = inline(&s.doc).into_iter().filter(|x| x.kind == InlineKind::Term).collect();
    terms.reverse();
    for t in terms.iter().take(MAX_CHANGES) {
        let want = value_of(&t.name).unwrap_or_else(|| t.original.clone());
        if t.value != want {
            fields::replace_content(&mut s.doc, &t.range, &want)?;
            updated += 1;
        }
    }

    // Fill-ins made from a placeholder the project has a value for become terms.
    let mut field_originals: BTreeMap<String, String> =
        s.doc.custom_prop(fields::ORIGINALS_PROP).and_then(|x| serde_json::from_str(x).ok()).unwrap_or_default();
    let mut found: Vec<fields::Field> = fields::fields(&s.doc)
        .into_iter()
        .filter(|f| matches!(f.kind, fields::FieldKind::Text | fields::FieldKind::Date))
        .filter(|f| term_name(&f.original).and_then(value_of).is_some())
        .collect();
    found.reverse();
    let mut next = next_id(&saved, &s.doc, 't');
    for f in found.iter().take(MAX_CHANGES) {
        let Some(name) = term_name(&f.original).map(str::to_string) else { continue };
        let Some(value) = value_of(&name) else { continue };
        let id = format!("t{next:04}");
        next = next.saturating_add(1);
        if let Some(c) = s.doc.control_mut(&f.range.start) {
            c.tag = format!("{TERM_PREFIX}{id}");
            c.title = name;
            c.kind = ControlKind::Text { multi_line: false };
        }
        field_originals.remove(&f.id);
        saved.insert(id, f.original.clone());
        if f.value != value {
            fields::replace_content(&mut s.doc, &f.range, &value)?;
        }
        added += 1;
    }
    if !found.is_empty() {
        if field_originals.is_empty() {
            s.doc.remove_custom_prop(fields::ORIGINALS_PROP);
        } else {
            s.doc.set_custom_prop(fields::ORIGINALS_PROP, &serde_json::to_string(&field_originals).map_err(|e| CmdError::Failed(e.to_string()))?);
        }
    }

    // Placeholders still in the text with a value, outside any control.
    let entries = sections::outline(&s.doc);
    for e in entries.iter().filter(|e| e.kind != Kind::Note) {
        let Some(block) = e.range.start.path.0.first().copied() else { continue };
        let Some(text) = s.doc.body.get(block as usize).and_then(|b| b.as_para()).map(|p| p.text.clone()) else { continue };
        let taken = busy(&s.doc, block);
        let mut here: Vec<(std::ops::Range<usize>, String)> = placeholders(&text)
            .into_iter()
            .filter(|(r, n)| value_of(n).is_some() && !taken.iter().any(|(a, b)| r.start < *b && r.end > *a))
            .collect();
        here.reverse();
        for (r, name) in here {
            if added >= MAX_CHANGES {
                break;
            }
            let id = format!("t{next:04}");
            next = next.saturating_add(1);
            let original = text.get(r.clone()).unwrap_or("").to_string();
            let control_id = fresh_control_id(&s.doc);
            wrap(&mut s.doc, block, r.clone(), (&format!("{TERM_PREFIX}{id}"), &name, &original), control_id)?;
            saved.insert(id.clone(), original);
            if let Some(t) = inline(&s.doc).into_iter().find(|x| x.id == id)
                && let Some(value) = value_of(&name)
            {
                fields::replace_content(&mut s.doc, &t.range, &value)?;
            }
            added += 1;
        }
    }
    let present: std::collections::BTreeSet<String> = inline(&s.doc).into_iter().map(|x| x.id).collect();
    saved.retain(|k, _| present.contains(k));
    if saved != stored(&s.doc) {
        store(&mut s.doc, &saved)?;
    }
    Ok(json!({"updated": updated, "added": added, "terms": inline(&s.doc).iter().filter(|x| x.kind == InlineKind::Term).count()}))
}

fn terms_list(s: &mut Session, _v: &Value) -> CmdResult {
    let rows: Vec<Value> = inline(&s.doc)
        .into_iter()
        .filter(|x| x.kind == InlineKind::Term)
        .map(|x| json!({"id": x.id, "name": x.name, "value": x.value, "original": x.original, "block": x.range.start.path.0.first()}))
        .collect();
    Ok(json!({"count": rows.len(), "terms": rows}))
}

// ---------------------------------------------------------------------------------------------
// Section references

/// A section reference found in text: byte range, the number as written, and the quoted title's
/// byte range inside it if there is one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoundRef {
    pub range: std::ops::Range<usize>,
    pub number: String,
    pub title: Option<std::ops::Range<usize>>,
}

/// "Section 07 9200", "Section 079200", "Section 07 92 00", each optionally followed by a quoted
/// title, in `t`.
pub fn find_refs(t: &str) -> Vec<FoundRef> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(a) = t.get(from..).and_then(|r| r.find("Section ")).map(|k| k + from) {
        from = a + 8;
        let num_start = a + 8;
        let rest = t.get(num_start..).unwrap_or("");
        // Digits and the single spaces between them.
        let mut end = 0;
        let mut digits = 0;
        let chars: Vec<(usize, char)> = rest.char_indices().take(16).collect();
        for (k, (i, c)) in chars.iter().enumerate() {
            if c.is_ascii_digit() {
                digits += 1;
                end = i + 1;
            } else if (*c == ' ' || *c == '.') && chars.get(k + 1).is_some_and(|(_, n)| n.is_ascii_digit()) && digits > 0 {
                continue;
            } else {
                break;
            }
        }
        if !(digits == 6 || digits == 8) {
            continue;
        }
        let number = rest.get(..end).unwrap_or("").to_string();
        let mut stop = num_start + end;
        let mut title = None;
        let after = t.get(stop..).unwrap_or("");
        if let Some(q) = ["\u{20}\"", " \u{201c}"].iter().find(|q| after.starts_with(**q)) {
            let open = stop + q.len();
            let close_ch = if q.ends_with('"') { '"' } else { '\u{201d}' };
            if let Some(k) = t.get(open..).and_then(|r| r.char_indices().take(MAX_TITLE).find(|(_, c)| *c == close_ch || *c == OBJ))
                && k.1 == close_ch
            {
                title = Some(open..open + k.0);
                stop = open + k.0 + close_ch.len_utf8();
            }
        }
        out.push(FoundRef { range: a..stop, number, title });
        from = stop;
    }
    out
}

fn titles_param(v: &Value) -> Result<HashMap<String, String>, CmdError> {
    let Some(m) = v.get("titles") else { return Ok(HashMap::new()) };
    let m = m.as_object().ok_or_else(|| CmdError::Params("`titles` must be an object of section number: title".into()))?;
    let mut out = HashMap::new();
    for (k, x) in m.iter().take(MAX_CHANGES) {
        let x = x.as_str().ok_or_else(|| CmdError::Params(format!("title of {k:?} must be a string")))?;
        if x.chars().count() > MAX_TITLE || x.contains(['\n', '\r', OBJ, '"', '\u{201d}']) {
            return Err(CmdError::Params(format!("title of {k:?} must be one line of at most {MAX_TITLE} characters, without quotes")));
        }
        let key = super::project::number_key(k);
        if !key.is_empty() {
            out.insert(key, x.trim().to_string());
        }
    }
    Ok(out)
}

/// Wrap section references and bring their quoted titles up to date.
pub(crate) fn refs_apply(s: &mut Session, v: &Value) -> CmdResult {
    let titles = titles_param(v)?;
    let mut saved = stored(&s.doc);
    let (mut updated, mut added) = (0usize, 0usize);
    // Existing references: the title the project has for them.
    let mut refs: Vec<Inline> = inline(&s.doc).into_iter().filter(|x| x.kind == InlineKind::Ref).collect();
    refs.reverse();
    for r in refs.iter().take(MAX_CHANGES) {
        let Some(title) = titles.get(&super::project::number_key(&r.name)) else { continue };
        let Some(found) = find_refs(&r.value).into_iter().next() else { continue };
        let Some(t) = found.title.clone() else { continue };
        if r.value.get(t.clone()) == Some(title.as_str()) {
            continue;
        }
        let want = format!("{}{title}{}", r.value.get(..t.start).unwrap_or(""), r.value.get(t.end..).unwrap_or(""));
        fields::replace_content(&mut s.doc, &r.range, &want)?;
        updated += 1;
    }
    // New ones, outside any control, in section paragraphs (not notes, the title or the end line).
    let mut next = next_id(&saved, &s.doc, 'r');
    let entries = sections::outline(&s.doc);
    for e in entries.iter().filter(|e| matches!(e.kind, Kind::Article | Kind::Para | Kind::Other | Kind::Part)) {
        let Some(block) = e.range.start.path.0.first().copied() else { continue };
        let Some(text) = s.doc.body.get(block as usize).and_then(|b| b.as_para()).map(|p| p.text.clone()) else { continue };
        let taken = busy(&s.doc, block);
        let mut here: Vec<FoundRef> =
            find_refs(&text).into_iter().filter(|f| !taken.iter().any(|(a, b)| f.range.start < *b && f.range.end > *a)).collect();
        here.reverse();
        for f in here {
            if added >= MAX_CHANGES {
                break;
            }
            let id = format!("r{next:04}");
            next = next.saturating_add(1);
            let original = text.get(f.range.clone()).unwrap_or("").to_string();
            let control_id = fresh_control_id(&s.doc);
            wrap(&mut s.doc, block, f.range.clone(), (&format!("{REF_PREFIX}{id}"), &f.number, &original), control_id)?;
            saved.insert(id.clone(), original.clone());
            added += 1;
            // Its title, if the project has another one.
            if let (Some(t), Some(title)) = (f.title.clone(), titles.get(&super::project::number_key(&f.number)))
                && text.get(t.clone()) != Some(title.as_str())
                && let Some(x) = inline(&s.doc).into_iter().find(|x| x.id == id)
            {
                let rel = (t.start - f.range.start)..(t.end - f.range.start);
                let want = format!("{}{title}{}", original.get(..rel.start).unwrap_or(""), original.get(rel.end..).unwrap_or(""));
                fields::replace_content(&mut s.doc, &x.range, &want)?;
                updated += 1;
            }
        }
    }
    if saved != stored(&s.doc) {
        store(&mut s.doc, &saved)?;
    }
    Ok(json!({"updated": updated, "added": added, "refs": inline(&s.doc).iter().filter(|x| x.kind == InlineKind::Ref).count()}))
}

fn refs_list(s: &mut Session, _v: &Value) -> CmdResult {
    let entries = sections::outline(&s.doc);
    let prints = |block: Option<&u32>| entries.iter().any(|e| e.on && e.range.start.path.0.first() == block);
    let rows: Vec<Value> = inline(&s.doc)
        .into_iter()
        .filter(|x| x.kind == InlineKind::Ref)
        .map(|x| {
            let b = x.range.start.path.0.first();
            json!({"id": x.id, "number": x.name, "key": super::project::number_key(&x.name), "text": x.value, "block": b, "prints": prints(b)})
        })
        .collect();
    Ok(json!({"count": rows.len(), "refs": rows}))
}

// ---------------------------------------------------------------------------------------------
// Reference standards

/// A standard cited in text: byte range, organisation, and designation ("C1396/C1396M").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Standard {
    pub range: std::ops::Range<usize>,
    pub org: String,
    pub designation: String,
}

impl Standard {
    /// "ASTM C1396/C1396M": the key the project's list uses.
    pub fn key(&self) -> String {
        format!("{} {}", self.org, self.designation)
    }
}

/// Whether `w` reads as (part of) a designation: an optional letter prefix, then a digit.
fn designation_part(w: &str) -> bool {
    let w = w.trim_end_matches([',', ';', ':', '.', ')']);
    let letters = w.chars().take_while(char::is_ascii_uppercase).count();
    letters <= 3
        && w.chars().nth(letters).is_some_and(|c| c.is_ascii_digit())
        && w.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '-'))
}

/// The reference standards cited in `t`.
pub fn find_standards(t: &str) -> Vec<Standard> {
    let mut out = Vec::new();
    for (i, _) in t.char_indices() {
        let before = t.get(..i).and_then(|b| b.chars().next_back());
        if before.is_some_and(char::is_alphanumeric) {
            continue;
        }
        let Some(org) = ORGS.iter().find(|o| t.get(i..).is_some_and(|r| r.starts_with(**o) && r.get(o.len()..).is_some_and(|x| x.starts_with(' '))))
        else {
            continue;
        };
        let mut at = i + org.len() + 1;
        let mut parts: Vec<String> = Vec::new();
        let mut end = at;
        // Up to three words: "C 1396/C 1396M", "A653/A653M", "80".
        for _ in 0..3 {
            let rest = t.get(at..).unwrap_or("");
            let word: String = rest.chars().take_while(|c| !c.is_whitespace() && *c != OBJ).collect();
            if word.is_empty() {
                break;
            }
            let single_letter = word.len() == 1 && word.chars().all(|c| c.is_ascii_uppercase());
            let next_is_number =
                rest.get(word.len()..).and_then(|r| r.strip_prefix(' ')).is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit()));
            if designation_part(&word) || (single_letter && next_is_number) || (!parts.is_empty() && word.starts_with('/')) {
                let clean = word.trim_end_matches([',', ';', ':', '.', ')']).to_string();
                end = at + clean.len();
                parts.push(clean);
                at += word.len() + 1;
                // A designation ends at punctuation or after a whole word, unless the next word goes on with it.
                if word.ends_with([',', ';', ':', '.', ')']) {
                    break;
                }
                let next: String = t.get(at..).unwrap_or("").chars().take_while(|c| !c.is_whitespace()).collect();
                // "C 1396" and "C 1396/C 1396M": a lone letter goes on with the number after it.
                let lone_letter = |p: &String| p.rsplit('/').next().is_some_and(|x| x.len() == 1 && x.chars().all(|c| c.is_ascii_uppercase()));
                let continues = next.starts_with('/') || (parts.last().is_some_and(lone_letter) && designation_part(&next));
                if !continues && !single_letter {
                    break;
                }
            } else {
                break;
            }
        }
        if parts.is_empty() || !parts.iter().any(|p| p.chars().any(|c| c.is_ascii_digit())) {
            continue;
        }
        let designation: String = parts.concat();
        out.push(Standard { range: i..end, org: (*org).to_string(), designation });
    }
    out
}

fn standards_list(s: &mut Session, _v: &Value) -> CmdResult {
    let entries = sections::outline(&s.doc);
    let mut by_key: BTreeMap<String, (String, String, Vec<String>, bool)> = BTreeMap::new();
    for e in entries.iter().filter(|e| e.kind != Kind::Note) {
        let text = s.doc.control_text(&e.range).replace(OBJ, "");
        for st in find_standards(&text) {
            let row = by_key.entry(st.key()).or_insert_with(|| (st.org.clone(), st.designation.clone(), Vec::new(), false));
            if !row.2.contains(&e.id) {
                row.2.push(e.id.clone());
            }
            row.3 |= e.on;
        }
    }
    let rows: Vec<Value> = by_key
        .into_iter()
        .map(|(key, (org, designation, paras, prints))| json!({"key": key, "org": org, "designation": designation, "paragraphs": paras, "prints": prints}))
        .collect();
    Ok(json!({"count": rows.len(), "standards": rows}))
}

/// `spec.standards.list` for one document, as plain rows (for the project report).
pub fn standards_in(doc: &Document) -> Vec<(String, bool)> {
    let mut s = Session::new(doc.clone());
    standards_list(&mut s, &json!({}))
        .ok()
        .and_then(|v| v.get("standards").and_then(Value::as_array).cloned())
        .unwrap_or_default()
        .iter()
        .map(|r| (r["key"].as_str().unwrap_or("").to_string(), r["prints"].as_bool().unwrap_or(false)))
        .collect()
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
            para("PR1", "Work for <Insert Project Name>, project <Insert Project ID>, by <Insert Contractor>."),
            para("PR1", "Section 07 9200 \"Sealants\" for joints; Section 09 9123 for painting."),
            para("PR1", "Panels: ASTM C 1396/C 1396M, Type X; frames to NFPA 80 and UL 10C; steel ASTM A653/A653M, G60."),
            para("EOS", "END OF SECTION 09 2900"),
        ];
        let mut s = Session::new(d);
        run(&mut s, "spec.import", json!({}));
        s
    }

    fn text(s: &Session, block: usize) -> String {
        s.doc.body[block].as_para().unwrap().plain_text().replace(OBJ, "")
    }

    #[test]
    fn terms_show_the_project_values_and_follow_them() {
        let mut s = section();
        // One placeholder was already a fill-in.
        run(&mut s, "spec.fields.detect", json!({}));
        let r = run(&mut s, "spec.terms.apply", json!({"values": {"Project Name": "Riverside Library", "project id": "LANL-0042"}}));
        assert_eq!(r["added"], 2, "{r}");
        assert_eq!(text(&s, 3), "Work for Riverside Library, project LANL-0042, by <Insert Contractor>.");
        // The paragraph still reads as the master's wording.
        let o = run(&mut s, "spec.origin", json!({}));
        assert_eq!(o["counts"], json!({"master": 7}), "{o}");
        // A changed value follows; a removed one goes back to the placeholder.
        let r = run(&mut s, "spec.terms.apply", json!({"values": {"Project Name": "Riverside Branch Library"}}));
        assert_eq!(r["updated"], 2);
        assert_eq!(text(&s, 3), "Work for Riverside Branch Library, project <Insert Project ID>, by <Insert Contractor>.");
        // Nothing to do the second time: no change, no undo step.
        let rev = s.rev();
        run(&mut s, "spec.terms.apply", json!({"values": {"Project Name": "Riverside Branch Library"}}));
        assert_eq!(s.rev(), rev);
        let list = run(&mut s, "spec.terms.list", json!({}));
        assert_eq!(list["count"], 2);
        assert_eq!(list["terms"][0]["name"], "Project Name");
        // Saved and opened again, still terms with their wording.
        let bytes = crate::io::save_bytes("x.docx", &s.doc).unwrap();
        let mut b = Session::new(crate::io::open_bytes("x.docx", &bytes).unwrap());
        assert_eq!(run(&mut b, "spec.terms.list", json!({}))["terms"][0]["original"], "<Insert Project Name>");
        // The issued Word copy has no controls left.
        let mut copy = s.doc.clone();
        super::super::output::strip_markings(&mut copy);
        assert!(copy.control_ranges(StoryRef::Body).is_empty());
    }

    #[test]
    fn section_references_are_marked_and_their_titles_follow_the_project() {
        let mut s = section();
        let r = run(&mut s, "spec.refs.apply", json!({"titles": {"079200": "Joint Sealants", "09 9123": "Interior Painting"}}));
        assert_eq!((r["added"].clone(), r["updated"].clone()), (json!(2), json!(1)), "{r}");
        assert_eq!(text(&s, 4), "Section 07 9200 \"Joint Sealants\" for joints; Section 09 9123 for painting.");
        let list = run(&mut s, "spec.refs.list", json!({}));
        assert_eq!(list["refs"][1]["key"], "099123");
        assert_eq!(run(&mut s, "spec.origin", json!({}))["counts"], json!({"master": 7}));
        // Again: nothing to do.
        let r = run(&mut s, "spec.refs.apply", json!({"titles": {"079200": "Joint Sealants"}}));
        assert_eq!((r["added"].clone(), r["updated"].clone()), (json!(0), json!(0)));
        assert!(s.run("spec.refs.apply", &json!({"titles": {"079200": "Bad \" title"}})).is_err());
    }

    #[test]
    fn finding_references() {
        let f = find_refs("See Section 079200, Section 07 92 00 \u{201c}Joint Sealants\u{201d}, and Section 1 and Section 09 2900.13.");
        let nums: Vec<&str> = f.iter().map(|x| x.number.as_str()).collect();
        assert_eq!(nums, ["079200", "07 92 00", "09 2900.13"]);
        assert!(f[1].title.is_some());
    }

    #[test]
    fn reference_standards_are_recognised() {
        let t = "Panels: ASTM C 1396/C 1396M, Type X; frames to NFPA 80 and UL 10C; steel ASTM A653/A653M, G60; ASTMX 1; per ADA.";
        let keys: Vec<String> = find_standards(t).iter().map(Standard::key).collect();
        assert_eq!(keys, ["ASTM C1396/C1396M", "NFPA 80", "UL 10C", "ASTM A653/A653M"]);
        let mut s = section();
        let r = run(&mut s, "spec.standards.list", json!({}));
        assert_eq!(r["count"], 4, "{r}");
        assert_eq!(r["standards"][0]["paragraphs"], json!(["p0006"]));
    }
}
