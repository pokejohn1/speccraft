//! A paragraph's notes and tags (milestone M26). Added by the Speccraft fork; not part of the
//! original WordCraft.
//!
//! - **Master notes** are the author notes the master has just before a paragraph (they never
//!   print).
//! - **Project notes** are the author's own notes on a paragraph for this project, kept out of
//!   the text ([`PROJECT_NOTES_PROP`]); a draft prints them after their paragraph.
//! - **Tags** are the author's words for finding paragraphs again ("coordination", "owner
//!   review"), kept in [`TAGS_PROP`].

use std::collections::BTreeMap;

use serde_json::{Value, json};
use wordcraft_doc::Document;
use wordcraft_doc::para::OBJ;

use super::sections::{self, Entry, Kind};
use crate::{CmdError, CmdResult, CommandSpec, Session, p};

const LOC: &str = "Specs › Paragraph";
pub const PROJECT_NOTES_PROP: &str = "Speccraft.ProjectNotes";
pub const TAGS_PROP: &str = "Speccraft.Tags";
const MAX_NOTE: usize = 4_000;
const MAX_TAGS: usize = 20;
const MAX_TAG: usize = 40;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("spec.paragraph.info", "Paragraph Notes and Tags", LOC, info_cmd)
            .params(r#"{"id"?: string (default: the paragraph at the caret)}"#)
            .pure(),
        CommandSpec::new("spec.paragraph.note", "Project Note", LOC, note_cmd)
            .params(r#"{"id"?: string (default: the paragraph at the caret), "text": string (empty: no note)}"#),
        CommandSpec::new("spec.paragraph.tags", "Paragraph Tags", LOC, tags_cmd)
            .params(r#"{"id"?: string (default: the paragraph at the caret), "tags": [string]}"#),
        CommandSpec::new("spec.tags.list", "Tags in Section", LOC, tags_list).pure(),
    ]
}

pub fn project_notes(doc: &Document) -> BTreeMap<String, String> {
    doc.custom_prop(PROJECT_NOTES_PROP).and_then(|v| serde_json::from_str(v).ok()).unwrap_or_default()
}

pub fn tags(doc: &Document) -> BTreeMap<String, Vec<String>> {
    doc.custom_prop(TAGS_PROP).and_then(|v| serde_json::from_str(v).ok()).unwrap_or_default()
}

fn store<T: serde::Serialize>(doc: &mut Document, prop: &str, m: &BTreeMap<String, T>) -> Result<(), CmdError> {
    if m.is_empty() {
        doc.remove_custom_prop(prop);
    } else {
        doc.set_custom_prop(prop, &serde_json::to_string(m).map_err(|e| CmdError::Failed(e.to_string()))?);
    }
    Ok(())
}

fn target(s: &Session, v: &Value, entries: &[Entry]) -> Result<usize, CmdError> {
    match p::str(v, "id") {
        Some(id) => entries.iter().position(|e| e.id == id.trim()).ok_or_else(|| CmdError::Failed(format!("no spec paragraph called {id:?}"))),
        None => sections::at_caret(s, entries).ok_or_else(|| CmdError::Failed("the caret is not in a spec paragraph".into())),
    }
}

/// The master's notes just before paragraph `i` (and its own text if it is a note).
fn master_notes(doc: &Document, entries: &[Entry], i: usize) -> Vec<String> {
    let text = |e: &Entry| doc.control_text(&e.range).replace(OBJ, "").trim().to_string();
    let mut out: Vec<String> = entries.get(..i).unwrap_or(&[]).iter().rev().take_while(|e| e.kind == Kind::Note).map(text).collect();
    out.reverse();
    out
}

fn info_cmd(s: &mut Session, v: &Value) -> CmdResult {
    let entries = sections::outline(&s.doc);
    let i = target(s, v, &entries)?;
    let e = entries.get(i).ok_or_else(|| CmdError::Failed("no such paragraph".into()))?;
    let why = s.run("spec.why", &json!({"id": e.id})).ok().and_then(|w| w.get("reasons").cloned()).unwrap_or(json!([]));
    Ok(json!({
        "id": e.id,
        "seq": i,
        "masterNotes": master_notes(&s.doc, &entries, i),
        "projectNote": project_notes(&s.doc).get(&e.id).cloned().unwrap_or_default(),
        "tags": tags(&s.doc).get(&e.id).cloned().unwrap_or_default(),
        "reasons": why,
    }))
}

fn note_cmd(s: &mut Session, v: &Value) -> CmdResult {
    let entries = sections::outline(&s.doc);
    let i = target(s, v, &entries)?;
    let id = entries.get(i).map(|e| e.id.clone()).ok_or_else(|| CmdError::Failed("no such paragraph".into()))?;
    let text = p::req_str(v, "text")?.trim().to_string();
    if text.chars().count() > MAX_NOTE || text.contains(OBJ) {
        return Err(CmdError::Params(format!("a note is at most {MAX_NOTE} characters")));
    }
    let mut all = project_notes(&s.doc);
    if text.is_empty() {
        all.remove(&id);
    } else {
        all.insert(id.clone(), text.clone());
    }
    let live: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
    all.retain(|k, _| live.contains(&k.as_str()));
    store(&mut s.doc, PROJECT_NOTES_PROP, &all)?;
    Ok(json!({"id": id, "note": text}))
}

fn tags_cmd(s: &mut Session, v: &Value) -> CmdResult {
    let entries = sections::outline(&s.doc);
    let i = target(s, v, &entries)?;
    let id = entries.get(i).map(|e| e.id.clone()).ok_or_else(|| CmdError::Failed("no such paragraph".into()))?;
    let list = v.get("tags").and_then(Value::as_array).ok_or_else(|| CmdError::Params("`tags` must be a list of words".into()))?;
    if list.len() > MAX_TAGS {
        return Err(CmdError::Params(format!("at most {MAX_TAGS} tags on a paragraph")));
    }
    let mut want: Vec<String> = Vec::new();
    for t in list {
        let t = t.as_str().ok_or_else(|| CmdError::Params("`tags` must be a list of words".into()))?.trim().to_lowercase();
        if t.chars().count() > MAX_TAG || t.contains([',', '\n', OBJ]) {
            return Err(CmdError::Params(format!("a tag is one line of at most {MAX_TAG} characters, without commas")));
        }
        if !t.is_empty() && !want.contains(&t) {
            want.push(t);
        }
    }
    let mut all = tags(&s.doc);
    if want.is_empty() {
        all.remove(&id);
    } else {
        all.insert(id.clone(), want.clone());
    }
    let live: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
    all.retain(|k, _| live.contains(&k.as_str()));
    store(&mut s.doc, TAGS_PROP, &all)?;
    Ok(json!({"id": id, "tags": want}))
}

fn tags_list(s: &mut Session, _v: &Value) -> CmdResult {
    let mut by: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (id, list) in tags(&s.doc) {
        for t in list {
            by.entry(t).or_default().push(id.clone());
        }
    }
    Ok(json!({"tags": by}))
}

/// Print-time, for drafts: each project note as an italic paragraph after its paragraph.
pub fn print_project_notes(copy: &mut Document) {
    use std::sync::Arc;
    use wordcraft_doc::{Block, CharProps, Paragraph};
    let notes = project_notes(copy);
    if notes.is_empty() {
        return;
    }
    let mut at: Vec<(usize, String, Option<String>)> = sections::outline(copy)
        .into_iter()
        .filter_map(|e| {
            let note = notes.get(&e.id)?;
            let b = *e.range.start.path.0.first()? as usize;
            let style = copy.body.get(b).and_then(|x| x.as_para()).and_then(|p| p.props.style.clone());
            Some((b, note.clone(), style))
        })
        .collect();
    at.sort_by_key(|x| std::cmp::Reverse(x.0));
    for (b, note, style) in at {
        let mut p = Paragraph::with_text(&format!("Project note: {note}"), CharProps { italic: Some(true), ..Default::default() });
        if let Some(st) = style {
            p = p.styled(&st);
        }
        p.props.numbering = Some(wordcraft_doc::props::NumRef { num: 0, level: 0 });
        if b < copy.body.len() {
            copy.body.insert(b + 1, Arc::new(Block::Para(p)));
        }
    }
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

    #[test]
    fn notes_and_tags_stay_with_their_paragraph() {
        let mut d = Document::new();
        d.body = vec![
            para("SCT", "SECTION 09 2900 - GYPSUM BOARD"),
            para("PRT", "GENERAL"),
            para("CMT", "// Retain for fire-rated assemblies."),
            para("PR1", "Type X board."),
            para("EOS", "END OF SECTION 09 2900"),
        ];
        let mut s = Session::new(d);
        run(&mut s, "spec.import", json!({}));
        run(&mut s, "spec.activate", json!({"ids": ["p0002", "p0004"]}));
        let i = run(&mut s, "spec.paragraph.info", json!({"id": "p0004"}));
        assert_eq!(i["masterNotes"], json!(["// Retain for fire-rated assemblies."]));
        run(&mut s, "spec.paragraph.note", json!({"id": "p0004", "text": "Owner asked for Type C at the stair."}));
        run(&mut s, "spec.paragraph.tags", json!({"id": "p0004", "tags": ["Owner Review", "fire", "fire", ""]}));
        let i = run(&mut s, "spec.paragraph.info", json!({"id": "p0004"}));
        assert_eq!((i["projectNote"].clone(), i["tags"].clone()), (json!("Owner asked for Type C at the stair."), json!(["owner review", "fire"])));
        assert_eq!(run(&mut s, "spec.tags.list", json!({}))["tags"]["fire"], json!(["p0004"]));
        assert!(s.run("spec.paragraph.tags", &json!({"id": "p0004", "tags": ["a,b"]})).is_err());
        // Notes never print; a draft prints the project note after its paragraph.
        let plain = super::super::output::printable_copy_as(&s.doc, super::super::forms::Form::Full).unwrap().0;
        assert!(!plain.plain_text(wordcraft_doc::StoryRef::Body).contains("Owner asked"));
        let mut draft = plain.clone();
        print_project_notes(&mut draft);
        let text = draft.plain_text(wordcraft_doc::StoryRef::Body).replace(OBJ, "");
        assert!(text.contains("Type X board.\nProject note: Owner asked for Type C at the stair."), "{text}");
        // Clearing.
        run(&mut s, "spec.paragraph.note", json!({"id": "p0004", "text": ""}));
        run(&mut s, "spec.paragraph.tags", json!({"id": "p0004", "tags": []}));
        assert_eq!(s.doc.custom_prop(PROJECT_NOTES_PROP), None);
        assert_eq!(s.doc.custom_prop(TAGS_PROP), None);
    }
}
