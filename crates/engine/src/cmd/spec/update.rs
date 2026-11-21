//! Specs › Master Updates: bring a project copy up to a newer master (milestone M11). Added by the
//! Speccraft fork; not part of the original WordCraft.
//!
//! A copy remembers the master's wording of every paragraph when it was made (`Speccraft.Original`,
//! M8) and the order of the master's paragraphs (`Speccraft.Order`). [`compare`] reads the master
//! as it is now and lines its paragraphs up with those: paragraphs the master **changed**, **added**
//! and **removed**. Each change says what it means for the project: whether the author edited
//! the paragraph too (a conflict), whether it prints, whether a section rule names it, whether
//! fill-ins chosen in it would be reset.
//!
//! [`apply`] takes a decision for each change: `master` (take the master's) or `mine` (keep the
//! project's). Without one, a change takes the master's unless it is a conflict or would remove a
//! printing paragraph. Nothing switches on or off: paragraphs the master added come in switched
//! off, and the author's choices, overrides and rules stay. Afterwards the copy's record of the
//! master is the new master, so a paragraph kept as the project had it shows as modified (M\*), a
//! new master paragraph not taken shows as a deleted master paragraph (and can be restored), and a
//! removed one kept shows as the project's own. Each update is added to `Speccraft.History`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};
use wordcraft_doc::para::OBJ;
use wordcraft_doc::{Block, Document};

use super::edits::{self, Original};
use super::fields::{self, FieldState};
use super::project::{self, BASELINE_PROP, MASTER_PROP, ROLE_PROP, fingerprint};
use super::sections::{self, Entry};
use crate::{CmdError, CmdResult, CommandSpec, Session, p};

const LOC: &str = "Specs › Master Updates";
/// The master's paragraph ids in the master's order.
pub const ORDER_PROP: &str = "Speccraft.Order";
/// Updates taken, newest last (JSON list).
pub const HISTORY_PROP: &str = "Speccraft.History";
/// Most updates kept in the history.
const MAX_HISTORY: usize = 100;
/// Largest comparison table (paragraphs × paragraphs, after common beginnings and ends).
const MAX_CELLS: usize = 16_000_000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("spec.update.compare", "Compare with Master", LOC, compare_cmd)
            .params(r#"{"master"?: string (default: the master this copy was made from)}"#)
            .pure(),
        CommandSpec::new("spec.update.apply", "Update from Master", LOC, apply_cmd)
            .params(r#"{"master"?: string, "fingerprint"?: string (from spec.update.compare: refused if the master changed since), "decisions"?: {"<change n>": "master" or "mine"}}"#),
        CommandSpec::new("spec.update.history", "Master Update History", LOC, history_cmd).params(r#"{}"#).pure(),
    ]
}

/// Text compared: objects out, whitespace runs as one space.
fn normal(t: &str) -> String {
    t.replace(OBJ, "").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The master's paragraphs as this copy last knew them, in the master's order.
pub fn master_order(doc: &Document) -> Vec<(String, Original)> {
    let orig = edits::originals(doc);
    if orig.is_empty() {
        return from_baseline(doc);
    }
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for id in doc.custom_prop(ORDER_PROP).unwrap_or("").split(',').map(str::trim) {
        if let Some(o) = orig.get(id)
            && seen.insert(id.to_string())
        {
            out.push((id.to_string(), o.clone()));
        }
    }
    for (id, o) in &orig {
        if !seen.contains(id) {
            out.push((id.clone(), o.clone()));
        }
    }
    out
}

/// For a copy made before the master's wording was recorded (M8), once its master has changed:
/// the paragraphs it still has unedited are the old wording (their fingerprints say so), and an
/// edited one stands in with the author's wording, marked as edited by [`compare`].
fn from_baseline(doc: &Document) -> Vec<(String, Original)> {
    let order: Vec<String> =
        doc.custom_prop(BASELINE_PROP).unwrap_or("").split(',').filter_map(|x| x.split_once(':')).map(|(id, _)| id.trim().to_string()).collect();
    let entries = sections::outline(doc);
    order
        .iter()
        .filter_map(|id| {
            let e = entries.iter().find(|e| e.id == *id)?;
            let style = doc.para_at(&e.range.start).and_then(|p| p.props.style.clone()).unwrap_or_default();
            Some((id.clone(), Original(style, edits::base_text(doc, e))))
        })
        .collect()
}

/// A master read for comparing.
pub struct Master {
    pub path: String,
    pub version: String,
    pub fingerprint: String,
    pub paras: Vec<Original>,
}

/// The master `doc` came from, or `path`.
fn master_path(doc: &Document, v: &Value) -> Result<String, CmdError> {
    if let Some(m) = p::str(v, "master").map(str::trim).filter(|m| !m.is_empty()) {
        return Ok(m.to_string());
    }
    doc.custom_prop(MASTER_PROP)
        .and_then(|m| serde_json::from_str::<Value>(m).ok())
        .and_then(|m| m.get("path").and_then(Value::as_str).map(str::to_string))
        .ok_or_else(|| CmdError::Failed("this document doesn't record its master: give `master`".into()))
}

/// Read the master at `path` and list its paragraphs.
pub fn read_master(path: &str) -> Result<Master, CmdError> {
    let bytes = crate::io::read_document(Path::new(path)).map_err(CmdError::Failed)?;
    let doc = crate::io::open_bytes(path, &bytes).map_err(CmdError::Failed)?;
    if doc.custom_prop(ROLE_PROP) == Some("project") {
        return Err(CmdError::Failed(format!("{path} is a project copy, not a master")));
    }
    let mut s = Session::new(doc);
    s.run("spec.import", &json!({}))?;
    let orig = edits::originals(&s.doc);
    let paras: Vec<Original> = sections::outline(&s.doc).iter().filter_map(|e| orig.get(&e.id).cloned()).collect();
    if paras.is_empty() {
        return Err(CmdError::Failed(format!("{path} has no spec paragraphs")));
    }
    Ok(Master { path: path.to_string(), version: project::version_stamp(&s.doc), fingerprint: fingerprint(&bytes), paras })
}

// ---------------------------------------------------------------------------------------------
// Lining up

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Same(usize, usize),
    Old(usize),
    New(usize),
}

/// The longest common run of `a` and `b`, as steps. Common beginnings and ends are matched first.
fn diff(a: &[String], b: &[String]) -> Result<Vec<Op>, CmdError> {
    let pre = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let suf = a.get(pre..).unwrap_or(&[]).iter().rev().zip(b.get(pre..).unwrap_or(&[]).iter().rev()).take_while(|(x, y)| x == y).count();
    let (ma, mb) = (a.get(pre..a.len() - suf).unwrap_or(&[]), b.get(pre..b.len() - suf).unwrap_or(&[]));
    let (n, m) = (ma.len(), mb.len());
    if (n + 1).saturating_mul(m + 1) > MAX_CELLS {
        return Err(CmdError::Failed("the master changed too much to compare paragraph by paragraph".into()));
    }
    // dp[i][j]: the longest common run of ma[i..] and mb[j..].
    let w = m + 1;
    let mut dp = vec![0u32; (n + 1) * w];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            let v = if ma.get(i) == mb.get(j) {
                dp.get((i + 1) * w + j + 1).copied().unwrap_or(0) + 1
            } else {
                dp.get((i + 1) * w + j).copied().unwrap_or(0).max(dp.get(i * w + j + 1).copied().unwrap_or(0))
            };
            if let Some(c) = dp.get_mut(i * w + j) {
                *c = v;
            }
        }
    }
    let mut out: Vec<Op> = (0..pre).map(|k| Op::Same(k, k)).collect();
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        let at = |i: usize, j: usize| dp.get(i * w + j).copied().unwrap_or(0);
        if i < n && j < m && ma.get(i) == mb.get(j) {
            out.push(Op::Same(pre + i, pre + j));
            i += 1;
            j += 1;
        } else if j < m && (i == n || at(i, j + 1) >= at(i + 1, j)) {
            out.push(Op::New(pre + j));
            j += 1;
        } else {
            out.push(Op::Old(pre + i));
            i += 1;
        }
    }
    out.extend((0..suf).map(|k| Op::Same(a.len() - suf + k, b.len() - suf + k)));
    Ok(out)
}

/// How alike two paragraphs read: shared words over all words (0 to 1).
fn likeness(a: &str, b: &str) -> f32 {
    let words =
        |t: &str| t.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect::<HashSet<_>>();
    let (x, y) = (words(a), words(b));
    let all = x.union(&y).count();
    if all == 0 {
        return if a.trim() == b.trim() { 1.0 } else { 0.0 };
    }
    x.intersection(&y).count() as f32 / all as f32
}

/// What happened to one master paragraph, in master order: kept, changed, added or removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Same(usize, usize),
    Changed(usize, usize),
    Added(usize),
    Removed(usize),
}

/// Line the old master up with the new: unmatched paragraphs between two matches that read
/// alike are one paragraph changed.
fn line_up(old: &[Original], new: &[Original]) -> Result<Vec<Step>, CmdError> {
    let key = |o: &Original| format!("{}\u{1}{}", o.0.to_ascii_uppercase(), normal(&o.1));
    let ops = diff(&old.iter().map(key).collect::<Vec<_>>(), &new.iter().map(key).collect::<Vec<_>>())?;
    let mut out = Vec::new();
    let (mut olds, mut news): (Vec<usize>, Vec<usize>) = (Vec::new(), Vec::new());
    let flush = |olds: &mut Vec<usize>, news: &mut Vec<usize>, out: &mut Vec<Step>| {
        let mut paired: HashMap<usize, usize> = HashMap::new();
        let mut from = 0;
        for &i in olds.iter() {
            let Some(o) = old.get(i) else { continue };
            let best = news.iter().enumerate().skip(from).find(|(_, j)| {
                new.get(**j).is_some_and(|n| {
                    let l = likeness(&o.1, &n.1);
                    (n.0.eq_ignore_ascii_case(&o.0) && l >= 0.5) || l >= 0.75
                })
            });
            if let Some((k, &j)) = best {
                paired.insert(j, i);
                from = k + 1;
            }
        }
        // In reading order: a removed paragraph before the changed one that follows it.
        let taken: HashSet<usize> = paired.values().copied().collect();
        let mut removed = olds.iter().copied().filter(|i| !taken.contains(i)).peekable();
        for &j in news.iter() {
            match paired.get(&j) {
                Some(&i) => {
                    while let Some(r) = removed.next_if(|r| *r < i) {
                        out.push(Step::Removed(r));
                    }
                    out.push(Step::Changed(i, j));
                }
                None => out.push(Step::Added(j)),
            }
        }
        out.extend(removed.map(Step::Removed));
        olds.clear();
        news.clear();
    };
    for op in ops {
        match op {
            Op::Same(i, j) => {
                flush(&mut olds, &mut news, &mut out);
                out.push(Step::Same(i, j));
            }
            Op::Old(i) => olds.push(i),
            Op::New(j) => news.push(j),
        }
    }
    flush(&mut olds, &mut news, &mut out);
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Comparing

/// One change the master made, and what it means for the project.
#[derive(Clone, Debug)]
pub struct Change {
    pub n: usize,
    /// `changed`, `added` or `removed`.
    pub kind: &'static str,
    /// The project's id for the paragraph (for an added one, the id it will get).
    pub id: String,
    /// Its place in the new master (none for a removed paragraph).
    pub j: Option<usize>,
    pub old: Option<Original>,
    pub new: Option<Original>,
    /// The paragraph is in the copy, and how the copy has it.
    pub present: bool,
    pub mine: String,
    /// The author edited it too, it prints, a section rule names it, fill-ins chosen in it.
    pub edited: bool,
    pub printing: bool,
    pub rules: bool,
    pub fill_ins: usize,
    /// The copy already reads as the new master.
    pub already: bool,
    /// What happens without a decision: `master` or `mine`.
    pub default: &'static str,
}

impl Change {
    pub fn conflict(&self) -> bool {
        self.edited && self.present && !self.already && self.kind != "added"
    }

    fn json(&self) -> Value {
        let o = |x: &Option<Original>| x.as_ref().map(|o| json!({"style": o.0, "text": o.1}));
        json!({
            "n": self.n, "kind": self.kind, "id": self.id, "old": o(&self.old), "new": o(&self.new),
            "present": self.present, "mine": self.mine, "edited": self.edited, "printing": self.printing,
            "rules": self.rules, "fillIns": self.fill_ins, "already": self.already, "conflict": self.conflict(), "default": self.default,
        })
    }
}

/// The comparison of a copy with a master: the changes, and the id each new master paragraph has.
pub struct Comparison {
    pub changes: Vec<Change>,
    pub ids: Vec<String>,
    pub old_version: String,
}

/// The next free `pNNNN` above every id in use or recorded.
fn next_ids(doc: &Document, entries: &[Entry]) -> impl FnMut() -> String {
    let num = |id: &str| id.strip_prefix('p').and_then(|n| n.parse::<u32>().ok());
    let top = edits::originals(doc).keys().filter_map(|k| num(k)).chain(entries.iter().filter_map(|e| num(&e.id))).max().unwrap_or(0);
    let mut next = top;
    move || {
        next = next.saturating_add(1);
        format!("p{next:04}")
    }
}

pub fn compare(doc: &Document, master: &Master) -> Result<Comparison, CmdError> {
    let old = master_order(doc);
    if old.is_empty() {
        return Err(CmdError::Failed("this copy has no record of its master's wording to compare with".into()));
    }
    let entries = sections::outline(doc);
    let by_id: HashMap<&str, &Entry> = entries.iter().map(|e| (e.id.as_str(), e)).collect();
    let all_fields = fields::fields(doc);
    // Edited by the author: against the recorded wording, or (older copies) the fingerprints.
    let edited_ids: HashSet<String> = edits::origins(doc).0.into_iter().filter(|(_, o)| *o == edits::Origin::Modified).map(|(e, _)| e.id).collect();
    let named: HashSet<String> = super::rules::rules(doc)
        .iter()
        .flat_map(|r| r.when.iter().flat_map(|c| [c.on.clone(), c.off.clone()]).flatten().chain(std::iter::once(r.target.clone())))
        .collect();
    // An older copy (no recorded wording) has its master's fingerprints instead.
    let fallback = edits::originals(doc).is_empty();
    let base = edits::baseline(doc);
    let master_unchanged = |id: &str, new: Option<&Original>| {
        fallback
            && new.is_some_and(|n| base.get(id).is_some_and(|fp| *fp == fingerprint(n.1.as_bytes()) || *fp == fingerprint(normal(&n.1).as_bytes())))
    };
    let old_texts: Vec<Original> = old.iter().map(|(_, o)| o.clone()).collect();
    let steps = line_up(&old_texts, &master.paras)?;
    let mut fresh = next_ids(doc, &entries);
    let mut ids = vec![String::new(); master.paras.len()];
    let mut changes = Vec::new();
    for step in steps {
        let (kind, i, j) = match step {
            Step::Same(i, j) => {
                if let (Some(slot), Some((id, _))) = (ids.get_mut(j), old.get(i)) {
                    *slot = id.clone();
                }
                continue;
            }
            Step::Changed(i, j) => ("changed", Some(i), Some(j)),
            Step::Added(j) => ("added", None, Some(j)),
            Step::Removed(i) => ("removed", Some(i), None),
        };
        let id = match i.and_then(|i| old.get(i)) {
            Some((id, _)) => id.clone(),
            None => fresh(),
        };
        if let Some(slot) = j.and_then(|j| ids.get_mut(j)) {
            *slot = id.clone();
        }
        let old_o = i.and_then(|i| old.get(i)).map(|(_, o)| o.clone());
        let new_o = j.and_then(|j| master.paras.get(j)).cloned();
        // Edited here, not in the master: no change from the master.
        if kind == "changed" && master_unchanged(&id, new_o.as_ref()) {
            continue;
        }
        let e = by_id.get(id.as_str()).copied();
        let mine = e.map(|e| edits::base_text(doc, e)).unwrap_or_default();
        let edited = e.is_some() && (old_o.as_ref().is_some_and(|o| normal(&o.1) != mine) || edited_ids.contains(&id));
        let already = e.is_some() && new_o.as_ref().is_some_and(|n| normal(&n.1) == mine);
        let block = e.and_then(|e| e.range.start.path.0.first().copied());
        let fill_ins =
            all_fields.iter().filter(|f| f.state != FieldState::Open && f.range.start.path.0.first().copied() == block && block.is_some()).count();
        let mut c = Change {
            n: changes.len() + 1,
            kind,
            id: id.clone(),
            j,
            old: old_o,
            new: new_o,
            present: e.is_some(),
            mine,
            edited,
            printing: e.is_some_and(|e| e.on),
            rules: named.contains(&id),
            fill_ins,
            already,
            default: "master",
        };
        if c.conflict() || (kind == "removed" && c.printing) {
            c.default = "mine";
        }
        changes.push(c);
    }
    let old_version = doc
        .custom_prop(MASTER_PROP)
        .and_then(|m| serde_json::from_str::<Value>(m).ok())
        .and_then(|m| m.get("version").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_default();
    Ok(Comparison { changes, ids, old_version })
}

fn compare_cmd(s: &mut Session, v: &Value) -> CmdResult {
    let master = read_master(&master_path(&s.doc, v)?)?;
    let c = compare(&s.doc, &master)?;
    let count = |k: &str| c.changes.iter().filter(|x| x.kind == k).count();
    Ok(json!({
        "master": master.path, "version": master.version, "fingerprint": master.fingerprint, "from": c.old_version,
        "changed": count("changed"), "added": count("added"), "removed": count("removed"),
        "conflicts": c.changes.iter().filter(|x| x.conflict()).count(),
        "changes": c.changes.iter().map(Change::json).collect::<Vec<_>>(),
    }))
}

// ---------------------------------------------------------------------------------------------
// Applying

fn block_of(doc: &Document, id: &str) -> Option<usize> {
    sections::outline(doc).iter().find(|e| e.id == id).and_then(|e| e.range.start.path.0.first()).map(|b| *b as usize)
}

/// Give the paragraph at body index `b` the style `style`.
fn set_style(doc: &mut Document, b: usize, style: &str) {
    if let Some(block) = doc.body.get_mut(b)
        && let Block::Para(p) = Arc::make_mut(block)
        && p.props.style.as_deref() != Some(style)
    {
        p.props.style = (!style.is_empty()).then(|| style.to_string());
    }
}

/// What applying did with one change.
struct Done {
    took: Vec<Value>,
    kept: usize,
    reset: Vec<String>,
}

fn take_change(s: &mut Session, c: &Change, ids: &[String], done: &mut Done) -> Result<(), CmdError> {
    match c.kind {
        "changed" => {
            let (Some(new), true, false) = (&c.new, c.present, c.already) else { return Ok(()) };
            let entries = sections::outline(&s.doc);
            let Some(e) = entries.iter().find(|e| e.id == c.id) else { return Ok(()) };
            edits::set_paragraph_text(&mut s.doc, e, &new.1)?;
            if let Some(b) = block_of(&s.doc, &c.id) {
                set_style(&mut s.doc, b, &new.0);
            }
            if c.fill_ins > 0 {
                done.reset.push(c.id.clone());
            }
        }
        "added" => {
            let Some(new) = &c.new else { return Ok(()) };
            // After the nearest earlier master paragraph the copy has; else before the first.
            let j = c.j.unwrap_or(0);
            let anchor = (0..j).rev().filter_map(|k| ids.get(k)).find_map(|id| block_of(&s.doc, id));
            let at = match anchor {
                Some(b) => b + 1,
                None => sections::outline(&s.doc).first().and_then(|e| e.range.start.path.0.first()).map_or(0, |b| *b as usize),
            };
            edits::insert_item(s, at, &new.0, &new.1, &c.id, Default::default())?;
        }
        _ => {
            if !c.present {
                return Ok(());
            }
            let Some(b) = block_of(&s.doc, &c.id) else { return Ok(()) };
            if s.doc.body.get(b).and_then(|x| x.as_para()).is_some_and(|p| p.section.is_some()) {
                // It ends a page layout section: kept, as the project's own.
                done.kept += 1;
                return Ok(());
            }
            s.doc.body.remove(b);
            s.doc.ensure_nonempty();
        }
    }
    done.took.push(json!({"n": c.n, "kind": c.kind, "id": c.id}));
    Ok(())
}

fn apply_cmd(s: &mut Session, v: &Value) -> CmdResult {
    let master = read_master(&master_path(&s.doc, v)?)?;
    if let Some(fp) = p::str(v, "fingerprint")
        && fp != master.fingerprint
    {
        return Err(CmdError::Failed("the master changed since it was compared: compare again".into()));
    }
    let cmp = compare(&s.doc, &master)?;
    let mut decisions: BTreeMap<usize, bool> = BTreeMap::new();
    if let Some(d) = v.get("decisions") {
        let map = d.as_object().ok_or_else(|| CmdError::Params("`decisions` must map change numbers to \"master\" or \"mine\"".into()))?;
        for (k, x) in map {
            let n: usize = k.trim().parse().map_err(|_| CmdError::Params(format!("{k:?} is not a change number")))?;
            if !cmp.changes.iter().any(|c| c.n == n) {
                return Err(CmdError::Failed(format!("there is no change {n}: compare again")));
            }
            let take = match x.as_str() {
                Some("master") => true,
                Some("mine") => false,
                _ => return Err(CmdError::Params(format!("change {n}: say \"master\" or \"mine\""))),
            };
            decisions.insert(n, take);
        }
    }
    let had_fields = s.doc.custom_prop(fields::ORIGINALS_PROP).is_some();
    let mut done = Done { took: Vec::new(), kept: 0, reset: Vec::new() };
    for c in &cmp.changes {
        if decisions.get(&c.n).copied().unwrap_or(c.default == "master") {
            take_change(s, c, &cmp.ids, &mut done)?;
        } else {
            done.kept += 1;
        }
    }
    if had_fields {
        fields::detect(&mut s.doc)?;
    }
    record_master(&mut s.doc, &master, &cmp.ids);
    let entry = json!({
        "date": crate::cmd::now_iso(), "master": master.path, "from": cmp.old_version, "to": master.version,
        "changes": cmp.changes.len(), "tookMaster": done.took.len(), "keptMine": done.kept, "took": done.took,
    });
    let mut history: Vec<Value> = s.doc.custom_prop(HISTORY_PROP).and_then(|h| serde_json::from_str(h).ok()).unwrap_or_default();
    history.push(entry);
    let skip = history.len().saturating_sub(MAX_HISTORY);
    let history: Vec<Value> = history.into_iter().skip(skip).collect();
    if let Ok(h) = serde_json::to_string(&history) {
        s.doc.set_custom_prop(HISTORY_PROP, &h);
    }
    Ok(
        json!({"version": master.version, "changes": cmp.changes.len(), "tookMaster": done.took.len(), "keptMine": done.kept, "fillInsReset": done.reset}),
    )
}

/// Make `master` the copy's record of its master: wording, order, fingerprints, path and version.
fn record_master(doc: &mut Document, master: &Master, ids: &[String]) {
    let map: BTreeMap<&String, &Original> = ids.iter().zip(&master.paras).collect();
    if let Ok(o) = serde_json::to_string(&map) {
        doc.set_custom_prop(edits::ORIGINALS_PROP, &o);
    }
    doc.set_custom_prop(ORDER_PROP, &ids.join(","));
    let baseline: Vec<String> = ids.iter().zip(&master.paras).map(|(id, o)| format!("{id}:{}", fingerprint(normal(&o.1).as_bytes()))).collect();
    doc.set_custom_prop(BASELINE_PROP, &baseline.join(","));
    doc.set_custom_prop(MASTER_PROP, &json!({"path": master.path, "version": master.version, "fingerprint": master.fingerprint}).to_string());
}

fn history_cmd(s: &mut Session, _: &Value) -> CmdResult {
    let history: Vec<Value> = s.doc.custom_prop(HISTORY_PROP).and_then(|h| serde_json::from_str(h).ok()).unwrap_or_default();
    Ok(json!({"count": history.len(), "updates": history}))
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

    fn doc(body: &[(&str, &str)]) -> Document {
        let mut d = Document::new();
        d.body = body.iter().map(|(s, t)| para(s, t)).collect();
        d
    }

    const V1: [(&str, &str); 9] = [
        ("SCT", "SECTION 09 2900 - GYPSUM BOARD"),
        ("PRT", "GENERAL"),
        ("ART", "SUMMARY"),
        ("PR1", "Section includes gypsum board for interior walls."),
        ("PR1", "Section includes tile backing panels."),
        ("ART", "SUBMITTALS"),
        ("PR1", "Product data for each type of panel."),
        ("PR1", "Samples of trim accessories."),
        ("EOS", "END OF SECTION 09 2900"),
    ];

    fn run(s: &mut Session, id: &str, v: Value) -> Value {
        s.run(id, &v).unwrap_or_else(|e| panic!("{id}: {e}"))
    }

    /// A project copy of V1 (through spec.project.addSection), opened, with the master's path.
    fn copy(name: &str) -> (std::path::PathBuf, Session) {
        let dir = std::env::temp_dir().join(format!("speccraft-update-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("project")).unwrap();
        let m = dir.join("master.docx");
        crate::io::save_path(&m, &doc(&V1)).unwrap();
        let path = dir.join("project").join("P.speccraft");
        let mut s = Session::new(Document::new());
        run(&mut s, "spec.project.new", json!({"path": path.to_string_lossy(), "name": "P"}));
        run(&mut s, "spec.project.addSection", json!({"path": path.to_string_lossy(), "master": m.to_string_lossy(), "file": "copy.docx"}));
        let bytes = crate::io::read_document(&dir.join("project").join("copy.docx")).unwrap();
        let s = Session::new(crate::io::open_bytes("copy.docx", &bytes).unwrap());
        (dir, s)
    }

    fn texts(s: &Session) -> Vec<(String, String)> {
        sections::outline(&s.doc).iter().map(|e| (e.id.clone(), edits::base_text(&s.doc, e))).collect()
    }

    #[test]
    fn lining_up_finds_changed_added_and_removed_paragraphs() {
        let o = |s: &str, t: &str| Original(s.into(), t.into());
        let old =
            [o("PR1", "Provide gypsum board panels."), o("PR1", "Provide tile backing."), o("PR1", "Provide trim."), o("PR1", "Provide sealant.")];
        let new = [
            o("PR1", "Provide gypsum board panels, Type X."),
            o("PR1", "Provide trim."),
            o("PR1", "Provide acoustical sealant."),
            o("PR1", "Provide corner guards."),
        ];
        let steps = line_up(&old, &new).unwrap();
        assert_eq!(steps, [Step::Changed(0, 0), Step::Removed(1), Step::Same(2, 1), Step::Changed(3, 2), Step::Added(3)]);
        assert_eq!(line_up(&old, &old).unwrap().iter().filter(|s| !matches!(s, Step::Same(..))).count(), 0);
    }

    #[test]
    fn an_update_keeps_choices_and_edits_and_asks_about_conflicts() {
        let (dir, mut s) = copy("apply");
        // The author's work: two paragraphs on, one edited.
        run(&mut s, "spec.activate", json!({"ids": ["p0002", "p0003", "p0004", "p0005", "p0006", "p0007"]}));
        let e = sections::outline(&s.doc).into_iter().find(|e| e.id == "p0005").unwrap();
        edits::set_paragraph_text(&mut s.doc, &e, "Section includes cement backing panels.").unwrap();
        // The newer master: p0004 reworded, p0005 reworded too (a conflict), a new paragraph after
        // p0007, and p0008 removed.
        let mut v2 = V1.to_vec();
        v2[3] = ("PR1", "Section includes gypsum board for interior walls and ceilings.");
        v2[4] = ("PR1", "Section includes tile backing panels and joint treatment.");
        v2[7] = ("PR1", "Shop drawings of control joint layout.");
        v2.insert(7, ("PR1", "Product certificates for each type of panel."));
        let m = dir.join("master.docx");
        crate::io::save_path(&m, &doc(&v2)).unwrap();

        let c = run(&mut s, "spec.update.compare", json!({}));
        assert_eq!((c["changed"].clone(), c["added"].clone(), c["conflicts"].clone()), (json!(2), json!(2), json!(1)), "{c}");
        let ch = c["changes"].as_array().unwrap();
        let by = |id: &str| ch.iter().find(|x| x["id"] == id).unwrap().clone();
        assert_eq!((by("p0004")["printing"].clone(), by("p0004")["default"].clone()), (json!(true), json!("master")));
        assert_eq!((by("p0005")["conflict"].clone(), by("p0005")["default"].clone()), (json!(true), json!("mine")));
        assert_eq!(by("p0008")["kind"], "removed", "{c}");
        assert_eq!(by("p0008")["default"], "master", "it was off, so removing it changes nothing printed");
        let added: Vec<&Value> = ch.iter().filter(|x| x["kind"] == "added").collect();
        assert_eq!(added.len(), 2, "the certificate paragraph, and the shop drawings one replacing samples");
        assert!(added.iter().all(|x| x["id"].as_str().unwrap() > "p0009"), "new master paragraphs get new ids");

        // Take the defaults, but keep the project's own way for p0004.
        let n4 = by("p0004")["n"].to_string();
        let fp = c["fingerprint"].clone();
        let r = run(&mut s, "spec.update.apply", json!({"fingerprint": fp, "decisions": {n4: "mine"}}));
        assert_eq!((r["tookMaster"].clone(), r["keptMine"].clone()), (json!(3), json!(2)), "{r}");
        let t = texts(&s);
        let get = |id: &str| t.iter().find(|(i, _)| i == id).map(|(_, x)| x.clone());
        assert_eq!(get("p0004").unwrap(), "Section includes gypsum board for interior walls.", "kept the project's");
        assert_eq!(get("p0005").unwrap(), "Section includes cement backing panels.", "the conflict kept the author's edit");
        assert_eq!(get("p0008"), None, "removed with the master");
        let new_ids: Vec<&String> = t.iter().map(|(i, _)| i).filter(|i| i.as_str() > "p0009").collect();
        assert_eq!(new_ids.len(), 2);
        // New paragraphs come in switched off, in the master's place; choices are unchanged.
        let st = run(&mut s, "spec.outline", json!({}));
        let item = |id: &str| st["items"].as_array().unwrap().iter().find(|x| x["id"] == id).unwrap().clone();
        assert_eq!(item(new_ids[0])["state"], "off");
        assert_eq!(item("p0004")["state"], "on");
        assert_eq!(t.iter().position(|(i, _)| i == new_ids[0]), Some(7), "after p0007: {t:?}");
        // The copy now compares with the new master: what it kept shows as modified.
        let c = run(&mut s, "spec.update.compare", json!({}));
        assert_eq!(c["changes"].as_array().unwrap().len(), 0, "{c}");
        let o = run(&mut s, "spec.origin", json!({}));
        let origin = |id: &str| o["items"].as_array().unwrap().iter().find(|x| x["id"] == id).unwrap()["origin"].clone();
        assert_eq!((origin("p0004"), origin("p0005"), origin("p0006")), (json!("modified"), json!("modified"), json!("master")));
        let h = run(&mut s, "spec.update.history", json!({}));
        assert_eq!((h["count"].clone(), h["updates"][0]["tookMaster"].clone()), (json!(1), json!(3)));
        // A changed master after comparing is refused.
        assert!(s.run("spec.update.apply", &json!({"fingerprint": "nope"})).is_err());
        assert!(s.run("spec.update.apply", &json!({"decisions": {"99": "master"}})).is_err(), "there is no change 99");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_removed_printing_paragraph_is_kept_unless_the_author_says() {
        let (dir, mut s) = copy("removed");
        run(&mut s, "spec.activate", json!({"ids": ["p0002", "p0003", "p0004", "p0005"]}));
        let mut v2 = V1.to_vec();
        v2.remove(4);
        crate::io::save_path(&dir.join("master.docx"), &doc(&v2)).unwrap();
        let c = run(&mut s, "spec.update.compare", json!({}));
        assert_eq!(
            (c["removed"].clone(), c["changes"][0]["printing"].clone(), c["changes"][0]["default"].clone()),
            (json!(1), json!(true), json!("mine"))
        );
        run(&mut s, "spec.update.apply", json!({}));
        assert!(texts(&s).iter().any(|(i, _)| i == "p0005"), "kept by default");
        let o = run(&mut s, "spec.origin", json!({}));
        assert_eq!(o["items"].as_array().unwrap().iter().find(|x| x["id"] == "p0005").unwrap()["origin"], "added", "now the project's own");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_new_master_paragraph_not_taken_can_be_restored_later() {
        let (dir, mut s) = copy("declined");
        let mut v2 = V1.to_vec();
        v2.insert(5, ("PR1", "Section includes shaft wall assemblies."));
        crate::io::save_path(&dir.join("master.docx"), &doc(&v2)).unwrap();
        let c = run(&mut s, "spec.update.compare", json!({}));
        let id = c["changes"][0]["id"].as_str().unwrap().to_string();
        run(&mut s, "spec.update.apply", json!({"decisions": {"1": "mine"}}));
        assert!(!texts(&s).iter().any(|(i, _)| *i == id));
        let o = run(&mut s, "spec.origin", json!({}));
        assert!(o["removed"].as_array().unwrap().iter().any(|x| x["id"] == id.as_str()), "{o}");
        run(&mut s, "spec.restore", json!({"id": id}));
        let t = texts(&s);
        assert_eq!(t.iter().position(|(i, _)| *i == id), Some(5), "back in the master's place: {t:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn taking_a_new_wording_resets_its_fill_ins_and_says_so() {
        let dir = std::env::temp_dir().join(format!("speccraft-update-fields-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("project")).unwrap();
        let m = dir.join("master.docx");
        let mut v1 = V1.to_vec();
        v1[4] = ("PR1", "Provide [silicone] [urethane] sealant.");
        crate::io::save_path(&m, &doc(&v1)).unwrap();
        let path = dir.join("project").join("P.speccraft");
        let mut s = Session::new(Document::new());
        run(&mut s, "spec.project.new", json!({"path": path.to_string_lossy(), "name": "P"}));
        run(&mut s, "spec.project.addSection", json!({"path": path.to_string_lossy(), "master": m.to_string_lossy(), "file": "copy.docx"}));
        let bytes = crate::io::read_document(&dir.join("project").join("copy.docx")).unwrap();
        let mut s = Session::new(crate::io::open_bytes("copy.docx", &bytes).unwrap());
        let f = run(&mut s, "spec.fields.list", json!({}));
        let fid = f["fields"][0]["id"].as_str().unwrap().to_string();
        run(&mut s, "spec.fields.set", json!({"id": fid, "value": "silicone"}));
        // Unchanged in the master: the choice stays.
        let mut v2 = v1.clone();
        v2[3] = ("PR1", "Section includes gypsum board for walls and ceilings.");
        crate::io::save_path(&m, &doc(&v2)).unwrap();
        let r = run(&mut s, "spec.update.apply", json!({}));
        assert_eq!(r["fillInsReset"], json!([]), "{r}");
        assert_eq!(run(&mut s, "spec.fields.list", json!({}))["fields"][0]["state"], "set");
        // Reworded in the master: taking it resets the choice, which is open again.
        v2[4] = ("PR1", "Provide [silicone] [urethane] [polysulfide] sealant.");
        crate::io::save_path(&m, &doc(&v2)).unwrap();
        let c = run(&mut s, "spec.update.compare", json!({}));
        assert_eq!(c["changes"][0]["fillIns"], 1, "{c}");
        let r = run(&mut s, "spec.update.apply", json!({}));
        assert_eq!(r["fillInsReset"], json!(["p0005"]), "{r}");
        let f = run(&mut s, "spec.fields.list", json!({}));
        let field = f["fields"].as_array().unwrap().iter().find(|x| x["original"].as_str().is_some_and(|o| o.contains("polysulfide"))).cloned();
        assert_eq!(field.map(|x| x["state"].clone()), Some(json!("open")), "{f}");
        assert_eq!(run(&mut s, "spec.update.history", json!({}))["count"], 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_older_copy_without_recorded_wording_still_compares() {
        let (dir, mut s) = copy("older");
        // As a copy made before the wording was recorded: no Original or Order.
        s.doc.remove_custom_prop(edits::ORIGINALS_PROP);
        s.doc.remove_custom_prop(ORDER_PROP);
        let e = sections::outline(&s.doc).into_iter().find(|e| e.id == "p0007").unwrap();
        edits::set_paragraph_text(&mut s.doc, &e, "Product data for each panel type.").unwrap();
        let mut v2 = V1.to_vec();
        v2[3] = ("PR1", "Section includes gypsum board for interior walls and ceilings.");
        crate::io::save_path(&dir.join("master.docx"), &doc(&v2)).unwrap();
        let c = run(&mut s, "spec.update.compare", json!({}));
        let ch = c["changes"].as_array().unwrap();
        assert_eq!(ch.len(), 1, "only the master's change, not the author's edit of p0007: {c}");
        assert_eq!((ch[0]["id"].clone(), ch[0]["old"]["text"].clone()), (json!("p0004"), json!("Section includes gypsum board for interior walls.")));
        run(&mut s, "spec.update.apply", json!({}));
        assert!(s.doc.custom_prop(edits::ORIGINALS_PROP).is_some(), "recorded from now on");
        let o = run(&mut s, "spec.origin", json!({}));
        let origin = |id: &str| o["items"].as_array().unwrap().iter().find(|x| x["id"] == id).unwrap()["origin"].clone();
        assert_eq!((origin("p0004"), origin("p0007")), (json!("master"), json!("modified")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn copies_without_a_master_record_are_refused() {
        let mut s = Session::new(doc(&V1));
        assert!(s.run("spec.update.compare", &json!({})).is_err());
        assert!(s.run("spec.update.compare", &json!({"master": "/no/such/master.docx"})).is_err());
    }
}
