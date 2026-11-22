//! Tracking changes for review (milestone M24). Added by the Speccraft fork; not part of the
//! original WordCraft.
//!
//! A review round starts from a baseline: which paragraphs printed then ([`REVIEW_PROP`]).
//! While it runs, WordCraft's Track Changes records the wording changes (additions underlined,
//! deletions struck), and Speccraft shows which paragraphs were switched on or off since the
//! baseline (a blue bar by the sequence number). Drafts can print with this markup (print option
//! `markup`). Starting a new round takes the section as it is now as the baseline.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use wordcraft_doc::props::{Border, BorderStyle};
use wordcraft_doc::{Block, Document, Rgb};

use super::sections::{self, Kind};
use crate::{CmdError, CmdResult, CommandSpec, Session, p};

const LOC: &str = "Specs › Review";
/// Document property: the review round's baseline.
pub const REVIEW_PROP: &str = "Speccraft.Review";

/// A review round's baseline.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Baseline {
    /// When it was taken ("2026-11-21").
    #[serde(default)]
    pub since: String,
    /// The paragraphs that printed then.
    #[serde(default)]
    pub on: BTreeSet<String>,
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("spec.review.track", "Track Changes for Review", LOC, track_cmd).params(
            r#"{"value"?: bool (default: the opposite of now; on starts a round if none is running), "date"?: string (the round's date, default today)}"#,
        ),
        CommandSpec::new("spec.review.newRound", "New Review Round", LOC, new_round_cmd).params(
            r#"{"date"?: string, "acceptChanges"?: bool (also accept the tracked wording changes)}"#,
        ),
        CommandSpec::new("spec.review.changes", "Changes This Round", LOC, changes_cmd).pure(),
    ]
}

/// The running round's baseline, if any.
pub fn baseline(doc: &Document) -> Option<Baseline> {
    doc.custom_prop(REVIEW_PROP).and_then(|v| serde_json::from_str(v).ok())
}

fn printing(doc: &Document) -> BTreeSet<String> {
    sections::outline(doc).into_iter().filter(|e| e.on && e.kind != Kind::Note).map(|e| e.id).collect()
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn date_param(v: &Value) -> Result<String, CmdError> {
    match p::str(v, "date") {
        None => Ok(today()),
        Some(d) if d.chars().count() <= 40 && !d.contains(['\n', '\r']) => Ok(d.trim().to_string()),
        Some(_) => Err(CmdError::Params("`date` is one line of at most 40 characters".into())),
    }
}

fn set_baseline(doc: &mut Document, date: String) -> Result<(), CmdError> {
    let b = Baseline { since: date, on: printing(doc) };
    doc.set_custom_prop(REVIEW_PROP, &serde_json::to_string(&b).map_err(|e| CmdError::Failed(e.to_string()))?);
    Ok(())
}

fn track_cmd(s: &mut Session, v: &Value) -> CmdResult {
    let want = p::bool(v, "value").unwrap_or(!s.doc.settings.track_changes);
    s.doc.settings.track_changes = want;
    if want && baseline(&s.doc).is_none() {
        set_baseline(&mut s.doc, date_param(v)?)?;
    }
    Ok(json!({"tracking": want, "since": baseline(&s.doc).map(|b| b.since)}))
}

fn new_round_cmd(s: &mut Session, v: &Value) -> CmdResult {
    if p::bool(v, "acceptChanges") == Some(true) {
        s.run("review.acceptAll", &json!({}))?;
    }
    let date = date_param(v)?;
    set_baseline(&mut s.doc, date.clone())?;
    Ok(json!({"since": date}))
}

/// Paragraphs switched on and off since the round's baseline: (on, off).
pub fn switched(doc: &Document) -> Option<(Vec<String>, Vec<String>)> {
    let b = baseline(doc)?;
    let now = printing(doc);
    Some((now.difference(&b.on).cloned().collect(), b.on.difference(&now).cloned().collect()))
}

/// Wording changes tracked in the body: distinct insertions and deletions.
pub fn revisions(doc: &Document) -> usize {
    let mut ids = BTreeSet::new();
    for b in &doc.body {
        if let Some(p) = b.as_para() {
            for r in &p.runs {
                if let Some(i) = r.props.ins {
                    ids.insert((true, i));
                }
                if let Some(d) = r.props.del {
                    ids.insert((false, d));
                }
            }
        }
    }
    ids.len()
}

fn changes_cmd(s: &mut Session, _v: &Value) -> CmdResult {
    let b = baseline(&s.doc);
    let (on, off) = switched(&s.doc).unwrap_or_default();
    Ok(json!({
        "tracking": s.doc.settings.track_changes,
        "since": b.map(|b| b.since),
        "switchedOn": on,
        "switchedOff": off,
        "revisions": revisions(&s.doc),
    }))
}

/// Print-time, for a copy with markup: paragraphs switched on this round get a change bar in the
/// left margin. (Paragraphs switched off don't print, so they show no change.)
pub fn bar_switched_on(copy: &mut Document, switched_on: &[String]) {
    if switched_on.is_empty() {
        return;
    }
    let bar = Border { style: BorderStyle::Single, width: 1.5, color: Rgb::parse("1F6FD1"), space: 4.0 };
    for e in sections::outline(copy) {
        if !switched_on.contains(&e.id) {
            continue;
        }
        let Some(b) = e.range.start.path.0.first().map(|b| *b as usize) else { continue };
        if let Some(block) = copy.body.get_mut(b)
            && let Block::Para(p) = std::sync::Arc::make_mut(block)
        {
            let mut borders = p.props.borders.unwrap_or_default();
            borders.left = Some(bar);
            p.props.borders = Some(borders);
            p.touch();
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

    fn section() -> Session {
        let mut d = Document::new();
        d.body = vec![
            para("SCT", "SECTION 09 2900 - GYPSUM BOARD"),
            para("PRT", "GENERAL"),
            para("ART", "SUMMARY"),
            para("PR1", "Gypsum board."),
            para("PR1", "Joint treatment."),
            para("EOS", "END OF SECTION 09 2900"),
        ];
        let mut s = Session::new(d);
        run(&mut s, "spec.import", json!({}));
        run(&mut s, "spec.activate", json!({"ids": ["p0002", "p0003", "p0004"]}));
        s
    }

    #[test]
    fn a_review_round_shows_what_was_switched_and_reworded() {
        let mut s = section();
        let r = run(&mut s, "spec.review.track", json!({"value": true, "date": "2026-11-21"}));
        assert_eq!((r["tracking"].clone(), r["since"].clone()), (json!(true), json!("2026-11-21")));
        assert!(s.doc.settings.track_changes);
        run(&mut s, "spec.activate", json!({"id": "p0005"}));
        run(&mut s, "spec.deactivate", json!({"id": "p0004"}));
        // A tracked wording change.
        s.sel = crate::Selection::caret(wordcraft_doc::Pos::new(
            wordcraft_doc::StoryRef::Body,
            wordcraft_doc::Path::top(4),
            wordcraft_doc::para::OBJ.len_utf8(),
        ));
        run(&mut s, "text.insert", json!({"text": "Level 4 "}));
        let c = run(&mut s, "spec.review.changes", json!({}));
        assert_eq!((c["switchedOn"].clone(), c["switchedOff"].clone()), (json!(["p0005"]), json!(["p0004"])));
        assert_eq!(c["revisions"], 1);
        // Switching tracking off and on keeps the round.
        run(&mut s, "spec.review.track", json!({"value": false}));
        run(&mut s, "spec.review.track", json!({"value": true, "date": "2027-01-01"}));
        assert_eq!(baseline(&s.doc).unwrap().since, "2026-11-21");
        // A new round, accepting the wording: nothing changed since.
        run(&mut s, "spec.review.newRound", json!({"date": "2026-12-01", "acceptChanges": true}));
        let c = run(&mut s, "spec.review.changes", json!({}));
        assert_eq!((c["switchedOn"].clone(), c["revisions"].clone(), c["since"].clone()), (json!([]), json!(0), json!("2026-12-01")));
        assert!(s.run("spec.review.newRound", &json!({"date": "a\nb"})).is_err());
    }

    #[test]
    fn switched_on_paragraphs_get_a_change_bar_when_printed_with_markup() {
        let mut s = section();
        run(&mut s, "spec.review.track", json!({"value": true}));
        run(&mut s, "spec.activate", json!({"id": "p0005"}));
        let (on, _) = switched(&s.doc).unwrap();
        let mut copy = s.doc.clone();
        bar_switched_on(&mut copy, &on);
        let p5 = copy.body[4].as_para().unwrap();
        assert!(p5.props.borders.as_ref().and_then(|b| b.left.as_ref()).is_some());
        assert!(copy.body[3].as_para().unwrap().props.borders.is_none());
    }
}
