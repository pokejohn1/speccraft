//! Specs › Check: quality checks of one section (milestone M10). Added by the Speccraft fork; not
//! part of the original WordCraft.
//!
//! [`check`] looks at what will print and reports what a reviewer would mark up before issue:
//! fill-ins still open, the section's rule findings, headings with nothing under them, a missing
//! Part, drafting notes and editing marks left in printing text, the same paragraph twice under one
//! heading, and an end-of-section line that is missing or names another number. Every finding
//! names the paragraph it is about (`para`), so a report can take the author there.
//! `spec.project.validate` runs the same checks for every section of a project.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::{Value, json};
use wordcraft_doc::Document;
use wordcraft_doc::para::OBJ;

use super::fields::{self, FieldState};
use super::project::{filled_by_project, number_key, split_title};
use super::sections::{self, Entry, Kind};
use crate::{CmdResult, CommandSpec, Session};

const LOC: &str = "Specs › Check";
/// Most findings of one kind reported for one section.
const MAX_EACH: usize = 50;

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec::new("spec.check", "Check Section", LOC, check_cmd).params(r#"{}"#).pure()]
}

/// One finding: `severity` is `error`, `warning` or `notice`; `para` the paragraph it is about.
fn finding(severity: &str, code: &str, para: Option<&str>, message: String) -> Value {
    json!({"severity": severity, "code": code, "para": para, "message": message})
}

fn text_of(doc: &Document, e: &Entry) -> String {
    let i = e.range.start.path.0.first().copied().unwrap_or(u32::MAX) as usize;
    doc.body.get(i).and_then(|b| b.as_para()).map(|p| p.plain_text().replace(OBJ, "")).unwrap_or_default()
}

fn block_of(e: &Entry) -> Option<u32> {
    e.range.start.path.0.first().copied()
}

/// Words that mark text meant for the specifier rather than the contractor.
fn drafting_note(text: &str) -> bool {
    let low = text.trim().to_lowercase();
    low.contains("specifier")
        || low.contains("note to ")
        || low.starts_with("retain ")
        || low.starts_with("delete ")
        || low.starts_with("edit ")
        || low.starts_with("revise ")
        || low.starts_with("coordinate with ")
        || low.contains("[note")
}

/// Editing marks left in text: "XXX", "***", "??", "TBD".
fn editing_mark(text: &str) -> Option<&'static str> {
    let up = text.to_uppercase();
    let word = |w: &str| up.split(|c: char| !c.is_ascii_alphanumeric()).any(|x| x == w);
    if up.contains("XXX") {
        Some("XXX")
    } else if up.contains("***") {
        Some("***")
    } else if up.contains("??") {
        Some("??")
    } else if word("TBD") {
        Some("TBD")
    } else {
        None
    }
}

/// Square-bracketed text, such as `[Owner]`, as (start char, the text with brackets).
fn brackets(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut open: Option<usize> = None;
    for (i, c) in text.char_indices() {
        match c {
            '[' => open = Some(i),
            ']' => {
                if let Some(s) = open.take()
                    && let Some(t) = text.get(s..i + c.len_utf8())
                {
                    out.push(t.to_string());
                }
            }
            _ => {}
        }
    }
    out
}

/// The quality findings of one section. `values` are the project's named values (lower-case
/// names): fill-ins they fill are not open.
pub fn check(doc: &Document, values: &BTreeMap<String, String>) -> Vec<Value> {
    let entries = sections::outline(doc);
    let mut out = Vec::new();
    if entries.is_empty() {
        return out;
    }
    let by_block: HashMap<u32, &str> = entries.iter().filter_map(|e| block_of(e).map(|b| (b, e.id.as_str()))).collect();
    let printing_blocks: HashSet<u32> = entries.iter().filter(|e| e.on).filter_map(block_of).collect();
    let texts: Vec<String> = entries.iter().map(|e| text_of(doc, e)).collect();
    fill_ins(doc, values, &by_block, &printing_blocks, &mut out);
    rule_findings(doc, &mut out);
    if !entries.iter().any(|e| e.on && matches!(e.kind, Kind::Para | Kind::Article | Kind::Other)) {
        // Nothing chosen yet: one finding says so, rather than one per missing heading.
        out.push(finding("warning", "nothingPrints", None, "nothing in this section is switched on yet".into()));
        return out;
    }
    headings(&entries, &texts, &mut out);
    leftovers(doc, &entries, &texts, &by_block, &mut out);
    duplicates(&entries, &texts, &mut out);
    section_end(&entries, &texts, &mut out);
    out
}

/// Fill-ins still open in printing paragraphs.
fn fill_ins(doc: &Document, values: &BTreeMap<String, String>, by_block: &HashMap<u32, &str>, printing: &HashSet<u32>, out: &mut Vec<Value>) {
    let open = fields::fields(doc)
        .into_iter()
        .filter(|f| f.state == FieldState::Open && f.range.start.path.0.first().is_some_and(|b| printing.contains(b)))
        .filter(|f| !filled_by_project(&f.original, values));
    for f in open.take(MAX_EACH * 4) {
        let para = f.range.start.path.0.first().and_then(|b| by_block.get(b)).copied();
        out.push(finding("warning", "fieldOpen", para, format!("fill-in {} is still open: {}", f.id, f.original)));
    }
}

/// The section's own rule findings (`spec.rules.check`).
fn rule_findings(doc: &Document, out: &mut Vec<Value>) {
    let mut probe = Session::new(doc.clone());
    let Ok(r) = probe.run("spec.rules.check", &json!({})) else { return };
    for x in r.get("findings").and_then(Value::as_array).into_iter().flatten() {
        let sev = x.get("severity").and_then(Value::as_str).unwrap_or("notice");
        let m = x.get("message").and_then(Value::as_str).unwrap_or("");
        out.push(finding(sev, "sectionRule", x.get("target").and_then(Value::as_str), m.to_string()));
    }
}

/// Printing Parts and Articles with nothing printing under them; a three-Part section missing a Part.
fn headings(entries: &[Entry], texts: &[String], out: &mut Vec<Value>) {
    // Which entries have a printing, non-heading entry somewhere under them.
    let mut filled = vec![false; entries.len()];
    for (i, e) in entries.iter().enumerate() {
        if !e.on || matches!(e.kind, Kind::Part | Kind::Article | Kind::Title | Kind::End | Kind::Note) {
            continue;
        }
        let mut p = entries.get(i).and_then(|e| e.parent);
        let mut guard = 0;
        while let Some(k) = p {
            if let Some(f) = filled.get_mut(k) {
                *f = true;
            }
            p = entries.get(k).and_then(|e| e.parent);
            guard += 1;
            if guard > 64 {
                break;
            }
        }
    }
    let mut empty = 0;
    for (i, e) in entries.iter().enumerate() {
        if e.on && matches!(e.kind, Kind::Article) && !filled.get(i).copied().unwrap_or(true) && empty < MAX_EACH {
            empty += 1;
            out.push(finding(
                "warning",
                "emptyHeading",
                Some(&e.id),
                format!("{} has nothing printing under it", texts.get(i).map_or("", |t| t.trim())),
            ));
        }
        // A Part needs an Article (or text) under it, except one marked "(Not Used)".
        let has_child = entries.iter().any(|c| c.on && c.parent == Some(i) && c.kind != Kind::Note);
        let text = texts.get(i).map_or("", |t| t.trim());
        if e.on && e.kind == Kind::Part && !has_child && !text.to_lowercase().contains("not used") {
            out.push(finding("warning", "emptyHeading", Some(&e.id), format!("PART {text} has nothing printing under it")));
        }
    }
    // The three Parts of a section, when the section uses them.
    let parts: Vec<(usize, String)> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.kind == Kind::Part)
        .map(|(i, _)| (i, texts.get(i).map_or(String::new(), |t| t.trim().to_uppercase())))
        .collect();
    let three = ["GENERAL", "PRODUCTS", "EXECUTION"];
    if parts.iter().any(|(_, t)| three.iter().any(|w| t.starts_with(w))) {
        for w in three {
            match parts.iter().find(|(_, t)| t.starts_with(w)) {
                None => out.push(finding("warning", "missingPart", None, format!("the section has no PART {w}"))),
                Some((i, _)) if entries.get(*i).is_some_and(|e| !e.on) => out.push(finding(
                    "warning",
                    "missingPart",
                    entries.get(*i).map(|e| e.id.as_str()),
                    format!("PART {w} is switched off: keep it, marked (Not Used) if empty"),
                )),
                Some(_) => {}
            }
        }
    }
}

/// Drafting notes, editing marks and bracketed text left in printing paragraphs.
fn leftovers(doc: &Document, entries: &[Entry], texts: &[String], by_block: &HashMap<u32, &str>, out: &mut Vec<Value>) {
    // Bracketed text that is an open fill-in is reported with the fill-ins.
    let mut field_text: HashMap<String, Vec<String>> = HashMap::new();
    for f in fields::fields(doc) {
        if let Some(p) = f.range.start.path.0.first().and_then(|b| by_block.get(b)) {
            field_text.entry(p.to_string()).or_default().push(f.value);
        }
    }
    let (mut notes, mut marks, mut bracketed) = (0, 0, 0);
    for (e, text) in entries.iter().zip(texts) {
        if !e.on || matches!(e.kind, Kind::Note) {
            continue;
        }
        let short: String = text.trim().chars().take(80).collect();
        if drafting_note(text) && notes < MAX_EACH {
            notes += 1;
            out.push(finding("warning", "draftingNote", Some(&e.id), format!("reads like a note to the specifier, but prints: {short}")));
        }
        if let Some(m) = editing_mark(text)
            && marks < MAX_EACH
        {
            marks += 1;
            out.push(finding("warning", "editingMark", Some(&e.id), format!("{m} left in the text: {short}")));
        }
        // Fill-ins are reported with the fill-ins, and `<Insert …>` with the placeholders.
        let left: Vec<String> = brackets(text)
            .into_iter()
            .filter(|b| !field_text.get(&e.id).is_some_and(|v| v.iter().any(|x| x.contains(b.as_str()))))
            .filter(|b| !b.to_lowercase().contains("<insert"))
            .collect();
        if !left.is_empty() && bracketed < MAX_EACH {
            bracketed += 1;
            out.push(finding("warning", "bracketText", Some(&e.id), format!("still in brackets: {} (choose, fill in or remove)", left.join(" "))));
        }
    }
}

/// The same paragraph twice under one heading.
fn duplicates(entries: &[Entry], texts: &[String], out: &mut Vec<Value>) {
    let norm = |t: &str| t.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    let mut seen: HashMap<(Option<usize>, String), &str> = HashMap::new();
    let mut n = 0;
    for (e, text) in entries.iter().zip(texts) {
        if !e.on || !matches!(e.kind, Kind::Para | Kind::Other) {
            continue;
        }
        let key = (e.parent, norm(text));
        if key.1.chars().count() < 12 {
            continue;
        }
        match seen.get(&key) {
            Some(first) if n < MAX_EACH => {
                n += 1;
                out.push(finding("notice", "duplicateText", Some(&e.id), format!("says the same as {first} under the same heading")));
            }
            Some(_) => {}
            None => {
                seen.insert(key, &e.id);
            }
        }
    }
}

/// The end-of-section line: present, printing, and naming the section's own number.
fn section_end(entries: &[Entry], texts: &[String], out: &mut Vec<Value>) {
    let title = entries.iter().zip(texts).find(|(e, _)| e.kind == Kind::Title).map(|(_, t)| split_title(t).0).unwrap_or_default();
    let Some((end, text)) = entries.iter().zip(texts).find(|(e, _)| e.kind == Kind::End) else {
        out.push(finding("notice", "noEnd", None, "the section has no END OF SECTION line".into()));
        return;
    };
    if !end.on {
        out.push(finding("notice", "noEnd", Some(&end.id), "END OF SECTION is switched off".into()));
    }
    let digits: String = text.chars().filter(char::is_ascii_digit).collect();
    if !digits.is_empty() && !number_key(&title).is_empty() && digits != number_key(&title) {
        out.push(finding("warning", "endNumber", Some(&end.id), format!("{} doesn't match the title, Section {title}", text.trim())));
    }
}

fn check_cmd(s: &mut Session, _: &Value) -> CmdResult {
    let out = check(&s.doc, &BTreeMap::new());
    let count = |sev: &str| out.iter().filter(|x| x["severity"] == sev).count();
    Ok(json!({"errors": count("error"), "warnings": count("warning"), "notices": count("notice"), "findings": out}))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::{Value, json};
    use wordcraft_doc::{Block, Document, Paragraph};

    use crate::Session;

    fn para(style: &str, text: &str) -> Arc<Block> {
        Arc::new(Block::Para(Paragraph::with_text(text, Default::default()).styled(style)))
    }

    fn section(body: &[(&str, &str)]) -> Session {
        let mut d = Document::new();
        d.body = body.iter().map(|(s, t)| para(s, t)).collect();
        let mut s = Session::new(d);
        s.run("spec.import", &json!({})).unwrap();
        // Everything but notes on, as an author would have it before issue.
        let ids: Vec<String> = (1..=body.len()).filter(|i| body.get(i - 1).is_some_and(|(st, _)| *st != "CMT")).map(|i| format!("p{i:04}")).collect();
        s.run("spec.activate", &json!({"ids": ids})).unwrap();
        s
    }

    fn codes(s: &mut Session) -> Vec<(String, Value)> {
        let r = s.run("spec.check", &json!({})).unwrap();
        r["findings"].as_array().unwrap().iter().map(|f| (f["code"].as_str().unwrap().to_string(), f["para"].clone())).collect()
    }

    #[test]
    fn a_clean_section_has_no_findings() {
        let mut s = section(&[
            ("SCT", "SECTION 09 2900 - GYPSUM BOARD"),
            ("PRT", "GENERAL"),
            ("ART", "SUMMARY"),
            ("PR1", "Section includes gypsum board."),
            ("CMT", "Retain this note: notes never print, so they are not findings."),
            ("PRT", "PRODUCTS (Not Used)"),
            ("PRT", "EXECUTION"),
            ("ART", "INSTALLATION"),
            ("PR1", "Install panels to comply with ASTM C840."),
            ("EOS", "END OF SECTION 09 2900"),
        ]);
        assert_eq!(codes(&mut s), []);
    }

    #[test]
    fn leftovers_empty_headings_and_endings_are_found_with_their_paragraphs() {
        let mut s = section(&[
            ("SCT", "SECTION 09 2900 - GYPSUM BOARD"),
            ("PRT", "GENERAL"),
            ("ART", "SUMMARY"),
            ("PR1", "Retain this paragraph if the Project has wet areas."),
            ("PR1", "Provide XXX panels."),
            ("PR1", "Provide panels from [Manufacturer]."),
            ("PR1", "Provide panels in all rooms."),
            ("PR1", "Provide  panels in all rooms."),
            ("ART", "SUBMITTALS"),
            ("PRT", "EXECUTION"),
            ("ART", "INSTALLATION"),
            ("PR1", "Install panels."),
            ("EOS", "END OF SECTION 09 2116"),
        ]);
        let found = codes(&mut s);
        let has = |c: &str, p: &str| found.iter().any(|(code, para)| code == c && para == p);
        assert!(has("draftingNote", "p0004"), "{found:?}");
        assert!(has("editingMark", "p0005"), "{found:?}");
        assert!(has("duplicateText", "p0008"), "{found:?}");
        assert!(has("emptyHeading", "p0009"), "SUBMITTALS has nothing under it: {found:?}");
        assert!(has("endNumber", "p0013"), "{found:?}");
        assert!(found.iter().any(|(c, p)| c == "missingPart" && p.is_null()), "no PART PRODUCTS: {found:?}");
        // "[Manufacturer]" became a fill-in when the section was imported? Only in a project copy;
        // here it is bracketed text.
        assert!(has("bracketText", "p0006") || has("fieldOpen", "p0006"), "{found:?}");
        // Switched-off paragraphs are not checked.
        s.run("spec.toggle", &json!({"id": "p0005"})).unwrap();
        assert!(!codes(&mut s).iter().any(|(c, _)| c == "editingMark"));
    }

    #[test]
    fn open_fill_ins_and_rule_findings_name_their_paragraph() {
        let mut s = section(&[
            ("SCT", "SECTION 07 9200 - JOINT SEALANTS"),
            ("PRT", "GENERAL"),
            ("ART", "SUMMARY"),
            ("PR1", "Provide [silicone] [urethane] sealant."),
            ("PR1", "Provide backer rod."),
            ("PRT", "PRODUCTS"),
            ("ART", "SEALANTS"),
            ("PR1", "Sealant colour: <Insert colour>."),
            ("PRT", "EXECUTION"),
            ("ART", "INSTALLATION"),
            ("PR1", "Install sealant."),
            ("EOS", "END OF SECTION 07 9200"),
        ]);
        s.run("spec.fields.detect", &json!({})).unwrap();
        s.run("spec.rules.add", &json!({"kind": "warn", "if": [{"on": "p0004"}], "target": "p0005", "note": "check"})).unwrap();
        let found = codes(&mut s);
        let fields: Vec<&Value> = found.iter().filter(|(c, _)| c == "fieldOpen").map(|(_, p)| p).collect();
        assert_eq!(fields, [&json!("p0004"), &json!("p0008")], "{found:?}");
        assert!(found.iter().any(|(c, p)| c == "sectionRule" && p == "p0005"), "{found:?}");
        assert!(!found.iter().any(|(c, _)| c == "bracketText"), "fill-ins are not reported twice: {found:?}");
    }

    #[test]
    fn a_section_with_nothing_chosen_says_so_once() {
        let mut d = Document::new();
        d.body =
            [("SCT", "SECTION 09 2900 - GYPSUM BOARD"), ("PRT", "GENERAL"), ("ART", "SUMMARY"), ("PR1", "Text."), ("EOS", "END OF SECTION 09 2900")]
                .iter()
                .map(|(s, t)| para(s, t))
                .collect();
        let mut s = Session::new(d);
        s.run("spec.import", &json!({})).unwrap();
        assert_eq!(codes(&mut s), [("nothingPrints".to_string(), Value::Null)]);
    }

    #[test]
    fn helpers() {
        assert_eq!(super::brackets("a [b] c [d e] [f"), ["[b]", "[d e]"]);
        assert_eq!(super::editing_mark("cost tbd later"), Some("TBD"));
        assert_eq!(super::editing_mark("Outbound"), None);
        assert!(super::drafting_note("Retain for wet areas"));
        assert!(!super::drafting_note("Retaining walls are excluded."));
    }
}
