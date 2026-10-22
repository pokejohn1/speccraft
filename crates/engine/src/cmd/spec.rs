//! Specs › Items: mark paragraphs (or phrases) of a document as specification items, list them
//! and change their text. Added by the Speccraft fork; not part of the original WordCraft.
//!
//! A spec item is an ordinary rich text content control (see [`wordcraft_doc::control`]) whose
//! tag is `spec:<id>`. Content controls already survive saving to .docx and reopening, and Word
//! shows them as normal content controls, so no change to the document model or the .docx
//! reader/writer is needed.

use serde_json::{Value, json};
use wordcraft_doc::control::{ControlKind, ControlRange};
use wordcraft_doc::para::OBJ;
use wordcraft_doc::{Document, Pos, StoryRef};

use super::{controls, pos_json};
use crate::{CmdError, CmdResult, CommandSpec, Selection, Session, p};

const LOC: &str = "Specs › Items";
/// The start of the tag of every spec item's content control.
pub const TAG_PREFIX: &str = "spec:";
/// Longest item id, in characters.
const MAX_ID: usize = 64;
/// Longest title kept, in characters.
const MAX_TITLE: usize = 200;
/// Longest text `spec.set` writes, in characters.
const MAX_TEXT: usize = 20_000;
/// Most items one document is scanned for.
const MAX_ITEMS: usize = 10_000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("spec.tag", "Tag as Spec Item", LOC, tag).params(r#"{"id": string, "title"?: string}"#),
        CommandSpec::new("spec.list", "List Spec Items", LOC, list).pure(),
        CommandSpec::new("spec.set", "Set Spec Item Text", LOC, set).params(r#"{"id": string, "text": string}"#),
    ]
}

/// The `id` param: 1 to 64 letters, digits, `.`, `_` or `-`.
fn id_param(v: &Value) -> Result<String, CmdError> {
    let id = p::req_str(v, "id")?.trim();
    let ok = !id.is_empty() && id.chars().count() <= MAX_ID && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if ok { Ok(id.to_string()) } else { Err(CmdError::Params("`id` must be 1 to 64 letters, digits, '.', '_' or '-'".into())) }
}

/// Every spec item in the document (body, then the other stories), each story in document order.
fn items(doc: &Document) -> Vec<(String, ControlRange)> {
    let stories = std::iter::once(StoryRef::Body).chain(doc.parts.keys().map(|k| StoryRef::Part(*k)));
    let mut out = Vec::new();
    for story in stories {
        let mut ranges = doc.control_ranges(story);
        ranges.sort_by(|a, b| a.start.cmp(&b.start));
        for r in ranges {
            if let Some(id) = r.control.tag.strip_prefix(TAG_PREFIX).map(str::to_string) {
                out.push((id, r));
                if out.len() >= MAX_ITEMS {
                    return out;
                }
            }
        }
    }
    out
}

/// Mark the selection as spec item `id`. With only a caret, the whole paragraph it is in.
fn tag(s: &mut Session, v: &Value) -> CmdResult {
    let id = id_param(v)?;
    if items(&s.doc).iter().any(|(i, _)| *i == id) {
        return Err(CmdError::Failed(format!("a spec item called {id:?} already exists")));
    }
    let (a, b) = s.sel.ordered();
    if a == b
        && let Some(len) = s.doc.para_at(&a).map(|para| para.len()).filter(|n| *n > 0)
    {
        s.sel = Selection { anchor: Pos { off: 0, ..a.clone() }, focus: Pos { off: len, ..a } };
    }
    let mut args = json!({"tag": format!("{TAG_PREFIX}{id}")});
    if let Some(t) = p::str(v, "title") {
        args["title"] = json!(t.chars().filter(|c| !c.is_control()).take(MAX_TITLE).collect::<String>());
    }
    controls::insert(s, &args, ControlKind::RichText)?;
    Ok(json!({"id": id}))
}

/// All spec items: id, title, text and where they are.
fn list(s: &mut Session, _v: &Value) -> CmdResult {
    let rows: Vec<Value> = items(&s.doc)
        .into_iter()
        .map(|(id, r)| {
            let text = if r.control.showing_placeholder { String::new() } else { s.doc.control_text(&r) };
            json!({"id": id, "title": r.control.title, "text": text, "block": r.control.block, "start": pos_json(&r.start), "end": pos_json(&r.end)})
        })
        .collect();
    Ok(json!({"count": rows.len(), "items": rows}))
}

/// Replace the text of spec item `id` (one paragraph's worth: line breaks become spaces).
fn set(s: &mut Session, v: &Value) -> CmdResult {
    let id = id_param(v)?;
    let text: String = p::req_str(v, "text")?
        .chars()
        .map(|c| if matches!(c, '\n' | '\r' | '\t') { ' ' } else { c })
        .filter(|c| !c.is_control() && *c != OBJ)
        .take(MAX_TEXT)
        .collect();
    if text.is_empty() {
        return Err(CmdError::Params("`text` must not be empty".into()));
    }
    let Some((_, r)) = items(&s.doc).into_iter().find(|(i, _)| *i == id) else {
        return Err(CmdError::Failed(format!("no spec item called {id:?}")));
    };
    let a = r.content_start();
    controls::check_edit(s, &a, &r.end)?;
    // The text keeps the formatting the item's start marker was given.
    let props = s.doc.para_at(&r.start).map(|para| para.props_of_char(r.start.off).clone()).unwrap_or_default();
    s.doc.delete_range(&a, &r.end)?;
    s.doc.insert_text(&a, &text, &props)?;
    if let Some(c) = s.doc.control_mut(&r.start) {
        c.showing_placeholder = false;
    }
    s.sel = Selection::caret(a);
    Ok(json!({"id": id, "text": text}))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::Session;

    fn s() -> Session {
        Session::new(wordcraft_doc::Document::new())
    }

    fn run(s: &mut Session, id: &str, v: Value) -> Value {
        s.run(id, &v).unwrap_or_else(|e| panic!("{id}: {e}"))
    }

    fn body(s: &Session) -> String {
        s.doc.plain_text(wordcraft_doc::StoryRef::Body)
    }

    #[test]
    fn tag_list_and_set_survive_saving_to_docx_and_reopening() {
        let mut a = s();
        run(&mut a, "text.insert", json!({"text": "The wall shall be 12 inches thick."}));
        run(&mut a, "spec.tag", json!({"id": "wall-1", "title": "Wall thickness"}));
        let listed = run(&mut a, "spec.list", json!({}));
        assert_eq!(listed["count"], 1);
        assert_eq!(listed["items"][0]["id"], "wall-1");
        assert_eq!(listed["items"][0]["title"], "Wall thickness");
        assert_eq!(listed["items"][0]["text"], "The wall shall be 12 inches thick.");

        run(&mut a, "spec.set", json!({"id": "wall-1", "text": "The wall shall be 16 inches thick."}));
        assert_eq!(run(&mut a, "spec.list", json!({}))["items"][0]["text"], "The wall shall be 16 inches thick.");

        let bytes = crate::io::save_bytes("spec.docx", &a.doc).unwrap();
        let mut b = Session::new(crate::io::open_bytes("spec.docx", &bytes).unwrap());
        let again = run(&mut b, "spec.list", json!({}));
        assert_eq!(again["count"], 1);
        assert_eq!(again["items"][0]["id"], "wall-1");
        assert_eq!(again["items"][0]["title"], "Wall thickness");
        assert_eq!(again["items"][0]["text"], "The wall shall be 16 inches thick.");
    }

    #[test]
    fn a_second_item_needs_its_own_id() {
        let mut a = s();
        run(&mut a, "document.setText", json!({"text": "First\nSecond"}));
        run(&mut a, "caret.set", json!({"pos": {"block": 0, "off": 0}}));
        run(&mut a, "spec.tag", json!({"id": "one"}));
        run(&mut a, "caret.set", json!({"pos": {"block": 1, "off": 0}}));
        assert!(a.run("spec.tag", &json!({"id": "one"})).is_err(), "a repeated id is refused");
        run(&mut a, "spec.tag", json!({"id": "two"}));
        let listed = run(&mut a, "spec.list", json!({}));
        assert_eq!(listed["count"], 2);
        assert_eq!(listed["items"][0]["id"], "one");
        assert_eq!(listed["items"][1]["id"], "two");
    }

    #[test]
    fn bad_input_is_refused_and_changes_nothing() {
        let mut a = s();
        run(&mut a, "text.insert", json!({"text": "Keep me"}));
        run(&mut a, "spec.tag", json!({"id": "keep"}));
        let before = body(&a);
        let long_id = "x".repeat(65);
        for bad in [json!(null), json!({}), json!({"id": ""}), json!({"id": "has space"}), json!({"id": long_id}), json!({"id": 5})] {
            assert!(a.run("spec.tag", &bad).is_err(), "{bad}");
        }
        assert!(a.run("spec.set", &json!({"id": "keep"})).is_err(), "text is required");
        assert!(a.run("spec.set", &json!({"id": "keep", "text": ""})).is_err(), "text must not be empty");
        assert!(a.run("spec.set", &json!({"id": "nope", "text": "x"})).is_err(), "unknown id");
        assert_eq!(body(&a), before);
    }
}
