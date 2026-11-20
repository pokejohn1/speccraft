//! Specs › Rules: coordination between spec paragraphs (milestone M6). Added by the Speccraft
//! fork; not part of the original WordCraft.
//!
//! A rule says what follows when its conditions hold. Conditions: a paragraph prints
//! (`{"on": "p0042"}`), doesn't print (`{"off": …}`), or a fill-in is set (`{"field": "f0003"}`),
//! optionally to a value (`{"field": "f0003", "is": "Type X"}`). All of a rule's conditions must
//! hold. Kinds, as SpecLink's coloured links:
//!
//! | Kind | When its conditions hold | SpecLink |
//! |---|---|---|
//! | `require` | the target (and the paragraphs it sits under) comes on by itself | green |
//! | `recommend` | the target is suggested if it is off | yellow |
//! | `warn` | the target is flagged if it is on | red |
//! | `exclude` | the target can't be switched on, nor (for an `on` condition) the other way round; a rule's own links don't turn it on | red |
//!
//! **The author has the last say.** Switching off a paragraph a rule turned on overrides the rule
//! (kept in `Speccraft.Overridden`) and is reported, never undone; switching it on again lifts the
//! override. Rules live in the document property `Speccraft.Rules`, apart from the text, so a
//! master's rules travel into every project copy. Rules between sections live in the project
//! file (see [`super::project`]).

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use wordcraft_doc::Document;

use super::fields;
use super::sections::{self, Entry, Kind};
use crate::{CmdError, CmdResult, CommandSpec, Session, p};

const LOC: &str = "Specs › Rules";
/// Document property: the section's rules, JSON.
pub const RULES_PROP: &str = "Speccraft.Rules";
/// Most rules in one section.
const MAX_RULES: usize = 2_000;
/// Most conditions in one rule.
const MAX_CONDITIONS: usize = 20;
/// Longest note on a rule, in characters.
const MAX_NOTE: usize = 500;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("spec.rules.list", "List Rules", LOC, list_cmd).pure(),
        CommandSpec::new("spec.rules.add", "Add Rule", LOC, add_cmd).params(
            r#"{"kind": "require"|"recommend"|"warn"|"exclude", "target": string (paragraph id), "if": [{"on": id} | {"off": id} | {"field": id, "is"?: string}], "note"?: string}"#,
        ),
        CommandSpec::new("spec.rules.remove", "Remove Rule", LOC, remove_cmd).params(r#"{"id": string}"#),
        CommandSpec::new("spec.rules.check", "Check Rules", LOC, check_cmd).pure(),
        CommandSpec::new("spec.why", "Why Is This On or Off", LOC, why_cmd).params(r#"{"id"?: string (default: the paragraph at the caret)}"#).pure(),
    ]
}

/// What a rule does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleKind {
    Require,
    Recommend,
    Warn,
    Exclude,
}

impl RuleKind {
    pub fn name(self) -> &'static str {
        match self {
            RuleKind::Require => "require",
            RuleKind::Recommend => "recommend",
            RuleKind::Warn => "warn",
            RuleKind::Exclude => "exclude",
        }
    }
}

/// One condition of a rule.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cond {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub off: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is: Option<String>,
}

/// A rule of the section.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub kind: RuleKind,
    #[serde(rename = "if")]
    pub when: Vec<Cond>,
    pub target: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// The section's rules (an unreadable property counts as none).
pub fn rules(doc: &Document) -> Vec<Rule> {
    let mut v: Vec<Rule> = doc.custom_prop(RULES_PROP).and_then(|s| serde_json::from_str(s).ok()).unwrap_or_default();
    v.truncate(MAX_RULES);
    v
}

fn store(doc: &mut Document, rules: &[Rule]) -> Result<(), CmdError> {
    if rules.is_empty() {
        doc.remove_custom_prop(RULES_PROP);
        return Ok(());
    }
    let s = serde_json::to_string(rules).map_err(|e| CmdError::Failed(e.to_string()))?;
    doc.set_custom_prop(RULES_PROP, &s);
    Ok(())
}

/// Each fill-in's value, if it is set (left-out optional text counts as set to "(leave out)").
fn field_values(doc: &Document) -> HashMap<String, String> {
    fields::fields(doc).into_iter().filter(|f| f.state != fields::FieldState::Open).map(|f| (f.id, f.value)).collect()
}

/// Whether every condition of `r` holds, given which paragraphs print.
fn active(r: &Rule, index: &HashMap<&str, usize>, on: &[bool], values: &HashMap<String, String>) -> bool {
    let printing = |id: &str| index.get(id).and_then(|i| on.get(*i)).copied();
    !r.when.is_empty()
        && r.when.iter().all(|c| {
            if let Some(id) = &c.on {
                printing(id) == Some(true)
            } else if let Some(id) = &c.off {
                printing(id) == Some(false)
            } else if let Some(id) = &c.field {
                match (values.get(id), &c.is) {
                    (Some(v), Some(want)) => v.trim().eq_ignore_ascii_case(want.trim()),
                    (Some(_), None) => true,
                    (None, _) => false,
                }
            } else {
                false
            }
        })
}

fn index_of(entries: &[Entry]) -> HashMap<&str, usize> {
    entries.iter().enumerate().map(|(i, e)| (e.id.as_str(), i)).collect()
}

/// Which paragraphs print given what the author switched on (`manual`) and overrode
/// (`overridden`), with the section's rules applied until nothing changes. Returns (prints,
/// on because of a rule).
pub fn evaluate(doc: &Document, entries: &[Entry], manual: &[bool], overridden: &HashSet<String>) -> (Vec<bool>, Vec<bool>) {
    let rules = rules(doc);
    let n = entries.len();
    let compute = |linked: &[bool]| {
        let mut on = vec![false; n];
        for (i, e) in entries.iter().enumerate() {
            let parent = e.parent.is_none_or(|p| on.get(p).copied().unwrap_or(false));
            let want = manual.get(i).copied().unwrap_or(false) || (linked.get(i).copied().unwrap_or(false) && !overridden.contains(&e.id));
            if let Some(x) = on.get_mut(i) {
                *x = want && e.kind != Kind::Note && parent;
            }
        }
        on
    };
    let mut linked = vec![false; n];
    if rules.is_empty() {
        return (compute(&linked), linked);
    }
    let index = index_of(entries);
    let values = if rules.iter().any(|r| r.when.iter().any(|c| c.field.is_some())) { field_values(doc) } else { HashMap::new() };
    for _ in 0..rules.len().saturating_add(2).min(64) {
        let on = compute(&linked);
        let mut blocked = vec![false; n];
        for r in rules.iter().filter(|r| r.kind == RuleKind::Exclude && active(r, &index, &on, &values)) {
            if let Some(t) = index.get(r.target.as_str()).and_then(|t| blocked.get_mut(*t)) {
                *t = true;
            }
        }
        let mut next = vec![false; n];
        for r in rules.iter().filter(|r| r.kind == RuleKind::Require && active(r, &index, &on, &values)) {
            let mut at = index.get(r.target.as_str()).copied();
            if at.is_some_and(|t| blocked.get(t).copied().unwrap_or(false)) {
                continue;
            }
            // The target and the paragraphs it sits under.
            while let Some(i) = at {
                if let Some(x) = next.get_mut(i) {
                    if *x {
                        break;
                    }
                    *x = true;
                }
                at = entries.get(i).and_then(|e| e.parent);
            }
        }
        if next == linked {
            return (on, linked);
        }
        linked = next;
    }
    (compute(&linked), linked)
}

/// Why paragraph `i` can't be switched on now: an exclude rule in force (its target is `i`, or
/// its `on` condition is `i` and its target prints).
pub fn blocking(doc: &Document, entries: &[Entry], on: &[bool], i: usize) -> Option<String> {
    let rules = rules(doc);
    if rules.is_empty() {
        return None;
    }
    let index = index_of(entries);
    let values = field_values(doc);
    let id = entries.get(i)?.id.as_str();
    for r in rules.iter().filter(|r| r.kind == RuleKind::Exclude) {
        let note = if r.note.is_empty() { String::new() } else { format!(" ({})", r.note) };
        if r.target == id && active(r, &index, on, &values) {
            return Some(format!("rule {} excludes it while {}{note}", r.id, describe_when(r, entries)));
        }
        let target_on = index.get(r.target.as_str()).and_then(|t| on.get(*t)).copied().unwrap_or(false);
        if target_on && r.when.iter().any(|c| c.on.as_deref() == Some(id)) {
            return Some(format!("rule {} keeps it and {} apart, and {} is on{note}", r.id, r.target, r.target));
        }
    }
    None
}

/// A rule's conditions in words.
fn describe_when(r: &Rule, entries: &[Entry]) -> String {
    let name = |id: &str| entries.iter().find(|e| e.id == id).map_or_else(|| id.to_string(), |e| e.id.clone());
    r.when
        .iter()
        .map(|c| {
            if let Some(id) = &c.on {
                format!("{} is on", name(id))
            } else if let Some(id) = &c.off {
                format!("{} is off", name(id))
            } else if let (Some(f), Some(v)) = (&c.field, &c.is) {
                format!("fill-in {f} is {v:?}")
            } else if let Some(f) = &c.field {
                format!("fill-in {f} is filled in")
            } else {
                "?".into()
            }
        })
        .collect::<Vec<_>>()
        .join(" and ")
}

/// How a rule stands now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    /// Its conditions don't hold.
    Idle,
    /// In force and satisfied.
    Met,
    /// A recommendation not followed: the target is off.
    Suggested,
    /// A warning or exclusion broken: the target is on.
    Conflict,
    /// A requirement the author switched off.
    Overridden,
    /// A requirement whose target can't print (an exclusion or a note).
    Unmet,
}

impl Standing {
    pub fn name(self) -> &'static str {
        match self {
            Standing::Idle => "idle",
            Standing::Met => "met",
            Standing::Suggested => "suggested",
            Standing::Conflict => "conflict",
            Standing::Overridden => "overridden",
            Standing::Unmet => "unmet",
        }
    }
}

/// Every rule with how it stands, given the document's current state.
pub fn standings(doc: &Document) -> Vec<(Rule, Standing)> {
    let entries = sections::outline(doc);
    let on: Vec<bool> = entries.iter().map(|e| e.on).collect();
    let index = index_of(&entries);
    let values = field_values(doc);
    let overridden = sections::stored_overridden(doc);
    rules(doc)
        .into_iter()
        .map(|r| {
            let target_on = index.get(r.target.as_str()).and_then(|t| on.get(*t)).copied().unwrap_or(false);
            let st = if !active(&r, &index, &on, &values) {
                Standing::Idle
            } else {
                match (r.kind, target_on) {
                    (RuleKind::Require, true) | (RuleKind::Recommend, true) | (RuleKind::Warn, false) | (RuleKind::Exclude, false) => Standing::Met,
                    (RuleKind::Require, false) if overridden.contains(&r.target) => Standing::Overridden,
                    (RuleKind::Require, false) => Standing::Unmet,
                    (RuleKind::Recommend, false) => Standing::Suggested,
                    (RuleKind::Warn, true) | (RuleKind::Exclude, true) => Standing::Conflict,
                }
            };
            (r, st)
        })
        .collect()
}

/// Problems with the rules themselves: unknown paragraphs or fill-ins, targets that can't print,
/// a paragraph required and excluded on the same conditions, and circles of requirements.
pub fn problems(doc: &Document) -> Vec<Value> {
    let entries = sections::outline(doc);
    let index = index_of(&entries);
    let field_ids: HashSet<String> = fields::fields(doc).into_iter().map(|f| f.id).collect();
    let all = rules(doc);
    let mut out = Vec::new();
    for r in &all {
        let mut bad = |m: String| out.push(json!({"severity": "error", "rule": r.id, "message": m}));
        match index.get(r.target.as_str()).and_then(|i| entries.get(*i)) {
            None => bad(format!("its target {} isn't a paragraph of this section", r.target)),
            Some(e) if e.kind == Kind::Note && r.kind != RuleKind::Warn => bad(format!("its target {} is a note, which never prints", r.target)),
            _ => {}
        }
        for c in &r.when {
            for id in [&c.on, &c.off].into_iter().flatten() {
                if !index.contains_key(id.as_str()) {
                    bad(format!("its condition names {id}, which isn't a paragraph of this section"));
                }
                if *id == r.target {
                    bad(format!("{id} is both its condition and its target"));
                }
            }
            if let Some(f) = &c.field
                && !field_ids.contains(f)
            {
                bad(format!("its condition names fill-in {f}, which isn't in this section"));
            }
        }
    }
    // Required and excluded on the same conditions.
    for a in all.iter().filter(|r| r.kind == RuleKind::Require) {
        for b in all.iter().filter(|r| r.kind == RuleKind::Exclude && r.target == a.target && r.when == a.when) {
            let m = format!("rules {} and {} require and exclude {} on the same conditions", a.id, b.id, a.target);
            for id in [&a.id, &b.id] {
                out.push(json!({"severity": "error", "rule": id, "message": m}));
            }
        }
    }
    // Circles of requirements between paragraphs (A on requires B, B on requires A …).
    let mut graph: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for r in all.iter().filter(|r| r.kind == RuleKind::Require) {
        for c in &r.when {
            if let Some(from) = &c.on {
                graph.entry(from.as_str()).or_default().push(r.target.as_str());
            }
        }
    }
    let mut reported = HashSet::new();
    for start in graph.keys() {
        let mut stack = vec![(*start, vec![*start])];
        let mut seen = HashSet::new();
        while let Some((at, path)) = stack.pop() {
            if path.len() > 64 {
                continue;
            }
            for next in graph.get(at).into_iter().flatten() {
                if next == start {
                    let mut key: Vec<&str> = path.clone();
                    key.sort_unstable();
                    if reported.insert(key) {
                        out.push(
                            json!({"severity": "notice", "message": format!("requirements go round in a circle: {} → {start}", path.join(" → "))}),
                        );
                    }
                } else if seen.insert(*next) {
                    let mut p2 = path.clone();
                    p2.push(next);
                    stack.push((next, p2));
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Commands

fn rule_json(r: &Rule, st: Standing, entries: &[Entry]) -> Value {
    json!({
        "id": r.id,
        "kind": r.kind.name(),
        "if": r.when,
        "target": r.target,
        "note": r.note,
        "standing": st.name(),
        "when": describe_when(r, entries),
    })
}

fn list_cmd(s: &mut Session, _v: &Value) -> CmdResult {
    let entries = sections::outline(&s.doc);
    let rows: Vec<Value> = standings(&s.doc).iter().map(|(r, st)| rule_json(r, *st, &entries)).collect();
    Ok(json!({"rules": rows, "count": rows.len()}))
}

fn add_cmd(s: &mut Session, v: &Value) -> CmdResult {
    let kind: RuleKind = serde_json::from_value(v.get("kind").cloned().unwrap_or(Value::Null))
        .map_err(|_| CmdError::Params("`kind` must be require, recommend, warn or exclude".into()))?;
    let target = p::req_str(v, "target")?.trim().to_string();
    let when: Vec<Cond> = serde_json::from_value(v.get("if").cloned().unwrap_or(Value::Null))
        .map_err(|_| CmdError::Params("`if` must be a list of conditions: {\"on\": id}, {\"off\": id} or {\"field\": id, \"is\"?: value}".into()))?;
    if when.is_empty() || when.len() > MAX_CONDITIONS {
        return Err(CmdError::Params(format!("a rule needs 1 to {MAX_CONDITIONS} conditions")));
    }
    for c in &when {
        let n = [c.on.is_some(), c.off.is_some(), c.field.is_some()].iter().filter(|x| **x).count();
        if n != 1 || (c.is.is_some() && c.field.is_none()) {
            return Err(CmdError::Params("each condition names exactly one of `on`, `off` or `field` (`is` goes with `field`)".into()));
        }
    }
    let note = p::str(v, "note").unwrap_or("").trim().to_string();
    if note.chars().count() > MAX_NOTE {
        return Err(CmdError::Params(format!("`note` is longer than {MAX_NOTE} characters")));
    }
    let mut all = rules(&s.doc);
    if all.len() >= MAX_RULES {
        return Err(CmdError::Failed(format!("a section holds at most {MAX_RULES} rules")));
    }
    let next = all.iter().filter_map(|r| r.id.strip_prefix('r').and_then(|n| n.parse::<u32>().ok())).max().unwrap_or(0).saturating_add(1);
    let rule = Rule { id: format!("r{next:04}"), kind, when, target, note };
    all.push(rule.clone());
    // Refuse a rule that is wrong on its own (unknown ids, contradictions); circles are allowed.
    let mut probe = s.doc.clone();
    store(&mut probe, &all)?;
    let errors: Vec<String> = problems(&probe)
        .into_iter()
        .filter(|p| p["severity"] == "error" && p.get("rule").and_then(Value::as_str).is_none_or(|id| id == rule.id))
        .filter_map(|p| p["message"].as_str().map(str::to_string))
        .collect();
    if !errors.is_empty() {
        return Err(CmdError::Failed(format!("rule not added: {}", errors.join("; "))));
    }
    store(&mut s.doc, &all)?;
    let entries = sections::outline(&s.doc);
    let st = standings(&s.doc).into_iter().find(|(r, _)| r.id == rule.id).map_or(Standing::Idle, |(_, st)| st);
    Ok(rule_json(&rule, st, &entries))
}

fn remove_cmd(s: &mut Session, v: &Value) -> CmdResult {
    let id = p::req_str(v, "id")?.trim().to_string();
    let mut all = rules(&s.doc);
    let n = all.len();
    all.retain(|r| r.id != id);
    if all.len() == n {
        return Err(CmdError::Failed(format!("no rule called {id:?}")));
    }
    store(&mut s.doc, &all)?;
    Ok(json!({"removed": id, "count": all.len()}))
}

fn check_cmd(s: &mut Session, _v: &Value) -> CmdResult {
    let mut out = problems(&s.doc);
    for (r, st) in standings(&s.doc) {
        let note = if r.note.is_empty() { String::new() } else { format!(" ({})", r.note) };
        let finding = match st {
            Standing::Conflict if r.kind == RuleKind::Exclude => Some(("error", format!("{} is on, but rule {} excludes it{note}", r.target, r.id))),
            Standing::Conflict => Some(("warning", format!("{} is on: rule {} warns against it{note}", r.target, r.id))),
            Standing::Overridden => Some(("notice", format!("{} is off although rule {} requires it: switched off by you{note}", r.target, r.id))),
            Standing::Unmet => Some(("warning", format!("rule {} requires {}, which can't print{note}", r.id, r.target))),
            Standing::Suggested => Some(("notice", format!("rule {} recommends {}{note}", r.id, r.target))),
            Standing::Idle | Standing::Met => None,
        };
        if let Some((sev, m)) = finding {
            out.push(json!({"severity": sev, "rule": r.id, "target": r.target, "message": m}));
        }
    }
    let count = |sev: &str| out.iter().filter(|x| x["severity"] == sev).count();
    Ok(json!({"errors": count("error"), "warnings": count("warning"), "notices": count("notice"), "findings": out}))
}

/// Why a paragraph is on or off, in sentences.
pub fn why(doc: &Document, entries: &[Entry], i: usize) -> Vec<String> {
    let Some(e) = entries.get(i) else { return Vec::new() };
    let mut out = Vec::new();
    let overridden = sections::stored_overridden(doc);
    if e.kind == Kind::Note {
        out.push("It is an author note: notes never print.".into());
        return out;
    }
    if let Some(p) = e.parent.and_then(|p| entries.get(p))
        && !p.on
    {
        out.push(format!("Its parent {} is off, so it can't print.", p.id));
    }
    if e.manual {
        out.push("You switched it on.".into());
    }
    let st = standings(doc);
    for (r, s) in st.iter().filter(|(r, _)| r.target == e.id) {
        let when = describe_when(r, entries);
        let note = if r.note.is_empty() { String::new() } else { format!(" ({})", r.note) };
        let line = match (r.kind, s) {
            (RuleKind::Require, Standing::Met) if e.linked && !e.manual => format!("Rule {} switched it on because {when}{note}.", r.id),
            (RuleKind::Require, Standing::Met) => format!("Rule {} requires it because {when}{note}.", r.id),
            (RuleKind::Require, Standing::Overridden) => format!("Rule {} requires it because {when}, but you switched it off{note}.", r.id),
            (RuleKind::Require, Standing::Unmet) => format!("Rule {} requires it because {when}, but it can't print{note}.", r.id),
            (RuleKind::Recommend, Standing::Suggested) => format!("Rule {} recommends it because {when}{note}.", r.id),
            (RuleKind::Warn, Standing::Conflict) => format!("Rule {} warns against it because {when}{note}.", r.id),
            (RuleKind::Exclude, Standing::Conflict) => format!("Rule {} excludes it because {when}, yet it is on{note}.", r.id),
            (RuleKind::Exclude, Standing::Met) => format!("Rule {} excludes it because {when}{note}.", r.id),
            _ => continue,
        };
        out.push(line);
    }
    if !e.on && overridden.contains(&e.id) && !out.iter().any(|l| l.contains("you switched it off")) {
        out.push("You switched it off against a rule.".into());
    }
    if out.is_empty() {
        out.push(if e.on { "It is on.".into() } else { "It is off: switch it on to print it.".into() });
    }
    out
}

fn why_cmd(s: &mut Session, v: &Value) -> CmdResult {
    let entries = sections::outline(&s.doc);
    let i = match p::str(v, "id") {
        Some(id) => entries.iter().position(|e| e.id == id.trim()).ok_or_else(|| CmdError::Failed(format!("no spec paragraph called {id:?}")))?,
        None => sections::at_caret(s, &entries).ok_or_else(|| CmdError::Failed("the caret is not in a spec paragraph".into()))?,
    };
    let id = entries.get(i).map(|e| e.id.clone()).unwrap_or_default();
    Ok(json!({"id": id, "reasons": why(&s.doc, &entries, i)}))
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

    /// p0001 title; p0002 PART 1; p0003 SUBMITTALS, p0004 its LEED paragraph; p0005 PART 2;
    /// p0006 PANELS, p0007 Type X (with a choice), p0008 Type C, p0009 glass-mat; p0010 end.
    fn section() -> Session {
        let mut d = Document::new();
        d.body = vec![
            para("SCT", "SECTION 09 2900 - GYPSUM BOARD"),
            para("PRT", "GENERAL"),
            para("ART", "SUBMITTALS"),
            para("PR1", "LEED submittals: recycled content."),
            para("PRT", "PRODUCTS"),
            para("ART", "PANELS"),
            para("PR1", "Type X: [5/8 inch] [3/4 inch]."),
            para("PR1", "Type C."),
            para("PR1", "Glass-mat panels."),
            para("EOS", "END OF SECTION 09 2900"),
        ];
        let mut s = Session::new(d);
        run(&mut s, "spec.import", json!({}));
        run(&mut s, "spec.fields.detect", json!({}));
        s
    }

    fn state(s: &mut Session, id: &str) -> String {
        let o = run(s, "spec.outline", json!({}));
        o["items"].as_array().unwrap().iter().find(|i| i["id"] == id).unwrap()["state"].as_str().unwrap().to_string()
    }

    #[test]
    fn require_turns_the_target_and_its_parents_on_and_the_author_can_override() {
        let mut s = section();
        // Choosing glass-mat (p0009) requires the LEED submittal (p0004) in Part 1.
        run(&mut s, "spec.rules.add", json!({"kind": "require", "if": [{"on": "p0009"}], "target": "p0004", "note": "recycled content"}));
        run(&mut s, "spec.activate", json!({"ids": ["p0005", "p0006", "p0009"]}));
        assert_eq!(state(&mut s, "p0004"), "linked");
        assert_eq!(state(&mut s, "p0002"), "linked", "the paragraphs it sits under come on too");
        let why = run(&mut s, "spec.why", json!({"id": "p0004"}));
        assert!(why["reasons"][0].as_str().unwrap().contains("Rule r0001 switched it on because p0009 is on"), "{why}");
        // Not stored as the author's choice: switching glass-mat off takes it away again.
        assert!(!s.doc.custom_prop(sections::STATE_PROP).unwrap().contains("p0004"));
        run(&mut s, "spec.deactivate", json!({"id": "p0009"}));
        assert_eq!(state(&mut s, "p0004"), "off");
        // The author switches it off against the rule: overridden, reported, never undone.
        run(&mut s, "spec.activate", json!({"id": "p0009"}));
        run(&mut s, "spec.toggle", json!({"id": "p0004"}));
        assert_eq!(state(&mut s, "p0004"), "overridden");
        let c = run(&mut s, "spec.rules.check", json!({}));
        assert!(c["findings"].as_array().unwrap().iter().any(|f| f["message"].as_str().unwrap().contains("switched off by you")), "{c}");
        // Switching it on again lifts the override.
        run(&mut s, "spec.toggle", json!({"id": "p0004"}));
        assert_eq!(state(&mut s, "p0004"), "on");
        assert_eq!(s.doc.custom_prop(sections::OVERRIDDEN_PROP), None);
    }

    #[test]
    fn exclude_keeps_two_paragraphs_apart_and_says_why() {
        let mut s = section();
        run(&mut s, "spec.rules.add", json!({"kind": "exclude", "if": [{"on": "p0007"}], "target": "p0008", "note": "pick one panel type"}));
        run(&mut s, "spec.activate", json!({"ids": ["p0005", "p0006", "p0007"]}));
        let e = s.run("spec.activate", &json!({"id": "p0008"})).unwrap_err().to_string();
        assert!(e.contains("rule r0001 excludes it while p0007 is on (pick one panel type)"), "{e}");
        // And the other way round.
        run(&mut s, "spec.deactivate", json!({"id": "p0007"}));
        run(&mut s, "spec.activate", json!({"id": "p0008"}));
        let e = s.run("spec.toggle", &json!({"id": "p0007"})).unwrap_err().to_string();
        assert!(e.contains("keeps it and p0008 apart"), "{e}");
    }

    #[test]
    fn rules_on_fill_ins_recommend_and_warn() {
        let mut s = section();
        run(&mut s, "spec.rules.add", json!({"kind": "recommend", "if": [{"field": "f0001", "is": "3/4 inch"}], "target": "p0008"}));
        run(&mut s, "spec.rules.add", json!({"kind": "warn", "if": [{"on": "p0007"}], "target": "p0009", "note": "check fire rating"}));
        run(&mut s, "spec.activate", json!({"ids": ["p0005", "p0006", "p0007"]}));
        let standing = |s: &mut Session, id: &str| {
            let l = run(s, "spec.rules.list", json!({}));
            l["rules"].as_array().unwrap().iter().find(|r| r["id"] == id).unwrap()["standing"].as_str().unwrap().to_string()
        };
        assert_eq!(standing(&mut s, "r0001"), "idle");
        run(&mut s, "spec.fields.set", json!({"id": "f0001", "option": 1}));
        assert_eq!(standing(&mut s, "r0001"), "suggested");
        assert!(
            run(&mut s, "spec.why", json!({"id": "p0008"}))["reasons"][0]
                .as_str()
                .unwrap()
                .contains("recommends it because fill-in f0001 is \"3/4 inch\"")
        );
        run(&mut s, "spec.activate", json!({"id": "p0008"}));
        assert_eq!(standing(&mut s, "r0001"), "met");
        // A warning is only a warning: the paragraph comes on, flagged.
        assert_eq!(standing(&mut s, "r0002"), "met");
        run(&mut s, "spec.activate", json!({"id": "p0009"}));
        assert_eq!(standing(&mut s, "r0002"), "conflict");
        let c = run(&mut s, "spec.rules.check", json!({}));
        assert_eq!(c["warnings"], 1, "{c}");
    }

    #[test]
    fn bad_rules_are_refused_and_circles_reported() {
        let mut s = section();
        let e = |s: &mut Session, v: Value| s.run("spec.rules.add", &v).unwrap_err().to_string();
        assert!(e(&mut s, json!({"kind": "require", "if": [{"on": "p0099"}], "target": "p0004"})).contains("isn't a paragraph"));
        assert!(e(&mut s, json!({"kind": "require", "if": [{"on": "p0004"}], "target": "p0004"})).contains("both its condition and its target"));
        assert!(e(&mut s, json!({"kind": "maybe", "if": [{"on": "p0004"}], "target": "p0007"})).contains("`kind`"));
        assert!(e(&mut s, json!({"kind": "require", "if": [], "target": "p0007"})).contains("1 to"));
        assert!(e(&mut s, json!({"kind": "require", "if": [{"on": "p0004", "off": "p0007"}], "target": "p0008"})).contains("exactly one"));
        assert!(e(&mut s, json!({"kind": "require", "if": [{"field": "f0099"}], "target": "p0008"})).contains("fill-in f0099"));
        run(&mut s, "spec.rules.add", json!({"kind": "require", "if": [{"on": "p0007"}], "target": "p0008"}));
        assert!(e(&mut s, json!({"kind": "exclude", "if": [{"on": "p0007"}], "target": "p0008"})).contains("require and exclude"));
        run(&mut s, "spec.rules.add", json!({"kind": "require", "if": [{"on": "p0008"}], "target": "p0007"}));
        let c = run(&mut s, "spec.rules.check", json!({}));
        assert!(c["findings"].as_array().unwrap().iter().any(|f| f["message"].as_str().unwrap().contains("circle")), "{c}");
        // A circle still settles: one on brings the other.
        run(&mut s, "spec.activate", json!({"ids": ["p0005", "p0006", "p0007"]}));
        assert_eq!(state(&mut s, "p0008"), "linked");
        run(&mut s, "spec.rules.remove", json!({"id": "r0002"}));
        assert_eq!(run(&mut s, "spec.rules.list", json!({}))["count"], 1);
        // Undo takes the removal back.
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(run(&mut s, "spec.rules.list", json!({}))["count"], 2);
    }

    #[test]
    fn rules_travel_with_the_file_and_print() {
        let mut s = section();
        run(&mut s, "spec.rules.add", json!({"kind": "require", "if": [{"on": "p0009"}], "target": "p0004"}));
        run(&mut s, "spec.activate", json!({"ids": ["p0005", "p0006", "p0009"]}));
        let bytes = crate::io::save_bytes("x.docx", &s.doc).unwrap();
        let mut b = Session::new(crate::io::open_bytes("x.docx", &bytes).unwrap());
        assert_eq!(state(&mut b, "p0004"), "linked");
        let printed: Vec<String> = sections::printable(&b.doc).iter().map(|e| e.id.clone()).collect();
        assert!(printed.contains(&"p0004".to_string()) && printed.contains(&"p0003".to_string()), "{printed:?}");
    }
}
