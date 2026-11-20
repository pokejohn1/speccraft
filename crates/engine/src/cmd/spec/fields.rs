//! Specs › Fields: fill-ins and choices inside spec paragraphs (milestone M7). Added by the
//! Speccraft fork; not part of the original WordCraft.
//!
//! [`detect`] finds what a master leaves open and wraps each in an inline content control
//! tagged `field:<id>`:
//!
//! | In the master | Field | Control |
//! |---|---|---|
//! | `[a] [b] [c]` (two or more adjacent brackets) | choice | combo box listing a, b, c |
//! | `[text]` (one bracket) | optional text | combo box: the text, or *(leave out)* |
//! | `___` (three or more underscores) | blank | plain text |
//! | `<Insert name>` | named fill-in (a date when the name says date) | plain text or date |
//!
//! Each control keeps the master's original wording as its placeholder text, so a field can go
//! back to it and the master text stays identifiable. The control's content is the value: it shows
//! on the page and prints. A field whose content is still the original is *open*. Choosing works
//! with WordCraft's own content control list on the page (`developer.chooseItem`) or
//! `spec.fields.set`; Word opens the file with ordinary content controls.
//!
//! When the section publishes ([`resolve_for_print`]), left-out text is removed with the space
//! before it, and open fields print as written and are reported.

use std::collections::{BTreeMap, HashMap};

use serde_json::{Value, json};
use wordcraft_doc::control::{ContentControl, ControlKind, ControlRange, ListItem};
use wordcraft_doc::para::{InlineObject, OBJ};
use wordcraft_doc::{Document, Path, Pos, StoryRef};

use super::sections::{self, Kind};
use crate::{CmdError, CmdResult, CommandSpec, Session, p};

const LOC: &str = "Specs › Fields";
/// The start of a field control's tag.
pub const FIELD_PREFIX: &str = "field:";
/// Document property: each field's original master wording, as JSON `{"f0001": "[a] [b]", …}`.
/// (A Word file keeps a control's placeholder text only through a building block, so the control
/// alone would lose it when saved.)
pub const ORIGINALS_PROP: &str = "Speccraft.Fields";
/// The choice that leaves optional text out.
pub const OMIT: &str = "(leave out)";
/// Longest option or fill-in name kept, in characters.
const MAX_OPTION: usize = 300;
/// Most options in one choice.
const MAX_OPTIONS: usize = 20;
/// Most fields in one section.
const MAX_FIELDS: usize = 5_000;
/// Longest value `spec.fields.set` writes, in characters.
const MAX_VALUE: usize = 2_000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("spec.fields.detect", "Find Fill-ins", LOC, detect_cmd),
        CommandSpec::new("spec.fields.list", "List Fill-ins", LOC, list_cmd).pure(),
        CommandSpec::new("spec.fields.set", "Fill In", LOC, set_cmd).params(
            r#"{"id": string, "value"?: string or null (null: back to the master's wording), "option"?: n (index into the options), "omit"?: true (optional text only), "reset"?: true}"#,
        ),
    ]
}

/// What a field is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Choice,
    Optional,
    Text,
    Date,
}

impl FieldKind {
    pub fn name(self) -> &'static str {
        match self {
            FieldKind::Choice => "choice",
            FieldKind::Optional => "optional",
            FieldKind::Text => "text",
            FieldKind::Date => "date",
        }
    }
}

/// Whether a field has been dealt with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldState {
    /// Still the master's wording.
    Open,
    Set,
    /// Optional text left out.
    Omitted,
}

impl FieldState {
    pub fn name(self) -> &'static str {
        match self {
            FieldState::Open => "open",
            FieldState::Set => "set",
            FieldState::Omitted => "omitted",
        }
    }
}

/// A field in the document.
#[derive(Clone, Debug)]
pub struct Field {
    pub id: String,
    pub kind: FieldKind,
    /// The control's title: "Choice", "Optional text", "Blank" or the fill-in's name.
    pub name: String,
    pub options: Vec<String>,
    /// The master's wording.
    pub original: String,
    /// What the field holds now.
    pub value: String,
    pub state: FieldState,
    pub range: ControlRange,
}

/// The originals property, read.
fn originals(doc: &Document) -> BTreeMap<String, String> {
    doc.custom_prop(ORIGINALS_PROP).and_then(|v| serde_json::from_str(v).ok()).unwrap_or_default()
}

/// Every field control in the body, in document order.
pub fn fields(doc: &Document) -> Vec<Field> {
    let saved = originals(doc);
    let mut ranges = doc.control_ranges(StoryRef::Body);
    ranges.sort_by(|a, b| a.start.cmp(&b.start));
    let mut out = Vec::new();
    for r in ranges {
        let Some(id) = r.control.tag.strip_prefix(FIELD_PREFIX).map(str::to_string) else { continue };
        if r.control.block || out.len() >= MAX_FIELDS {
            continue;
        }
        let items: Vec<String> = r.control.kind.items().map(|i| i.iter().map(|x| x.display.clone()).collect()).unwrap_or_default();
        let kind = match &r.control.kind {
            ControlKind::ComboBox { .. } | ControlKind::DropDown { .. } if items.last().is_some_and(|x| x == OMIT) => FieldKind::Optional,
            ControlKind::ComboBox { .. } | ControlKind::DropDown { .. } => FieldKind::Choice,
            ControlKind::Date { .. } => FieldKind::Date,
            _ => FieldKind::Text,
        };
        let options: Vec<String> = items.into_iter().filter(|x| x != OMIT).collect();
        let value = doc.control_text(&r).replace(OBJ, "");
        let original = saved.get(&id).cloned().unwrap_or_else(|| r.control.placeholder_text.clone());
        let state = if kind == FieldKind::Optional && value == OMIT {
            FieldState::Omitted
        } else if r.control.showing_placeholder || value.trim().is_empty() || value.trim() == original.trim() {
            FieldState::Open
        } else {
            FieldState::Set
        };
        out.push(Field { id, kind, name: r.control.title.clone(), options, original, value, state, range: r });
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Finding fields in text

/// A field found in a paragraph's text: byte range, kind, options, name.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Found {
    start: usize,
    end: usize,
    kind: FieldKind,
    options: Vec<String>,
    name: String,
}

/// The closing `]` for a `[` at byte `i` of `t`, on the same line, with no `[` or object between.
fn close_bracket(t: &str, i: usize) -> Option<usize> {
    let rest = t.get(i + 1..)?;
    for (k, c) in rest.char_indices() {
        match c {
            ']' => return Some(i + 1 + k),
            '[' | '\n' => return None,
            c if c == OBJ => return None,
            _ => {}
        }
    }
    None
}

/// Fields in `t`, in order, not overlapping.
fn scan(t: &str) -> Vec<Found> {
    let mut out: Vec<Found> = Vec::new();
    // Brackets: adjacent groups make one choice.
    let mut i = 0;
    while let Some(a) = t.get(i..).and_then(|r| r.find('[')).map(|k| k + i) {
        let Some(mut b) = close_bracket(t, a) else {
            i = a + 1;
            continue;
        };
        let mut options = vec![t.get(a + 1..b).unwrap_or("").trim().to_string()];
        loop {
            let after = t.get(b + 1..).unwrap_or("");
            let skip = after.len() - after.trim_start_matches(' ').len();
            let next = b + 1 + skip;
            if t.get(next..).is_some_and(|r| r.starts_with('['))
                && let Some(c) = close_bracket(t, next)
            {
                options.push(t.get(next + 1..c).unwrap_or("").trim().to_string());
                b = c;
                continue;
            }
            break;
        }
        let ok = options.iter().all(|o| !o.is_empty() && o.chars().count() <= MAX_OPTION) && options.len() <= MAX_OPTIONS;
        if ok {
            let kind = if options.len() > 1 { FieldKind::Choice } else { FieldKind::Optional };
            let name = if kind == FieldKind::Choice { "Choice" } else { "Optional text" };
            out.push(Found { start: a, end: b + 1, kind, options, name: name.into() });
        }
        i = b + 1;
    }
    let covered = |at: usize, out: &[Found]| out.iter().any(|f| at >= f.start && at < f.end);
    // Blanks: three or more underscores.
    let mut extra = Vec::new();
    let mut i = 0;
    while let Some(a) = t.get(i..).and_then(|r| r.find("___")).map(|k| k + i) {
        let len = t.get(a..).unwrap_or("").chars().take_while(|c| *c == '_').count();
        if !covered(a, &out) {
            extra.push(Found { start: a, end: a + len, kind: FieldKind::Text, options: Vec::new(), name: "Blank".into() });
        }
        i = a + len;
    }
    // <Insert name>.
    let mut i = 0;
    while let Some(a) = t.get(i..).and_then(|r| r.find("<Insert ")).map(|k| k + i) {
        let Some(b) = t.get(a..).and_then(|r| r.find('>')).map(|k| k + a) else { break };
        let name = t.get(a + 8..b).unwrap_or("").trim().to_string();
        let fits = !name.is_empty() && name.chars().count() <= 80 && !name.contains(['<', '[', ']', OBJ, '\n']);
        if fits && !covered(a, &out) {
            let kind = if name.to_lowercase().split(|c: char| !c.is_alphanumeric()).any(|w| w == "date") { FieldKind::Date } else { FieldKind::Text };
            extra.push(Found { start: a, end: b + 1, kind, options: Vec::new(), name });
            i = b + 1;
        } else {
            i = a + 1;
        }
    }
    out.extend(extra);
    out.sort_by_key(|f| f.start);
    out
}

/// The control for a found field; `original` is the master's wording.
fn control(f: &Found, original: &str, id: &str, control_id: i64) -> ContentControl {
    let item = |s: &str| ListItem { display: s.to_string(), value: String::new() };
    let kind = match f.kind {
        FieldKind::Choice => ControlKind::ComboBox { items: f.options.iter().map(|o| item(o)).collect(), last_value: String::new() },
        FieldKind::Optional => {
            ControlKind::ComboBox { items: f.options.iter().map(|o| item(o)).chain([item(OMIT)]).collect(), last_value: String::new() }
        }
        FieldKind::Date => ControlKind::Date {
            full_date: String::new(),
            format: "MMMM d, yyyy".into(),
            lid: "en-US".into(),
            calendar: "gregorian".into(),
            store_as: "dateTime".into(),
        },
        FieldKind::Text => ControlKind::Text { multi_line: false },
    };
    let mut c = ContentControl::new(kind);
    c.id = Some(control_id);
    c.tag = format!("{FIELD_PREFIX}{id}");
    c.title = f.name.clone();
    c.placeholder_text = original.to_string();
    c
}

/// Ids of every content control, and the highest field number in use.
fn used_ids(doc: &Document) -> (std::collections::BTreeSet<i64>, u32) {
    let mut ids = std::collections::BTreeSet::new();
    let mut add = |bl: &wordcraft_doc::Blocks| {
        wordcraft_doc::control::walk_controls(bl, &mut |c| {
            if let Some(id) = c.id {
                ids.insert(id);
            }
        })
    };
    add(&doc.body);
    for p in doc.parts.values() {
        add(&p.blocks);
    }
    let top = fields(doc).iter().filter_map(|f| f.id.strip_prefix('f').and_then(|n| n.parse::<u32>().ok())).max().unwrap_or(0);
    (ids, top)
}

/// Wrap what the section's paragraphs leave open in field controls. Notes are skipped, and so is
/// anything already inside a field. Returns how many were added, by kind.
pub fn detect(doc: &mut Document) -> Result<BTreeMap<&'static str, usize>, CmdError> {
    let entries = sections::outline(doc);
    if entries.is_empty() {
        return Err(CmdError::Failed("this document has no spec paragraphs yet: run spec.import first".into()));
    }
    let (mut ids, mut next) = used_ids(doc);
    let mut control_id = 2_000_000 + ids.len() as i64;
    // Byte ranges already inside an inline control, by top-level block.
    let mut inside: HashMap<u32, Vec<(usize, usize)>> = HashMap::new();
    for r in doc.control_ranges(StoryRef::Body).into_iter().filter(|r| !r.control.block && r.start.path == r.end.path) {
        if let Some(b) = r.start.path.0.first() {
            inside.entry(*b).or_default().push((r.start.off, r.end.off));
        }
    }
    let mut added: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut saved = originals(doc);
    let mut total = fields(doc).len();
    for e in entries.iter().filter(|e| e.kind != Kind::Note) {
        let Some(block) = e.range.start.path.0.first().copied() else { continue };
        let Some(para) = doc.body.get(block as usize).and_then(|b| b.as_para()) else { continue };
        let text = para.text.clone();
        let busy = inside.get(&block).cloned().unwrap_or_default();
        let found: Vec<Found> = scan(&text).into_iter().filter(|f| !busy.iter().any(|(a, b)| f.start < *b && f.end > *a)).collect();
        // Numbered in reading order, inserted back to front so earlier offsets stay right.
        let room = MAX_FIELDS.saturating_sub(total);
        let numbered: Vec<(u32, &Found)> = found
            .iter()
            .take(room)
            .map(|f| {
                next += 1;
                (next, f)
            })
            .collect();
        for (n, f) in numbered.into_iter().rev() {
            let original = text.get(f.start..f.end).unwrap_or("").to_string();
            while ids.contains(&control_id) {
                control_id += 1;
            }
            ids.insert(control_id);
            let fid = format!("f{n:04}");
            let ctl = control(f, &original, &fid, control_id);
            saved.insert(fid, original.clone());
            let props = doc.body.get(block as usize).and_then(|b| b.as_para()).map(|p| p.props_of_char(f.start).clone()).unwrap_or_default();
            let at = |off: usize| Pos::new(StoryRef::Body, Path(vec![block]), off);
            doc.insert_object(&at(f.end), InlineObject::ControlEnd, &props).map_err(|e| CmdError::Failed(e.to_string()))?;
            doc.insert_object(&at(f.start), InlineObject::ControlStart { control: Box::new(ctl) }, &props)
                .map_err(|e| CmdError::Failed(e.to_string()))?;
            *added.entry(f.kind.name()).or_default() += 1;
            total += 1;
        }
    }
    if !added.is_empty() {
        // Only the fields still in the document.
        let present: std::collections::BTreeSet<String> = fields(doc).into_iter().map(|f| f.id).collect();
        saved.retain(|k, _| present.contains(k));
        doc.set_custom_prop(ORIGINALS_PROP, &serde_json::to_string(&saved).unwrap_or_default());
    }
    Ok(added)
}

/// Replace a field's content with `text`, in the formatting of its first character.
fn set_content(doc: &mut Document, f: &Field, text: &str) -> Result<(), CmdError> {
    let a = f.range.content_start();
    let props = doc.para_at(&a).map(|p| p.props_of_char(a.off).clone()).unwrap_or_default();
    if f.range.end.off > a.off {
        doc.delete_range(&a, &f.range.end).map_err(|e| CmdError::Failed(e.to_string()))?;
    }
    if !text.is_empty() {
        doc.insert_text(&a, text, &props).map_err(|e| CmdError::Failed(e.to_string()))?;
    }
    if let Some(c) = doc.control_mut(&f.range.start) {
        c.showing_placeholder = false;
        if let ControlKind::ComboBox { last_value, .. } | ControlKind::DropDown { last_value, .. } = &mut c.kind {
            *last_value = String::new();
        }
    }
    Ok(())
}

/// Publish-time: remove left-out text (and the space before it, when a space or punctuation
/// follows), and list the fields still open, as `(name, original)`.
pub fn resolve_for_print(doc: &mut Document) -> Vec<(String, String)> {
    let all = fields(doc);
    let open: Vec<(String, String)> = all.iter().filter(|f| f.state == FieldState::Open).map(|f| (f.name.clone(), f.original.clone())).collect();
    for f in all.iter().rev().filter(|f| f.state == FieldState::Omitted) {
        if set_content(doc, f, "").is_err() {
            continue;
        }
        let start = &f.range.start;
        let Some(para) = doc.para_at(start) else { continue };
        let w = OBJ.len_utf8();
        let before = para.text.get(..start.off).and_then(|t| t.chars().next_back());
        let after = para.text.get(start.off + 2 * w..).and_then(|t| t.chars().next());
        if before == Some(' ') && after.is_none_or(|c| matches!(c, ' ' | ',' | '.' | ';' | ':' | ')')) {
            let a = Pos { off: start.off - 1, ..start.clone() };
            let _ = doc.delete_range(&a, start);
        }
    }
    open
}

// ---------------------------------------------------------------------------------------------
// Commands

fn detect_cmd(s: &mut Session, _v: &Value) -> CmdResult {
    let added = detect(&mut s.doc)?;
    let n: usize = added.values().sum();
    Ok(json!({"added": n, "byKind": added, "fields": fields(&s.doc).len()}))
}

/// One field as `spec.fields.list` gives it, with the spec paragraph it is in and whether that prints.
fn field_json(f: &Field, para: Option<&sections::Entry>) -> Value {
    json!({
        "id": f.id,
        "kind": f.kind.name(),
        "name": f.name,
        "options": f.options,
        "original": f.original,
        "value": f.value,
        "state": f.state.name(),
        "paragraph": para.map(|e| e.id.clone()),
        "prints": para.is_some_and(|e| e.on),
        "block": f.range.start.path.0.first(),
    })
}

fn list_cmd(s: &mut Session, _v: &Value) -> CmdResult {
    let entries = sections::outline(&s.doc);
    let by_block: HashMap<u32, &sections::Entry> = entries.iter().filter_map(|e| e.range.start.path.0.first().map(|b| (*b, e))).collect();
    let all = fields(&s.doc);
    let items: Vec<Value> = all.iter().map(|f| field_json(f, f.range.start.path.0.first().and_then(|b| by_block.get(b).copied()))).collect();
    let open_printing = items.iter().filter(|i| i["state"] == "open" && i["prints"] == true).count();
    Ok(json!({"fields": items, "count": items.len(), "openPrinting": open_printing}))
}

fn set_cmd(s: &mut Session, v: &Value) -> CmdResult {
    let id = p::req_str(v, "id")?.trim().to_string();
    let all = fields(&s.doc);
    let f = all.iter().find(|f| f.id == id).ok_or_else(|| CmdError::Failed(format!("no fill-in called {id:?}")))?;
    let text = if p::bool(v, "reset") == Some(true) || v.get("value").is_some_and(Value::is_null) {
        f.original.clone()
    } else if p::bool(v, "omit") == Some(true) {
        if f.kind != FieldKind::Optional {
            return Err(CmdError::Params("only optional text can be left out".into()));
        }
        OMIT.to_string()
    } else if let Some(i) = p::u64(v, "option") {
        f.options.get(usize::try_from(i).unwrap_or(usize::MAX)).cloned().ok_or_else(|| CmdError::Params(format!("option {i} doesn't exist")))?
    } else if let Some(t) = p::str(v, "value") {
        if t.chars().count() > MAX_VALUE || t.contains(['\n', '\r', OBJ]) {
            return Err(CmdError::Params(format!("`value` must be one line of at most {MAX_VALUE} characters")));
        }
        t.to_string()
    } else {
        return Err(CmdError::Params("give `value`, `option`, `omit` or `reset`".into()));
    };
    set_content(&mut s.doc, f, &text)?;
    let after = fields(&s.doc).into_iter().find(|x| x.id == id);
    Ok(json!({"id": id, "value": text, "state": after.map_or("open", |f| f.state.name())}))
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

    /// p0001 title, p0002 Part, p0003 Article, p0004 a choice and an optional phrase, p0005 a
    /// blank and two named fill-ins, p0006 a note with brackets, p0007 an unclosed bracket, p0008 end.
    fn section() -> Session {
        let mut d = Document::new();
        d.body = vec![
            para("SCT", "SECTION 09 2900 - GYPSUM BOARD"),
            para("PRT", "GENERAL"),
            para("ART", "SUMMARY"),
            para("PR1", "Thickness: [1/2 inch] [5/8 inch], [factory primed] and tapered."),
            para("PR1", "Temperature above ___ deg F; colour <Insert colour>; on <Insert Issue Date>."),
            para("CMT", "Retain [this] if needed."),
            para("PR1", "[LEED Submittals: product data"),
            para("EOS", "END OF SECTION 09 2900"),
        ];
        let mut s = Session::new(d);
        run(&mut s, "spec.import", json!({}));
        s
    }

    #[test]
    fn scanning_finds_choices_optional_text_blanks_and_named_fill_ins() {
        let f = scan("A [x] [y] [z] b [w] c ____ d <Insert colour> e <Insert date of issue> [<Insert other>]");
        let kinds: Vec<(&str, Vec<String>, String)> = f.iter().map(|f| (f.kind.name(), f.options.clone(), f.name.clone())).collect();
        assert_eq!(
            kinds,
            [
                ("choice", vec!["x".into(), "y".into(), "z".into()], "Choice".into()),
                ("optional", vec!["w".into()], "Optional text".into()),
                ("text", vec![], "Blank".into()),
                ("text", vec![], "colour".into()),
                ("date", vec![], "date of issue".into()),
                ("optional", vec!["<Insert other>".into()], "Optional text".into()),
            ]
        );
        assert!(scan("[unclosed and [] empty").is_empty());
    }

    #[test]
    fn detect_wraps_fields_once_and_skips_notes() {
        let mut s = section();
        let r = run(&mut s, "spec.fields.detect", json!({}));
        assert_eq!(r["added"], 5, "{r}");
        assert_eq!(r["byKind"], json!({"choice": 1, "date": 1, "optional": 1, "text": 2}));
        // The text reads as before; nothing new to find the second time.
        let body = s.doc.plain_text(StoryRef::Body).replace(OBJ, "");
        assert!(body.contains("Thickness: [1/2 inch] [5/8 inch], [factory primed] and tapered."));
        assert_eq!(run(&mut s, "spec.fields.detect", json!({}))["added"], 0);
        let list = run(&mut s, "spec.fields.list", json!({}));
        let f = &list["fields"][0];
        assert_eq!((f["kind"].clone(), f["state"].clone(), f["paragraph"].clone()), (json!("choice"), json!("open"), json!("p0004")));
        assert_eq!(f["options"], json!(["1/2 inch", "5/8 inch"]));
        assert_eq!(list["openPrinting"], 0, "nothing prints yet");
        // The paragraphs are still spec paragraphs.
        assert_eq!(sections::outline(&s.doc).len(), 8);
    }

    #[test]
    fn fields_are_set_left_out_reset_and_survive_saving() {
        let mut s = section();
        run(&mut s, "spec.fields.detect", json!({}));
        run(&mut s, "spec.fields.set", json!({"id": "f0001", "option": 1}));
        run(&mut s, "spec.fields.set", json!({"id": "f0002", "omit": true}));
        run(&mut s, "spec.fields.set", json!({"id": "f0003", "value": "40"}));
        run(&mut s, "spec.fields.set", json!({"id": "f0004", "value": "Light grey"}));
        assert!(s.run("spec.fields.set", &json!({"id": "f0003", "omit": true})).is_err(), "only optional text is left out");
        let text = |s: &Session, block: usize| s.doc.body[block].as_para().unwrap().plain_text().replace(OBJ, "");
        assert_eq!(text(&s, 3), "Thickness: 5/8 inch, (leave out) and tapered.");
        let list = run(&mut s, "spec.fields.list", json!({}));
        let states: Vec<&str> = list["fields"].as_array().unwrap().iter().map(|f| f["state"].as_str().unwrap()).collect();
        assert_eq!(states.iter().filter(|x| **x == "set").count(), 3);
        assert_eq!(states.iter().filter(|x| **x == "omitted").count(), 1);

        // Saved and reopened, the fields come back with their values.
        let bytes = crate::io::save_bytes("x.docx", &s.doc).unwrap();
        let mut b = Session::new(crate::io::open_bytes("x.docx", &bytes).unwrap());
        let again = run(&mut b, "spec.fields.list", json!({}));
        assert_eq!(again["fields"], list["fields"]);

        // Back to the master's wording.
        run(&mut s, "spec.fields.set", json!({"id": "f0001", "reset": true}));
        run(&mut s, "spec.fields.set", json!({"id": "f0002", "value": null}));
        assert_eq!(text(&s, 3), "Thickness: [1/2 inch] [5/8 inch], [factory primed] and tapered.");
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(text(&s, 3), "Thickness: [1/2 inch] [5/8 inch], (leave out) and tapered.");
    }

    #[test]
    fn publishing_drops_left_out_text_and_reports_open_fields() {
        let mut s = section();
        run(&mut s, "spec.fields.detect", json!({}));
        let ids: Vec<String> = sections::outline(&s.doc).iter().filter(|e| e.kind != Kind::Note).map(|e| e.id.clone()).collect();
        run(&mut s, "spec.activate", json!({"ids": ids}));
        let list = run(&mut s, "spec.fields.list", json!({}));
        let id_of = |kind: &str| list["fields"].as_array().unwrap().iter().find(|f| f["kind"] == kind).unwrap()["id"].as_str().unwrap().to_string();
        run(&mut s, "spec.fields.set", json!({"id": id_of("optional"), "omit": true}));
        run(&mut s, "spec.fields.set", json!({"id": id_of("choice"), "option": 0}));
        let (copy, counts) = super::super::output::printable_copy(&s.doc).unwrap();
        let text = |d: &Document, block: usize| d.body[block].as_para().unwrap().plain_text().replace(OBJ, "");
        assert_eq!(text(&copy, 3), "Thickness: 1/2 inch, and tapered.");
        assert_eq!(counts.fields_open.len(), 3, "the blank, colour and date: {:?}", counts.fields_open);
        let r =
            run(&mut s, "spec.exportPdf", json!({"path": std::env::temp_dir().join(format!("fields-{}.pdf", std::process::id())).to_string_lossy()}));
        assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("3 fill-ins")), "{r}");
        // The live document still holds the choice.
        assert!(text(&s.doc, 3).contains("(leave out)"));
    }
}
