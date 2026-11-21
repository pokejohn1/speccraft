//! Editing section paragraphs the way spec authors are used to (milestone M18). Added by the
//! Speccraft fork; not part of the original WordCraft.
//!
//! - Enter in a section paragraph makes a new added paragraph (`u0001`, …) instead of an
//!   unmanaged one ([`enter`]): at the start of a paragraph it goes before it, anywhere else after
//!   it (as its first child when it has paragraphs under it). [`AUTO_SELECT_PROP`] says whether
//!   new paragraphs come switched on ("Auto-Select New"; on unless switched off).
//! - `spec.indent` moves a paragraph, and by default everything under it, a level down or up by
//!   changing its style (PR1 → PR2, `CSILevel4` → `CSILevel5`).
//! - `spec.fixHierarchy` repairs levels that skip (a PR3 straight under an Article).
//! - `spec.noNumber` takes a paragraph's automatic number off, keeping its text where it was.

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::{Value, json};
use wordcraft_doc::para::OBJ;
use wordcraft_doc::props::NumRef;
use wordcraft_doc::{Block, Document, Path, Pos, StoryRef};

use super::edits;
use super::sections::{self, Entry, Kind};
use crate::{CmdError, CmdResult, CommandSpec, Selection, Session, p};

const LOC: &str = "Specs › Edits";
/// Document property: "off" when new paragraphs come in switched off.
pub const AUTO_SELECT_PROP: &str = "Speccraft.AutoSelectNew";
/// Document property: ids whose automatic number the author took off (`spec.noNumber`).
pub const NO_NUMBER_PROP: &str = "Speccraft.NoNumber";
/// The standard level styles, Part to PR5 (levels 1 to 7).
const LEVEL_STYLES: [&str; 7] = ["PRT", "ART", "PR1", "PR2", "PR3", "PR4", "PR5"];

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("spec.autoSelectNew", "Auto-Select New Paragraphs", LOC, auto_select_cmd)
            .params(r#"{"value"?: bool (default: the opposite of now)}"#),
        CommandSpec::new("spec.indent", "Indent Paragraph", LOC, indent_cmd).params(
            r#"{"id"?: string (default: the paragraph at the caret), "by"?: 1 (a level down, the default) | -1 (a level up), "branch"?: bool (with everything under it; default true)}"#,
        ),
        CommandSpec::new("spec.fixHierarchy", "Fix Hierarchy", LOC, fix_cmd),
        CommandSpec::new("spec.noNumber", "No Autonumber", LOC, no_number_cmd)
            .params(r#"{"id"?: string (default: the paragraph at the caret), "value"?: bool (default: the opposite of now)}"#),
    ]
}

/// Whether new paragraphs come switched on.
pub fn auto_select(doc: &Document) -> bool {
    doc.custom_prop(AUTO_SELECT_PROP).is_none_or(|v| !v.trim().eq_ignore_ascii_case("off"))
}

fn auto_select_cmd(s: &mut Session, v: &Value) -> CmdResult {
    let want = match v.get("value") {
        None | Some(Value::Null) => !auto_select(&s.doc),
        Some(x) => x.as_bool().ok_or_else(|| CmdError::Params("`value` must be true or false".into()))?,
    };
    if want {
        s.doc.remove_custom_prop(AUTO_SELECT_PROP);
    } else {
        s.doc.set_custom_prop(AUTO_SELECT_PROP, "off");
    }
    Ok(json!({"on": want}))
}

/// Whether `text` holds nothing but control markers and spaces.
fn blank(text: &str) -> bool {
    text.chars().all(|c| c == OBJ || c.is_whitespace())
}

/// Enter in a section paragraph (called by `text.newParagraph` before its own Enter): a new
/// added paragraph before or after it, with the caret in it. `None` leaves Enter to WordCraft: a
/// document without Speccraft data, a selection, the caret outside a section paragraph, or in a
/// note.
pub fn enter(s: &mut Session) -> Option<CmdResult> {
    if !s.doc.custom_props.iter().any(|p| p.name.starts_with("Speccraft.")) || s.sel.anchor != s.sel.focus {
        return None;
    }
    let entries = sections::outline(&s.doc);
    let i = sections::at_caret(s, &entries)?;
    let e = entries.get(i)?;
    if e.kind == Kind::Note || e.kind == Kind::Other {
        return None;
    }
    let para = s.doc.para_at(&e.range.start)?.clone();
    let off = s.sel.focus.off.min(para.text.len());
    let at_start = para.text.get(..off).is_some_and(blank) && !para.text.get(off..).is_some_and(blank);
    Some(add(s, &entries, i, at_start))
}

/// Add an empty paragraph before (`before`) or after section paragraph `i` and put the caret in it.
fn add(s: &mut Session, entries: &[Entry], i: usize, before: bool) -> CmdResult {
    let e = entries.get(i).ok_or_else(|| CmdError::Failed("no such paragraph".into()))?;
    let para = s.doc.para_at(&e.range.start).cloned().ok_or_else(|| CmdError::Failed("no paragraph".into()))?;
    let style = para.props.style.clone().unwrap_or_default();
    let block = e.range.start.path.0.first().map_or(0, |b| *b as usize);
    let has_children = entries.iter().any(|x| x.parent == Some(i) && x.kind != Kind::Note);
    let (style, at) = match (e.kind, before) {
        (Kind::Title, true) => return Err(CmdError::Failed("nothing comes before the section title".into())),
        (Kind::Title, false) => (edits::child_style(&style).unwrap_or_else(|| "PRT".into()), block + 1),
        (Kind::End, false) => return Err(CmdError::Failed("nothing comes after the end of section".into())),
        (Kind::End, true) => {
            // Before the end line, at the level of the paragraph above it.
            let above = entries.iter().take(i).rev().find(|x| !matches!(x.kind, Kind::Note | Kind::Other));
            let st = above.and_then(|x| s.doc.para_at(&x.range.start)).and_then(|p| p.props.style.clone()).unwrap_or_else(|| "PRT".into());
            let st = if above.is_some_and(|x| x.kind == Kind::Title) { edits::child_style(&st).unwrap_or_else(|| "PRT".into()) } else { st };
            (st, block)
        }
        (_, true) => (style, block),
        (_, false) if has_children => {
            (edits::child_style(&style).ok_or_else(|| CmdError::Failed(format!("a {style:?} paragraph has no level below it")))?, block + 1)
        }
        (_, false) => (style, block + 1),
    };
    let props = para.props_of_char(e.range.content_start().off).clone();
    let id = edits::new_user_id(entries);
    edits::insert_item(s, at, &style, "", &id, props)?;
    let caret = s.sel.clone();
    let on = auto_select(&s.doc) && sections::activate(s, &json!({"id": id})).is_ok();
    s.sel = caret;
    s.goal_x = None;
    Ok(json!({"id": id, "style": style, "on": on, "before": before}))
}

/// The style one level down (`by` > 0) or up from `style`, from Article down; `None` past the
/// ends, for the title, a Part, the end line or a style without a level.
pub fn shifted_style(doc: &Document, style: &str, by: i32) -> Option<String> {
    let up = style.to_ascii_uppercase();
    if let Some(at) = LEVEL_STYLES.iter().position(|s| *s == up) {
        let to = i32::try_from(at).ok()?.checked_add(by)?;
        // Article (index 1) is as high as a paragraph goes: Parts are the section's frame.
        if at == 0 || to < 1 {
            return None;
        }
        return LEVEL_STYLES.get(usize::try_from(to).ok()?).map(|s| (*s).to_string());
    }
    let n = i32::from(sections::style_level(style)?);
    let to = n.checked_add(by)?;
    if n < 2 || !(2..=9).contains(&to) {
        return None;
    }
    let name = format!("{}{to}", style.get(..style.len().saturating_sub(1))?);
    // A level-numbered style the document defines must have its neighbour defined too.
    let defined = |id: &str| doc.styles.get(id).is_some();
    if defined(style) && !defined(&name) {
        return None;
    }
    Some(name)
}

/// Set the style of top-level body paragraph `block`; a direct list level moves with it.
fn restyle(doc: &mut Document, block: usize, style: &str, by: i32) {
    let Some(b) = doc.body.get_mut(block) else { return };
    if let Block::Para(p) = Arc::make_mut(b) {
        p.props.style = Some(style.to_string());
        if let Some(n) = p.props.numbering.as_mut().filter(|n| n.num != 0) {
            n.level = u8::try_from((i32::from(n.level) + by).clamp(0, 8)).unwrap_or(n.level);
        }
        p.touch();
    }
}

/// Paragraph `i` and, with `branch`, everything under it, in document order.
fn branch_of(entries: &[Entry], i: usize, branch: bool) -> Vec<usize> {
    let mut out = vec![i];
    if branch {
        for (k, e) in entries.iter().enumerate().skip(i + 1) {
            if e.parent.is_some_and(|p| out.contains(&p)) {
                out.push(k);
            } else if e.kind != Kind::Note {
                break;
            }
        }
    }
    out
}

fn target(s: &Session, v: &Value, entries: &[Entry]) -> Result<usize, CmdError> {
    match p::str(v, "id") {
        Some(id) => entries.iter().position(|e| e.id == id.trim()).ok_or_else(|| CmdError::Failed(format!("no spec paragraph called {id:?}"))),
        None => sections::at_caret(s, entries).ok_or_else(|| CmdError::Failed("the caret is not in a spec paragraph".into())),
    }
}

fn indent_cmd(s: &mut Session, v: &Value) -> CmdResult {
    let entries = sections::outline(&s.doc);
    let i = target(s, v, &entries)?;
    let by = match v.get("by") {
        None | Some(Value::Null) => 1,
        Some(x) => match x.as_i64() {
            Some(1) => 1,
            Some(-1) => -1,
            _ => return Err(CmdError::Params("`by` must be 1 or -1".into())),
        },
    };
    let branch = p::bool(v, "branch") != Some(false);
    let e = entries.get(i).ok_or_else(|| CmdError::Failed("no such paragraph".into()))?;
    if !matches!(e.kind, Kind::Article | Kind::Para) {
        return Err(CmdError::Failed(format!("{:?} is a {}: only Articles and paragraphs change level", e.id, e.kind.name())));
    }
    let mut changes: Vec<(usize, String)> = Vec::new();
    for k in branch_of(&entries, i, branch) {
        let Some(x) = entries.get(k) else { continue };
        if x.kind == Kind::Note {
            continue;
        }
        let style = s.doc.para_at(&x.range.start).and_then(|p| p.props.style.clone()).unwrap_or_default();
        let to = shifted_style(&s.doc, &style, by).ok_or_else(|| {
            let way = if by > 0 { "down" } else { "up" };
            CmdError::Failed(format!("{:?} ({style}) can't go a level {way}", x.id))
        })?;
        let block = x.range.start.path.0.first().map_or(usize::MAX, |b| *b as usize);
        changes.push((block, to));
    }
    let ids: Vec<&str> =
        branch_of(&entries, i, branch).iter().filter_map(|k| entries.get(*k)).filter(|x| x.kind != Kind::Note).map(|x| x.id.as_str()).collect();
    for (block, to) in &changes {
        restyle(&mut s.doc, *block, to, by);
    }
    Ok(json!({"ids": ids, "by": by}))
}

/// Levels that skip are brought up: a paragraph is at most one level below the one above it.
fn fix_cmd(s: &mut Session, _v: &Value) -> CmdResult {
    let entries = sections::outline(&s.doc);
    let mut fixed: Vec<Value> = Vec::new();
    // The level of the nearest earlier structural paragraph, as fixed so far.
    let mut above: Option<u8> = None;
    for e in &entries {
        let Some(level) = e.level else { continue };
        match e.kind {
            Kind::Title | Kind::Part | Kind::End => {
                above = Some(level);
                continue;
            }
            Kind::Note | Kind::Other => continue,
            Kind::Article | Kind::Para => {}
        }
        let most = above.map_or(level, |a| a.saturating_add(1));
        let mut now = level;
        if level > most {
            let style = s.doc.para_at(&e.range.start).and_then(|p| p.props.style.clone()).unwrap_or_default();
            let by = i32::from(most) - i32::from(level);
            if let Some(to) = shifted_style(&s.doc, &style, by) {
                let block = e.range.start.path.0.first().map_or(usize::MAX, |b| *b as usize);
                restyle(&mut s.doc, block, &to, by);
                fixed.push(json!({"id": e.id, "from": style, "to": to}));
                now = most;
            }
        }
        above = Some(now);
    }
    Ok(json!({"fixed": fixed}))
}

/// The ids whose number the author took off.
pub fn no_number_ids(doc: &Document) -> HashSet<String> {
    doc.custom_prop(NO_NUMBER_PROP)
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .take(super::MAX_ITEMS)
        .map(str::to_string)
        .collect()
}

/// Take paragraph `i`'s automatic number off (keeping its text where the number's indent put it)
/// or give it back.
fn no_number_cmd(s: &mut Session, v: &Value) -> CmdResult {
    let entries = sections::outline(&s.doc);
    let i = target(s, v, &entries)?;
    let e = entries.get(i).ok_or_else(|| CmdError::Failed("no such paragraph".into()))?;
    let mut ids = no_number_ids(&s.doc);
    let want = match v.get("value") {
        None | Some(Value::Null) => !ids.contains(&e.id),
        Some(x) => x.as_bool().ok_or_else(|| CmdError::Params("`value` must be true or false".into()))?,
    };
    let block = e.range.start.path.0.first().map_or(usize::MAX, |b| *b as usize);
    let para = s.doc.body.get(block).and_then(|b| b.as_para()).cloned().ok_or_else(|| CmdError::Failed("no paragraph".into()))?;
    let mut props = para.props.clone();
    if want {
        if ids.contains(&e.id) {
            return Ok(json!({"id": e.id, "numbered": false}));
        }
        let resolved = s.doc.styles.resolve_para(&para.props);
        let Some(n) = resolved.numbering else {
            return Err(CmdError::Failed(format!("{:?} has no automatic number", e.id)));
        };
        // The text stays where the number's level put it.
        let indent = s.doc.numbering.level(n.num, n.level).map(|l| l.indent);
        props.numbering = Some(NumRef { num: 0, level: 0 });
        if let Some(x) = indent {
            props.indent_left = Some(x);
            props.indent_first = Some(0.0);
        }
        ids.insert(e.id.clone());
    } else {
        if !ids.remove(&e.id) {
            return Ok(json!({"id": e.id, "numbered": true}));
        }
        props.numbering = None;
        props.indent_left = None;
        props.indent_first = None;
    }
    if let Some(b) = s.doc.body.get_mut(block)
        && let Block::Para(p) = Arc::make_mut(b)
    {
        p.props = props;
        p.touch();
    }
    let mut list: Vec<&str> = entries.iter().filter(|x| ids.contains(&x.id)).map(|x| x.id.as_str()).collect();
    list.dedup();
    if list.is_empty() {
        s.doc.remove_custom_prop(NO_NUMBER_PROP);
    } else {
        s.doc.set_custom_prop(NO_NUMBER_PROP, &list.join(","));
    }
    Ok(json!({"id": e.id, "numbered": !want}))
}

/// Put the caret at the start of top-level body paragraph `block`'s text (for tests and callers).
pub fn caret_in(s: &mut Session, block: usize) {
    s.sel = Selection::caret(Pos::new(StoryRef::Body, Path::top(block), OBJ.len_utf8()));
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

    /// p0001 title, p0002 Part, p0003 Article, p0004 PR1, p0005 PR2, p0006 PR1, p0007 end.
    fn section() -> Session {
        let mut d = Document::new();
        d.body = vec![
            para("SCT", "SECTION 09 9123 - INTERIOR PAINTING"),
            para("PRT", "GENERAL"),
            para("ART", "SUMMARY"),
            para("PR1", "Section includes:"),
            para("PR2", "Surface preparation."),
            para("PR1", "Related requirements."),
            para("EOS", "END OF SECTION 09 9123"),
        ];
        let mut s = Session::new(d);
        run(&mut s, "spec.import", json!({}));
        run(&mut s, "spec.activate", json!({"ids": ["p0002", "p0003", "p0004", "p0005", "p0006"]}));
        s
    }

    fn outline(s: &mut Session) -> Vec<(String, String, String)> {
        let o = run(s, "spec.outline", json!({}));
        o["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| (i["id"].as_str().unwrap().to_string(), i["state"].as_str().unwrap().to_string(), i["text"].as_str().unwrap().to_string()))
            .collect()
    }

    fn style_of(s: &Session, id: &str) -> String {
        let e = sections::outline(&s.doc).into_iter().find(|e| e.id == id).unwrap();
        s.doc.para_at(&e.range.start).unwrap().props.style.clone().unwrap()
    }

    #[test]
    fn enter_adds_a_managed_paragraph_after_or_before() {
        let mut s = section();
        // At the end of p0006: a new paragraph after it, switched on, caret in it; typing fills it.
        let end = s.doc.body[5].as_para().unwrap().len();
        s.sel = Selection::caret(Pos::new(StoryRef::Body, Path::top(5), end));
        run(&mut s, "text.newParagraph", json!({}));
        run(&mut s, "text.insert", json!({"text": "Painting schedule."}));
        let o = outline(&mut s);
        assert_eq!(o[6], ("u0001".into(), "on".into(), "Painting schedule.".into()));
        assert_eq!(style_of(&s, "u0001"), "PR1");
        assert_eq!(o.len(), 8, "no unmanaged paragraph");
        // At the start of p0005: before it, at its level.
        caret_in(&mut s, 4);
        run(&mut s, "text.newParagraph", json!({}));
        let o = outline(&mut s);
        assert_eq!(o[4].0, "u0002");
        assert_eq!(o[5].0, "p0005");
        assert_eq!(style_of(&s, "u0002"), "PR2");
        // At the end of a paragraph with paragraphs under it: its first child.
        let end = s.doc.body[3].as_para().unwrap().len();
        s.sel = Selection::caret(Pos::new(StoryRef::Body, Path::top(3), end));
        run(&mut s, "text.newParagraph", json!({}));
        assert_eq!(style_of(&s, "u0003"), "PR2");
        // Before the end line: at the level of the paragraph above it.
        let blocks = s.doc.body.len();
        caret_in(&mut s, blocks - 1);
        run(&mut s, "text.newParagraph", json!({}));
        assert_eq!(style_of(&s, "u0004"), "PR1");
        // Undo takes one Enter back.
        run(&mut s, "edit.undo", json!({}));
        assert!(!outline(&mut s).iter().any(|r| r.0 == "u0004"));
        // Auto-Select New off: new paragraphs come in off.
        assert_eq!(run(&mut s, "spec.autoSelectNew", json!({}))["on"], false);
        caret_in(&mut s, 2);
        let end = s.doc.body[2].as_para().unwrap().len();
        s.sel = Selection::caret(Pos::new(StoryRef::Body, Path::top(2), end));
        run(&mut s, "text.newParagraph", json!({}));
        assert!(outline(&mut s).iter().any(|r| r.0 == "u0004" && r.1 == "off"));
    }

    #[test]
    fn enter_outside_spec_data_or_in_a_note_is_wordcrafts_own() {
        let mut d = Document::new();
        d.body = vec![para("Normal", "Plain."), para("Normal", "Text.")];
        let mut s = Session::new(d);
        s.sel = Selection::caret(Pos::new(StoryRef::Body, Path::top(0), 6));
        run(&mut s, "text.newParagraph", json!({}));
        assert_eq!(s.doc.body.len(), 3);
        // A selection is Enter's own too.
        let mut s = section();
        s.sel = Selection { anchor: Pos::new(StoryRef::Body, Path::top(3), 2), focus: Pos::new(StoryRef::Body, Path::top(3), 5) };
        assert!(enter(&mut s).is_none());
    }

    #[test]
    fn indent_and_outdent_move_a_branch_and_fix_hierarchy_repairs_skips() {
        let mut s = section();
        run(&mut s, "spec.indent", json!({"id": "p0004"}));
        assert_eq!((style_of(&s, "p0004"), style_of(&s, "p0005")), ("PR2".into(), "PR3".into()));
        run(&mut s, "spec.indent", json!({"id": "p0004", "by": -1}));
        assert_eq!((style_of(&s, "p0004"), style_of(&s, "p0005")), ("PR1".into(), "PR2".into()));
        // Without the branch only the paragraph moves.
        run(&mut s, "spec.indent", json!({"id": "p0006", "branch": false}));
        assert_eq!(style_of(&s, "p0006"), "PR2");
        // Articles don't go above Article, Parts and the title don't move.
        assert!(s.run("spec.indent", &json!({"id": "p0003", "by": -1})).is_err());
        assert!(s.run("spec.indent", &json!({"id": "p0002"})).is_err());
        assert!(s.run("spec.indent", &json!({"id": "p0003", "by": 2})).is_err());
        // A skip: p0005 PR2 → PR4 under a PR1; Fix Hierarchy brings it back to PR2.
        run(&mut s, "spec.indent", json!({"id": "p0005"}));
        run(&mut s, "spec.indent", json!({"id": "p0005"}));
        assert_eq!(style_of(&s, "p0005"), "PR4");
        let r = run(&mut s, "spec.fixHierarchy", json!({}));
        assert_eq!(r["fixed"], json!([{"id": "p0005", "from": "PR4", "to": "PR2"}]));
        assert_eq!(run(&mut s, "spec.fixHierarchy", json!({}))["fixed"], json!([]));
    }

    #[test]
    fn notes_between_asterisks_are_author_notes() {
        assert_eq!(sections::classify(Some("PR1"), "*Retain for projects with lead paint.*").0, Kind::Note);
        assert_eq!(sections::classify(Some("PR1"), "**Edit to suit.**").0, Kind::Note);
        assert_eq!(sections::classify(Some("PR1"), "*Starred* items are listed below.").0, Kind::Para);
        assert_eq!(sections::classify(Some("PR1"), "***").0, Kind::Para);
    }

    #[test]
    fn level_numbered_styles_shift_by_their_digit() {
        let d = Document::new();
        assert_eq!(shifted_style(&d, "CSILevel4", 1).as_deref(), Some("CSILevel5"));
        assert_eq!(shifted_style(&d, "CSILevel4", -1).as_deref(), Some("CSILevel3"));
        assert_eq!(shifted_style(&d, "CSILevel2", -1), None, "Article is the top");
        assert_eq!(shifted_style(&d, "CSILevel9", 1), None);
        assert_eq!(shifted_style(&d, "PRT", 1), None);
        assert_eq!(shifted_style(&d, "PR5", 1), None);
        assert_eq!(shifted_style(&d, "Normal", 1), None);
    }

    #[test]
    fn no_number_is_switched_and_needs_a_number() {
        let mut s = section();
        // The test styles have no numbering: nothing to take off.
        assert!(s.run("spec.noNumber", &json!({"id": "p0004"})).is_err());
        let num = {
            let n = Arc::make_mut(&mut s.doc.numbering);
            n.add_list(wordcraft_doc::numbering::ListKind::Numbered)
        };
        if let Some(Block::Para(p)) = s.doc.body.get_mut(3).map(Arc::make_mut) {
            p.props.numbering = Some(NumRef { num, level: 0 });
        }
        let r = run(&mut s, "spec.noNumber", json!({"id": "p0004"}));
        assert_eq!(r["numbered"], false);
        let p = s.doc.body[3].as_para().unwrap();
        assert_eq!(p.props.numbering, Some(NumRef { num: 0, level: 0 }));
        assert!(p.props.indent_left.is_some());
        assert_eq!(s.doc.custom_prop(NO_NUMBER_PROP), Some("p0004"));
        run(&mut s, "spec.noNumber", json!({"id": "p0004"}));
        assert_eq!(s.doc.body[3].as_para().unwrap().props.numbering, None);
        assert_eq!(s.doc.custom_prop(NO_NUMBER_PROP), None);
    }
}
