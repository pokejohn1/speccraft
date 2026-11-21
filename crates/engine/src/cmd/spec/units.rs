//! Units of measure in a section (milestone M19). Added by the Speccraft fork; not part of the
//! original WordCraft.
//!
//! Masters write measurements in US units with the metric equivalent in brackets after them
//! ("4 inches (100 mm)"). [`UNITS_PROP`] says which the section prints: both (the default),
//! US only (the bracketed metric goes) or metric only (the metric replaces the US). A fill-in
//! that is a measurement is shown in its own colour ([`is_measure`]).

use serde_json::{Value, json};
use wordcraft_doc::para::OBJ;
use wordcraft_doc::{Document, Path, Pos, StoryRef};

use crate::{CmdError, CmdResult, CommandSpec, Session};

const LOC: &str = "Specs › Fields";
/// Document property: `us` or `metric`; absent prints both.
pub const UNITS_PROP: &str = "Speccraft.Units";
/// Longest measurement looked at, in characters ("60,000 psi", "1-1/2 inches").
const MAX_MEASURE: usize = 32;
/// Most replacements in one paragraph (bounded work for a hostile document).
const MAX_PER_PARA: usize = 200;

const US: [&str; 22] = [
    "inch", "inches", "in", "in.", "ft", "ft.", "foot", "feet", "psi", "ksi", "lb", "lbs", "lb.", "pound", "pounds", "gage", "gauge", "mil", "mils",
    "deg", "°f", "gal",
];
const METRIC: [&str; 16] = ["mm", "cm", "m", "km", "mpa", "kpa", "pa", "kg", "g", "kn", "n", "l", "ml", "°c", "micrometers", "microns"];

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("spec.units", "Units of Measure", LOC, units_cmd)
            .params(r#"{"value"?: "both" | "us" | "metric" (default: say which is set)}"#),
    ]
}

/// Which units print.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Units {
    Both,
    Us,
    Metric,
}

impl Units {
    pub fn name(self) -> &'static str {
        match self {
            Units::Both => "both",
            Units::Us => "us",
            Units::Metric => "metric",
        }
    }
}

/// The section's units setting.
pub fn units(doc: &Document) -> Units {
    match doc.custom_prop(UNITS_PROP).map(|v| v.trim().to_ascii_lowercase()) {
        Some(v) if v == "us" => Units::Us,
        Some(v) if v == "metric" => Units::Metric,
        _ => Units::Both,
    }
}

fn units_cmd(s: &mut Session, v: &Value) -> CmdResult {
    match v.get("value").and_then(Value::as_str) {
        None => {}
        Some("both") => {
            s.doc.remove_custom_prop(UNITS_PROP);
        }
        Some(x @ ("us" | "metric")) => s.doc.set_custom_prop(UNITS_PROP, x),
        Some(_) => return Err(CmdError::Params("`value` must be \"both\", \"us\" or \"metric\"".into())),
    }
    Ok(json!({"units": units(&s.doc).name()}))
}

/// The unit word after a number in `t` (lower case, at most a few characters), if `t` is a
/// number followed by a unit: "1/2 inch", "60,000 psi", "100 mm", "90 deg F".
fn unit_of(t: &str) -> Option<String> {
    let t = t.trim();
    if !t.chars().next().is_some_and(|c| c.is_ascii_digit()) || t.chars().count() > MAX_MEASURE {
        return None;
    }
    let rest = t.trim_start_matches(|c: char| c.is_ascii_digit() || matches!(c, ',' | '.' | '/' | '-' | ' '));
    let words: Vec<String> = rest.split_whitespace().map(str::to_lowercase).collect();
    match words.as_slice() {
        // "deg F" / "deg C".
        [d, x] if d == "deg" && (x == "f" || x == "c") => Some(format!("°{x}")),
        [w] => Some(w.clone()),
        _ => None,
    }
}

fn is_us(t: &str) -> bool {
    unit_of(t).is_some_and(|u| US.contains(&u.as_str()))
}

fn is_metric(t: &str) -> bool {
    unit_of(t).is_some_and(|u| METRIC.contains(&u.as_str()))
}

/// Whether `t` holds a measurement: a number with a unit, anywhere in it ("Grade 60 (420 MPa)").
pub fn is_measure(t: &str) -> bool {
    let mut prev: Option<char> = None;
    for (i, c) in t.char_indices() {
        if c.is_ascii_digit() && !prev.is_some_and(|p| p.is_ascii_digit() || matches!(p, ',' | '.' | '/' | '-')) {
            let piece: String = t.get(i..).unwrap_or("").chars().take(MAX_MEASURE).collect();
            // The shortest prefix that reads as a number and a unit.
            let mut end = 0;
            for (k, ch) in piece.char_indices() {
                end = k + ch.len_utf8();
                if ch.is_alphabetic() || ch == '°' {
                    let word_end = piece.get(k..).and_then(|r| r.find([' ', ',', ';', ')', '(']).map(|e| k + e)).unwrap_or(piece.len());
                    end = word_end;
                    break;
                }
            }
            if let Some(p) = piece.get(..end)
                && (is_us(p) || is_metric(p))
            {
                return true;
            }
        }
        prev = Some(c);
    }
    false
}

/// The edits that make `text` print in `units`: (byte range, replacement), back to front.
/// "4 inches (100 mm)": US only drops " (100 mm)"; metric only makes it "100 mm".
pub fn edits(text: &str, units: Units) -> Vec<(std::ops::Range<usize>, String)> {
    if units == Units::Both {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(open) = text.get(from..).and_then(|r| r.find(" (")).map(|k| k + from) {
        if out.len() >= MAX_PER_PARA {
            break;
        }
        let inner_start = open + 2;
        let Some(close) = text.get(inner_start..).and_then(|r| r.find(')')).map(|k| k + inner_start) else { break };
        from = close + 1;
        let Some(inner) = text.get(inner_start..close) else { continue };
        if inner.contains(OBJ) || !is_metric(inner) {
            continue;
        }
        if units == Units::Us {
            out.push((open..close + 1, String::new()));
            continue;
        }
        // The US measurement just before: the longest stretch ending at `open` that reads as one.
        let mut start = None;
        for (i, _) in text.char_indices().filter(|(i, _)| *i < open && open - *i <= MAX_MEASURE) {
            let Some(cand) = text.get(i..open) else { continue };
            let before = text.get(..i).and_then(|b| b.chars().next_back());
            if cand.starts_with(|c: char| c.is_ascii_digit())
                && is_us(cand)
                && !before.is_some_and(|c| c.is_ascii_digit() || matches!(c, ',' | '.' | '/' | '-'))
                && !cand.contains(OBJ)
            {
                start = Some(i);
                break;
            }
        }
        let Some(start) = start else { continue };
        out.push((start..close + 1, inner.trim().to_string()));
    }
    out.reverse();
    out
}

/// Print-time: make every top-level body paragraph print in the section's units.
pub fn apply(doc: &mut Document) {
    let u = units(doc);
    if u == Units::Both {
        return;
    }
    for b in 0..doc.body.len() {
        let Some(text) = doc.body.get(b).and_then(|x| x.as_para()).map(|p| p.text.clone()) else { continue };
        for (range, with) in edits(&text, u) {
            let at = |off: usize| Pos::new(StoryRef::Body, Path::top(b), off);
            let props = doc.body.get(b).and_then(|x| x.as_para()).map(|p| p.props_of_char(range.start).clone()).unwrap_or_default();
            if doc.delete_range(&at(range.start), &at(range.end)).is_err() {
                continue;
            }
            if !with.is_empty() {
                let _ = doc.insert_text(&at(range.start), &with, &props);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply_to(text: &str, u: Units) -> String {
        let mut t = text.to_string();
        for (r, with) in edits(text, u) {
            t.replace_range(r, &with);
        }
        t
    }

    #[test]
    fn measurements_are_recognised() {
        for t in ["1/2 inch", "Grade 60 (420 MPa)", "60,000 psi", "above 50 deg F", "100 mm", "4-1/2 inches thick", "16 gage"] {
            assert!(is_measure(t), "{t}");
        }
        for t in ["Section 09 2900", "Type X", "2 coats", "ASTM C 1396", "Grade 60"] {
            assert!(!is_measure(t), "{t}");
        }
    }

    #[test]
    fn us_or_metric_only() {
        let t = "Studs 4 inches (100 mm) deep, Grade 60 (420 MPa), at 16 inches (406 mm) o.c. (see drawings).";
        assert_eq!(apply_to(t, Units::Both), t);
        assert_eq!(apply_to(t, Units::Us), "Studs 4 inches deep, Grade 60, at 16 inches o.c. (see drawings).");
        assert_eq!(apply_to(t, Units::Metric), "Studs 100 mm deep, Grade 60 (420 MPa), at 406 mm o.c. (see drawings).");
        assert_eq!(apply_to("1-1/2 inches (38 mm)", Units::Metric), "38 mm");
        assert_eq!(apply_to("60,000 psi (420 MPa)", Units::Metric), "420 MPa");
    }

    #[test]
    fn the_setting_is_kept_in_the_document() {
        let mut s = Session::new(Document::new());
        assert_eq!(s.run("spec.units", &json!({})).unwrap()["units"], "both");
        assert_eq!(s.run("spec.units", &json!({"value": "us"})).unwrap()["units"], "us");
        assert_eq!(units(&s.doc), Units::Us);
        assert!(s.run("spec.units", &json!({"value": "imperial"})).is_err());
        s.run("spec.units", &json!({"value": "both"})).unwrap();
        assert_eq!(s.doc.custom_prop(UNITS_PROP), None);
    }
}
