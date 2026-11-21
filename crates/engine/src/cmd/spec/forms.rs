//! Specs › Short Form: the full, short and outline forms of a section from the same content
//! (milestone M16). Added by the Speccraft fork; not part of the original WordCraft.
//!
//! Nothing is shortened by itself: the author decides. A paragraph can be **left out of the short
//! form** (its sub-paragraphs go with it) and can have **short wording** written for it, used in
//! the short and outline forms instead of the full text. Both are kept per paragraph in
//! `Speccraft.Short`, with a fingerprint of the full text the short wording was written against;
//! when the full text changes later (an edit, a master update), Check Section says the short
//! wording may need a look.
//!
//! - **full**: what prints (as before).
//! - **short**: what prints, less what is left out of the short form, with short wording.
//! - **outline**: the short form down to a depth: Parts, Articles, and paragraphs to `depth`
//!   levels below the Article (default 1).

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use wordcraft_doc::Document;

use super::project::fingerprint;
use super::sections::{self, Entry, Kind};
use crate::{CmdError, CmdResult, CommandSpec, Session, p};

const LOC: &str = "Specs › Short Form";
/// The short-form marks: id → [`Mark`].
pub const SHORT_PROP: &str = "Speccraft.Short";
/// Longest short wording.
const MAX_TEXT: usize = 4_000;

/// Which form a copy is made in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Form {
    #[default]
    Full,
    Short,
    /// The short form to this many paragraph levels below the Article.
    Outline(u8),
}

impl Form {
    /// From the `form` and `depth` parameters.
    pub fn from_params(v: &Value) -> Result<Form, CmdError> {
        match p::str(v, "form").unwrap_or("full") {
            "full" => Ok(Form::Full),
            "short" => Ok(Form::Short),
            "outline" => {
                let d = p::u64(v, "depth").unwrap_or(1);
                if d > 6 {
                    return Err(CmdError::Params("`depth` must be 0 to 6".into()));
                }
                Ok(Form::Outline(d as u8))
            }
            other => Err(CmdError::Params(format!("`form` must be \"full\", \"short\" or \"outline\", not {other:?}"))),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Form::Full => "full",
            Form::Short => "short",
            Form::Outline(_) => "outline",
        }
    }

    /// What goes after a file name for this form ("" for the full form).
    pub fn suffix(self) -> &'static str {
        match self {
            Form::Full => "",
            Form::Short => " (Short Form)",
            Form::Outline(_) => " (Outline)",
        }
    }
}

/// One paragraph's short-form mark.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    /// Left out of the short and outline forms.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub leave: bool,
    /// Short wording, used instead of the full text in the short and outline forms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The full text's fingerprint when the short wording was written.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub of: String,
}

pub fn marks(doc: &Document) -> BTreeMap<String, Mark> {
    doc.custom_prop(SHORT_PROP).and_then(|v| serde_json::from_str(v).ok()).unwrap_or_default()
}

fn store(doc: &mut Document, m: &BTreeMap<String, Mark>) -> Result<(), CmdError> {
    if m.is_empty() {
        doc.remove_custom_prop(SHORT_PROP);
        return Ok(());
    }
    let text = serde_json::to_string(m).map_err(|e| CmdError::Failed(e.to_string()))?;
    doc.set_custom_prop(SHORT_PROP, &text);
    Ok(())
}

fn full_text(doc: &Document, e: &Entry) -> String {
    super::edits::base_text(doc, e)
}

/// Short wording whose full text has changed since it was written: (id, short wording).
pub fn stale(doc: &Document) -> Vec<(String, String)> {
    let m = marks(doc);
    if m.is_empty() {
        return Vec::new();
    }
    sections::outline(doc)
        .iter()
        .filter_map(|e| {
            let mark = m.get(&e.id)?;
            let short = mark.text.as_ref()?;
            (!mark.of.is_empty() && mark.of != fingerprint(full_text(doc, e).as_bytes())).then(|| (e.id.clone(), short.clone()))
        })
        .collect()
}

/// The entries `form` leaves out of a copy of `doc`, by id: those marked to leave (with what is
/// under them) for the short and outline forms, and for the outline those deeper than its depth.
pub fn left_out(doc: &Document, entries: &[Entry], form: Form) -> HashSet<String> {
    if form == Form::Full {
        return HashSet::new();
    }
    let m = marks(doc);
    let mut out: HashSet<usize> = HashSet::new();
    // The level of the Article a paragraph is under, to count depth below it.
    for (i, e) in entries.iter().enumerate() {
        let parent_out = e.parent.is_some_and(|p| out.contains(&p));
        let marked = m.get(&e.id).is_some_and(|x| x.leave);
        let too_deep = match form {
            Form::Outline(depth) if matches!(e.kind, Kind::Para | Kind::Other) => {
                // Paragraphs counted from the Article above them: the first level is 1.
                let mut below = 0u8;
                let mut p = Some(i);
                let mut guard = 0;
                while let Some(k) = p {
                    let Some(x) = entries.get(k) else { break };
                    if matches!(x.kind, Kind::Article | Kind::Part | Kind::Title) {
                        break;
                    }
                    below = below.saturating_add(1);
                    p = x.parent;
                    guard += 1;
                    if guard > 64 {
                        break;
                    }
                }
                below > depth
            }
            _ => false,
        };
        if parent_out || marked || too_deep {
            out.insert(i);
        }
    }
    out.into_iter().filter_map(|i| entries.get(i).map(|e| e.id.clone())).collect()
}

/// Put the short wording into a printable copy (short and outline forms).
pub fn apply_wording(copy: &mut Document, source_marks: &BTreeMap<String, Mark>, form: Form) -> Result<usize, CmdError> {
    if form == Form::Full || source_marks.values().all(|m| m.text.is_none()) {
        return Ok(0);
    }
    let mut n = 0;
    for (id, m) in source_marks {
        let Some(text) = &m.text else { continue };
        let entries = sections::outline(copy);
        let Some(e) = entries.iter().find(|e| e.id == *id) else { continue };
        super::edits::set_paragraph_text(copy, e, text)?;
        n += 1;
    }
    Ok(n)
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("spec.short.set", "Short Form of a Paragraph", LOC, set_cmd).params(
            r#"{"id"?: string (default: the paragraph at the caret), "leave"?: bool (leave it out of the short form), "text"?: string or null (its short wording; null: none)}"#,
        ),
        CommandSpec::new("spec.short.list", "Short Form", LOC, list_cmd).params(r#"{}"#).pure(),
    ]
}

fn set_cmd(s: &mut Session, v: &Value) -> CmdResult {
    let entries = sections::outline(&s.doc);
    let id = match p::str(v, "id") {
        Some(id) => id.trim().to_string(),
        None => sections::at_caret(s, &entries)
            .and_then(|i| entries.get(i))
            .map(|e| e.id.clone())
            .ok_or_else(|| CmdError::Failed("the caret is not in a spec paragraph".into()))?,
    };
    let e = entries.iter().find(|e| e.id == id).ok_or_else(|| CmdError::Failed(format!("no spec paragraph {id:?}")))?;
    if matches!(e.kind, Kind::Title | Kind::End) {
        return Err(CmdError::Failed("the title and end lines are in every form".into()));
    }
    let mut m = marks(&s.doc);
    let mut mark = m.get(&id).cloned().unwrap_or_default();
    if let Some(leave) = p::bool(v, "leave") {
        mark.leave = leave;
    }
    match v.get("text") {
        None => {}
        Some(Value::Null) => {
            mark.text = None;
            mark.of.clear();
        }
        Some(Value::String(t)) => {
            let t = t.trim();
            if t.chars().count() > MAX_TEXT || t.contains(['\n', '\r']) {
                return Err(CmdError::Params(format!("short wording is one paragraph of at most {MAX_TEXT} characters")));
            }
            if t.is_empty() {
                mark.text = None;
                mark.of.clear();
            } else {
                mark.text = Some(t.to_string());
                mark.of = fingerprint(full_text(&s.doc, e).as_bytes());
            }
        }
        Some(_) => return Err(CmdError::Params("`text` must be a string or null".into())),
    }
    if mark == Mark::default() {
        m.remove(&id);
    } else {
        m.insert(id.clone(), mark.clone());
    }
    store(&mut s.doc, &m)?;
    Ok(json!({"id": id, "leave": mark.leave, "text": mark.text}))
}

fn list_cmd(s: &mut Session, _: &Value) -> CmdResult {
    let m = marks(&s.doc);
    let stale: HashSet<String> = stale(&s.doc).into_iter().map(|(id, _)| id).collect();
    let entries = sections::outline(&s.doc);
    let short_out = left_out(&s.doc, &entries, Form::Short);
    let items: Vec<Value> = entries
        .iter()
        .filter_map(|e| {
            let mark = m.get(&e.id)?;
            Some(json!({"id": e.id, "leave": mark.leave, "text": mark.text, "stale": stale.contains(&e.id)}))
        })
        .collect();
    let printing = entries.iter().filter(|e| e.on).count();
    let short = entries.iter().filter(|e| e.on && !short_out.contains(&e.id)).count();
    Ok(json!({"marks": items, "printing": printing, "shortForm": short, "stale": stale.len()}))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::{Value, json};
    use wordcraft_doc::{Block, Document, Paragraph};

    use super::Form;
    use crate::Session;
    use crate::cmd::spec::output::printable_copy_as;

    fn section() -> Session {
        let p = |s: &str, t: &str| Arc::new(Block::Para(Paragraph::with_text(t, Default::default()).styled(s)));
        let mut d = Document::new();
        d.body = vec![
            p("SCT", "SECTION 09 2900 - GYPSUM BOARD"),
            p("PRT", "GENERAL"),
            p("ART", "SUMMARY"),
            p("PR1", "Section includes interior gypsum board, trim accessories and joint treatment."),
            p("PR2", "Tile backing panels in wet areas."),
            p("PR1", "Related requirements are specified elsewhere."),
            p("ART", "SUBMITTALS"),
            p("PR1", "Product data for each type of panel."),
            p("PR2", "Include installation instructions."),
            p("PR3", "Include fastener schedules."),
            p("EOS", "END OF SECTION 09 2900"),
        ];
        let mut s = Session::new(d);
        s.run("spec.import", &json!({})).unwrap();
        let ids: Vec<String> = (2..=10).map(|i| format!("p{i:04}")).collect();
        s.run("spec.activate", &json!({"ids": ids})).unwrap();
        s
    }

    fn texts(d: &Document) -> Vec<String> {
        d.body.iter().filter_map(|b| b.as_para()).map(|p| p.plain_text()).collect()
    }

    fn run(s: &mut Session, id: &str, v: Value) -> Value {
        s.run(id, &v).unwrap_or_else(|e| panic!("{id}: {e}"))
    }

    #[test]
    fn the_short_form_leaves_out_what_the_author_says_and_uses_their_wording() {
        let mut s = section();
        // Left out: "Related requirements…", and the submittal paragraph with what is under it.
        run(&mut s, "spec.short.set", json!({"id": "p0006", "leave": true}));
        run(&mut s, "spec.short.set", json!({"id": "p0009", "leave": true}));
        run(&mut s, "spec.short.set", json!({"id": "p0004", "text": "Interior gypsum board and accessories."}));
        let (full, _) = printable_copy_as(&s.doc, Form::Full).unwrap();
        assert_eq!(texts(&full).len(), 11, "the full form is unchanged");
        let (short, _) = printable_copy_as(&s.doc, Form::Short).unwrap();
        assert_eq!(
            texts(&short),
            [
                "SECTION 09 2900 - GYPSUM BOARD",
                "GENERAL",
                "SUMMARY",
                "Interior gypsum board and accessories.",
                "Tile backing panels in wet areas.",
                "SUBMITTALS",
                "Product data for each type of panel.",
                "END OF SECTION 09 2900"
            ]
        );
        // The section itself is unchanged.
        assert!(texts(&s.doc).contains(&"Section includes interior gypsum board, trim accessories and joint treatment.".to_string()));
        let l = run(&mut s, "spec.short.list", json!({}));
        assert_eq!((l["printing"].clone(), l["shortForm"].clone(), l["stale"].clone()), (json!(11), json!(8), json!(0)));
    }

    #[test]
    fn the_outline_form_keeps_headings_and_the_first_levels() {
        let mut s = section();
        let (o, _) = printable_copy_as(&s.doc, Form::Outline(1)).unwrap();
        assert_eq!(
            texts(&o),
            [
                "SECTION 09 2900 - GYPSUM BOARD",
                "GENERAL",
                "SUMMARY",
                "Section includes interior gypsum board, trim accessories and joint treatment.",
                "Related requirements are specified elsewhere.",
                "SUBMITTALS",
                "Product data for each type of panel.",
                "END OF SECTION 09 2900"
            ]
        );
        let (o, _) = printable_copy_as(&s.doc, Form::Outline(0)).unwrap();
        assert_eq!(texts(&o), ["SECTION 09 2900 - GYPSUM BOARD", "GENERAL", "SUMMARY", "SUBMITTALS", "END OF SECTION 09 2900"]);
        let (o, _) = printable_copy_as(&s.doc, Form::Outline(2)).unwrap();
        assert_eq!(texts(&o).len(), 10, "all but the third level");
        assert!(Form::from_params(&json!({"form": "outline", "depth": 9})).is_err());
        assert!(s.run("spec.short.set", &json!({"id": "p0001", "leave": true})).is_err(), "the title is in every form");
    }

    #[test]
    fn short_wording_is_flagged_when_the_full_text_changes() {
        let mut s = section();
        run(&mut s, "spec.short.set", json!({"id": "p0008", "text": "Product data."}));
        assert!(super::stale(&s.doc).is_empty());
        let e = crate::cmd::spec::sections::outline(&s.doc).into_iter().find(|e| e.id == "p0008").unwrap();
        crate::cmd::spec::edits::set_paragraph_text(&mut s.doc, &e, "Product data and samples for each type of panel.").unwrap();
        assert_eq!(super::stale(&s.doc), [("p0008".to_string(), "Product data.".to_string())]);
        let r = run(&mut s, "spec.check", json!({}));
        assert!(r["findings"].as_array().unwrap().iter().any(|f| f["code"] == "shortStale" && f["para"] == "p0008"), "{r}");
        // Writing it again settles it; clearing it removes the mark.
        run(&mut s, "spec.short.set", json!({"id": "p0008", "text": "Product data and samples."}));
        assert!(super::stale(&s.doc).is_empty());
        run(&mut s, "spec.short.set", json!({"id": "p0008", "text": null}));
        assert!(s.doc.custom_prop(super::SHORT_PROP).is_none());
    }
}
