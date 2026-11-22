//! Project settings for printing (milestones M20–M22). Added by the Speccraft fork; not part of
//! the original WordCraft.
//!
//! - **Sync** (M20): `spec.project.sync` brings the section on screen into line with its project:
//!   global terms show the project's values, section references take the project's titles, the
//!   project's units and Auto-Select New apply, and the project's header and footer (M21) replace
//!   the section's own (unless the section keeps its own). Publishing does the same to its copy.
//! - **Header and footer** (M21): lines on the left and on the right of the page with fields:
//!   `{Section Name}`, `{Section ID}`, `{Page}`, `{Pages}` (pages of the section) and any project
//!   value by name (`{Project ID}`, `{Rev No}`, `{Deliverable Date}`).
//! - **Print options** (M22): drafts (author notes in, a watermark), keep with next, widow and
//!   orphan control, PDF bookmarks, every choice's options shown, END OF SECTION with or without
//!   its number, hyperlinks, each section on an odd page with blank pages marked, file names.
//!   Kept in the project file; `spec.project.settings` changes them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use wordcraft_doc::para::{InlineObject, OBJ};
use wordcraft_doc::props::{TabAlign, TabLeader, TabStop, Underline};
use wordcraft_doc::{Block, CharProps, Document, Paragraph, PartKind, Pos, Rgb, StoryRef, Watermark};

use super::project::{self, ProjectFile};
use super::sections::{self, Kind};
use crate::{CmdError, CmdResult, CommandSpec, Session, p};

const LOC: &str = "Specs › Project";
/// Document property: a fingerprint of the project header and footer last put in the section.
pub const FURNITURE_PROP: &str = "Speccraft.Furniture";
/// Most lines on each side of a header or footer.
const MAX_LINES: usize = 6;
/// Longest header or footer line, in characters.
const MAX_LINE: usize = 300;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("spec.project.settings", "Project Print and Editing Settings", LOC, settings_cmd).params(
            r#"{"path": string, "print"?: {option: value} (changed options only), "header"?: {"left": [line], "right": [line]} | null, "footer"?: same, "autoSelectNew"?: bool | null, "units"?: "both" | "us" | "metric" | null, "file"?: string and "ownFurniture"?: bool (that section keeps its own header and footer)}"#,
        ),
        CommandSpec::new("spec.project.sync", "Bring Section Up to Date with Project", LOC, sync_cmd)
            .params(r#"{"path"?: string (the project; default: the one beside the section)}"#),
        CommandSpec::new("spec.project.standards", "Reference Standards Report", LOC, standards_cmd).params(r#"{"path": string}"#).pure(),
        CommandSpec::new("spec.project.setStandard", "Set Reference Standard", LOC, set_standard_cmd)
            .params(r#"{"path": string, "key": string ("ASTM C1396/C1396M"), "title"?: string, "edition"?: string}"#),
    ]
}

// ---------------------------------------------------------------------------------------------
// Settings

/// A header or footer: lines on the left and on the right of the page.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Furniture {
    #[serde(default)]
    pub left: Vec<String>,
    #[serde(default)]
    pub right: Vec<String>,
}

/// Print and export options (M22).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PrintOptions {
    /// Drafts: author notes print too, under a watermark.
    pub draft: bool,
    pub watermark: String,
    /// Titles, Parts and Articles stay with what follows; END OF SECTION is never alone.
    pub keep_with_next: bool,
    pub widow_control: bool,
    /// PDF bookmarks: "sections", "parts" (sections and their Parts) or "none".
    pub bookmarks: String,
    /// Every choice prints all its options, the chosen ones underlined (for review).
    pub show_all_choices: bool,
    /// END OF SECTION as the master has it ("asis"), with the section number ("id") or without ("plain").
    pub end_of_section: String,
    /// Hyperlinks stay links; off, they print as plain text.
    pub hyperlinks: bool,
    /// In the manual, each section starts on an odd page; the blank page before it says this.
    pub odd_pages: bool,
    pub blank_page_text: String,
    /// The blank page has the header and footer of the section before it.
    pub blank_page_furniture: bool,
    /// Published file names: "file" (the section file's), "number" or "numberTitle".
    pub file_names: String,
    /// The manual's page numbers: "restart" in each section or "continue".
    pub page_numbers: String,
    /// The manual starts with a table of contents.
    pub contents: bool,
    /// "pdf", "docx" or "both".
    pub format: String,
    /// Attached PDFs (M23): "after" each section, at the "end" of the manual, or "none".
    pub attachments: String,
}

impl Default for PrintOptions {
    fn default() -> Self {
        PrintOptions {
            draft: false,
            watermark: "DRAFT".into(),
            keep_with_next: true,
            widow_control: true,
            bookmarks: "sections".into(),
            show_all_choices: false,
            end_of_section: "asis".into(),
            hyperlinks: true,
            odd_pages: false,
            blank_page_text: "This page intentionally left blank.".into(),
            blank_page_furniture: true,
            file_names: "file".into(),
            page_numbers: "restart".into(),
            contents: true,
            format: "pdf".into(),
            attachments: "after".into(),
        }
    }
}

impl PrintOptions {
    /// Refuse values outside their lists.
    pub fn check(&self) -> Result<(), CmdError> {
        let one_of = |name: &str, v: &str, ok: &[&str]| {
            if ok.contains(&v) { Ok(()) } else { Err(CmdError::Params(format!("`{name}` must be one of {ok:?}, not {v:?}"))) }
        };
        one_of("bookmarks", &self.bookmarks, &["sections", "parts", "none"])?;
        one_of("endOfSection", &self.end_of_section, &["asis", "id", "plain"])?;
        one_of("fileNames", &self.file_names, &["file", "number", "numberTitle"])?;
        one_of("pageNumbers", &self.page_numbers, &["restart", "continue"])?;
        one_of("format", &self.format, &["pdf", "docx", "both"])?;
        one_of("attachments", &self.attachments, &["after", "end", "none"])?;
        if self.watermark.chars().count() > 60 || self.blank_page_text.chars().count() > MAX_LINE {
            return Err(CmdError::Params("`watermark` is at most 60 characters and `blankPageText` 300".into()));
        }
        Ok(())
    }

    /// The options with `changes` (an object of options) laid over them.
    pub fn merged(&self, changes: &Value) -> Result<PrintOptions, CmdError> {
        let Value::Object(m) = changes else { return Err(CmdError::Params("`print` must be an object of options".into())) };
        let mut now = serde_json::to_value(self).map_err(|e| CmdError::Failed(e.to_string()))?;
        for (k, v) in m {
            if now.get(k).is_none() {
                return Err(CmdError::Params(format!("no print option called {k:?}")));
            }
            if let Some(slot) = now.get_mut(k) {
                *slot = v.clone();
            }
        }
        let out: PrintOptions = serde_json::from_value(now).map_err(|e| CmdError::Params(format!("print options: {e}")))?;
        out.check()?;
        Ok(out)
    }
}

fn furniture_param(v: &Value) -> Result<Option<Furniture>, CmdError> {
    if v.is_null() {
        return Ok(None);
    }
    let f: Furniture = serde_json::from_value(v.clone()).map_err(|e| CmdError::Params(format!("header/footer: {e}")))?;
    if f.left.len() > MAX_LINES || f.right.len() > MAX_LINES {
        return Err(CmdError::Params(format!("at most {MAX_LINES} lines on each side")));
    }
    if f.left.iter().chain(&f.right).any(|l| l.chars().count() > MAX_LINE || l.contains(['\n', '\r', '\t', OBJ])) {
        return Err(CmdError::Params(format!("each line is one line of at most {MAX_LINE} characters, without tabs")));
    }
    Ok(if f.left.iter().chain(&f.right).all(|l| l.trim().is_empty()) { None } else { Some(f) })
}

fn settings_cmd(_: &mut Session, v: &Value) -> CmdResult {
    let path = PathBuf::from(p::req_str(v, "path")?);
    let mut f = project::load(&path)?;
    if let Some(changes) = v.get("print") {
        f.print = f.print.merged(changes)?;
    }
    if let Some(h) = v.get("header") {
        f.header = furniture_param(h)?;
    }
    if let Some(x) = v.get("footer") {
        f.footer = furniture_param(x)?;
    }
    match v.get("autoSelectNew") {
        None => {}
        Some(Value::Null) => f.editing.auto_select_new = None,
        Some(Value::Bool(b)) => f.editing.auto_select_new = Some(*b),
        Some(_) => return Err(CmdError::Params("`autoSelectNew` must be true, false or null".into())),
    }
    match v.get("units") {
        None => {}
        Some(Value::Null) => f.editing.units = None,
        Some(Value::String(u)) if matches!(u.as_str(), "both" | "us" | "metric") => f.editing.units = Some(u.clone()),
        Some(_) => return Err(CmdError::Params("`units` must be \"both\", \"us\", \"metric\" or null".into())),
    }
    if let Some(file) = p::str(v, "file") {
        let own = p::bool(v, "ownFurniture").ok_or_else(|| CmdError::Params("with `file`, give `ownFurniture`".into()))?;
        let sec =
            f.sections.iter_mut().find(|x| x.file == file).ok_or_else(|| CmdError::Failed(format!("{file:?} is not a section of this project")))?;
        if own {
            sec.extra.insert("ownFurniture".into(), json!(true));
        } else {
            sec.extra.remove("ownFurniture");
        }
    }
    project::store_file(&path, &f)?;
    Ok(json!({"print": f.print, "header": f.header, "footer": f.footer, "editing": f.editing}))
}

// ---------------------------------------------------------------------------------------------
// Sync

/// "GYPSUM BOARD" → "Gypsum Board"; titles already in mixed case stay as they are.
pub fn title_case(t: &str) -> String {
    if t.chars().any(char::is_lowercase) {
        return t.to_string();
    }
    let small = ["and", "or", "of", "for", "the", "a", "an", "in", "on", "to", "with", "by", "at"];
    t.split(' ')
        .enumerate()
        .map(|(i, w)| {
            let low = w.to_lowercase();
            if i > 0 && small.contains(&low.as_str()) {
                return low;
            }
            // Each part of "SELF-PERFORM" and "WOOD/METAL".
            let mut out = String::new();
            let mut start = true;
            for c in low.chars() {
                if start {
                    out.extend(c.to_uppercase());
                } else {
                    out.push(c);
                }
                start = matches!(c, '-' | '/' | '(');
            }
            out
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The project's section titles by number, for section references.
fn titles(f: &ProjectFile) -> serde_json::Map<String, Value> {
    f.sections.iter().filter(|s| !s.number.is_empty() && !s.title.is_empty()).map(|s| (s.number.clone(), json!(title_case(&s.title)))).collect()
}

/// Whether the section file `file` keeps its own header and footer.
fn keeps_own(f: &ProjectFile, file: Option<&str>) -> bool {
    file.and_then(|n| f.sections.iter().find(|s| s.file == n)).is_some_and(|s| s.extra.get("ownFurniture") == Some(&json!(true)))
}

/// Bring `s`'s document into line with project `f` (see the module docs). `file`: the section's
/// file name in the project. Returns what changed.
pub fn sync(s: &mut Session, f: &ProjectFile, file: Option<&str>) -> Result<Value, CmdError> {
    let values: serde_json::Map<String, Value> = project::placeholder_values(f).into_iter().map(|(k, v)| (k, json!(v))).collect();
    let terms = super::terms::terms_apply(s, &json!({"values": values}))?;
    let refs = super::terms::refs_apply(s, &json!({"titles": titles(f)}))?;
    if let Some(u) = &f.editing.units {
        super::units::units_cmd(s, &json!({"value": u}))?;
    }
    if let Some(a) = f.editing.auto_select_new
        && super::paragraphs::auto_select(&s.doc) != a
    {
        super::paragraphs::auto_select_cmd(s, &json!({"value": a}))?;
    }
    let mut furniture = false;
    if (f.header.is_some() || f.footer.is_some()) && !keeps_own(f, file) {
        let (number, title) = project::number_and_title(&s.doc);
        let stamp = project::fingerprint(serde_json::to_string(&(&f.header, &f.footer, &values, &number, &title)).unwrap_or_default().as_bytes());
        if s.doc.custom_prop(FURNITURE_PROP) != Some(stamp.as_str()) {
            put_furniture(&mut s.doc, f, &number, &title);
            s.doc.set_custom_prop(FURNITURE_PROP, &stamp);
            furniture = true;
        }
    }
    Ok(json!({"terms": terms, "refs": refs, "furniture": furniture}))
}

fn sync_cmd(s: &mut Session, v: &Value) -> CmdResult {
    let path =
        project::project_for(s.path.as_deref(), p::str(v, "path"))?.ok_or_else(|| CmdError::Failed("no project beside this section".into()))?;
    let f = project::load(&path)?;
    let file = s.path.as_deref().and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned());
    sync(s, &f, file.as_deref())
}

// ---------------------------------------------------------------------------------------------
// Header and footer (M21)

/// A header or footer line, cut into text and fields.
enum Piece {
    Text(String),
    Field(&'static str),
}

/// `line` with its `{…}` fields: section name and number, page fields, project values. Unknown
/// names are left as written.
fn pieces(line: &str, number: &str, title: &str, values: &BTreeMap<String, String>) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(a) = rest.find('{') {
        let Some(b) = rest.get(a..).and_then(|r| r.find('}')).map(|k| k + a) else { break };
        out.push(Piece::Text(rest.get(..a).unwrap_or("").to_string()));
        let name = rest.get(a + 1..b).unwrap_or("").trim();
        match name.to_lowercase().as_str() {
            "page" | "page number" => out.push(Piece::Field("PAGE")),
            "pages" | "section page count" | "section pages" => out.push(Piece::Field("SECTIONPAGES")),
            "section id" | "section number" => out.push(Piece::Text(number.to_string())),
            "section name" | "section title" => out.push(Piece::Text(title_case(title))),
            low => match values.get(low) {
                Some(v) => out.push(Piece::Text(v.clone())),
                None => out.push(Piece::Text(format!("{{{name}}}"))),
            },
        }
        rest = rest.get(b + 1..).unwrap_or("");
    }
    out.push(Piece::Text(rest.to_string()));
    out
}

/// One header or footer paragraph: `left`, a tab, `right` against the right margin.
/// `inherited`: the style's tab stops, cleared so the right side goes to the margin.
fn furniture_para(left: Vec<Piece>, right: Vec<Piece>, width: f32, style: Option<&str>, inherited: &[f32]) -> Paragraph {
    let mut text = String::new();
    let mut fields: Vec<(usize, &'static str)> = Vec::new();
    let mut add = |pieces: Vec<Piece>, text: &mut String| {
        for p in pieces {
            match p {
                Piece::Text(t) => text.push_str(&t),
                Piece::Field(f) => fields.push((text.len(), f)),
            }
        }
    };
    add(left, &mut text);
    text.push('\t');
    add(right, &mut text);
    let mut para = Paragraph::with_text(&text, CharProps::default());
    if let Some(st) = style {
        para = para.styled(st);
    }
    for (off, instr) in fields.into_iter().rev() {
        let obj = InlineObject::Field { instr: instr.into(), result: "1".into(), locked: false, dirty: true, code: false, form: String::new() };
        let _ = para.insert_object(off, obj, &CharProps::default());
    }
    let mut tabs: Vec<TabStop> = inherited
        .iter()
        .filter(|p| (**p - width).abs() > 0.5)
        .map(|p| TabStop { pos: *p, align: TabAlign::Clear, leader: TabLeader::None })
        .collect();
    tabs.push(TabStop { pos: width, align: TabAlign::Right, leader: TabLeader::None });
    para.props.tabs = Some(tabs);
    para.touch();
    para
}

/// Replace every section's headers (with `f.header`) and footers (with `f.footer`) in `doc`.
pub fn put_furniture(doc: &mut Document, f: &ProjectFile, number: &str, title: &str) {
    let values = project::placeholder_values(f);
    let width = (doc.last_section.page_w - doc.last_section.margin_left - doc.last_section.margin_right).max(72.0);
    for (furn, kind, style) in [(&f.header, PartKind::Header, "Header"), (&f.footer, PartKind::Footer, "Footer")] {
        let Some(furn) = furn else { continue };
        let style = doc.styles.get(style).is_some().then_some(style);
        let inherited: Vec<f32> = style
            .map(|st| {
                doc.styles
                    .resolve_para(&wordcraft_doc::ParaProps { style: Some(st.to_string()), ..Default::default() })
                    .tabs
                    .iter()
                    .map(|t| t.pos)
                    .collect()
            })
            .unwrap_or_default();
        let lines = furn.left.len().max(furn.right.len()).max(1);
        let blocks: Vec<Arc<Block>> = (0..lines)
            .map(|i| {
                let l = pieces(furn.left.get(i).map_or("", String::as_str), number, title, &values);
                let r = pieces(furn.right.get(i).map_or("", String::as_str), number, title, &values);
                Arc::new(Block::Para(furniture_para(l, r, width, style, &inherited)))
            })
            .collect();
        let id = doc.add_part(kind, blocks);
        let point = |set: &mut wordcraft_doc::section::HeaderSet| {
            set.default = Some(id);
            set.first = set.first.map(|_| id);
            set.even = set.even.map(|_| id);
        };
        for b in doc.body.iter_mut() {
            if b.as_para().is_some_and(|p| p.section.is_some())
                && let Block::Para(p) = Arc::make_mut(b)
                && let Some(s) = p.section.as_deref_mut()
            {
                point(if kind == PartKind::Header { &mut s.headers } else { &mut s.footers });
            }
        }
        point(if kind == PartKind::Header { &mut doc.last_section.headers } else { &mut doc.last_section.footers });
    }
}

// ---------------------------------------------------------------------------------------------
// Print options (M22)

/// Change top-level body paragraph `block` of `doc`.
fn with_para(doc: &mut Document, block: usize, f: impl FnOnce(&mut Paragraph)) {
    if let Some(b) = doc.body.get_mut(block)
        && let Block::Para(p) = Arc::make_mut(b)
    {
        f(p);
        p.touch();
    }
}

/// Every choice shows all its options, "[a] [b] [c]", the chosen ones underlined.
fn show_all_choices(doc: &mut Document) {
    let mut all: Vec<super::fields::Field> = super::fields::fields(doc)
        .into_iter()
        .filter(|f| matches!(f.kind, super::fields::FieldKind::Choice | super::fields::FieldKind::Optional))
        .collect();
    all.reverse();
    for f in all {
        let start = f.range.content_start();
        let base = doc.para_at(&start).map(|p| p.props_of_char(start.off).clone()).unwrap_or_default();
        let chosen = |i: usize, o: &str| f.state == super::fields::FieldState::Set && (f.chosen.contains(&i) || f.value == o);
        if super::fields::replace_content(doc, &f.range, "").is_err() {
            continue;
        }
        let mut at = start.clone();
        for (i, o) in f.options.iter().enumerate() {
            let sep = if i == 0 { "[" } else { " [" };
            for (t, props) in [
                (sep.to_string(), base.clone()),
                (o.clone(), if chosen(i, o) { CharProps { underline: Some(Underline::Single), ..base.clone() } } else { base.clone() }),
                ("]".to_string(), base.clone()),
            ] {
                if doc.insert_text(&at, &t, &props).is_err() {
                    break;
                }
                at = Pos { off: at.off + t.len(), ..at };
            }
        }
    }
}

/// Plain text for every hyperlink, in every story.
fn unlink(doc: &mut Document) {
    fn walk(blocks: &mut [Arc<Block>]) {
        for b in blocks {
            match Arc::make_mut(b) {
                Block::Para(p) => {
                    if p.runs.iter().any(|r| r.props.link.is_some()) {
                        for r in &mut p.runs {
                            r.props = r.props.unlinked();
                        }
                        p.touch();
                    }
                }
                Block::Table(t) => {
                    for c in t.rows.iter_mut().flat_map(|r| r.cells.iter_mut()) {
                        walk(&mut c.blocks);
                    }
                }
            }
        }
    }
    walk(&mut doc.body);
    for part in doc.parts.values_mut() {
        walk(&mut part.blocks);
    }
}

/// Apply the per-section print options to an issued copy (the manual's odd pages and the file
/// names are the manual's and publishing's). `number`: the section's number.
pub fn apply(doc: &mut Document, o: &PrintOptions, number: &str) {
    if o.show_all_choices {
        show_all_choices(doc);
    }
    let entries = sections::outline(doc);
    let block = |e: &sections::Entry| e.range.start.path.0.first().map(|b| *b as usize);
    let printing: Vec<&sections::Entry> = entries.iter().filter(|e| e.kind != Kind::Note || o.draft).collect();
    for (k, e) in printing.iter().enumerate() {
        let Some(b) = block(e) else { continue };
        let next_is_end = printing.get(k + 1).is_some_and(|n| n.kind == Kind::End);
        let heading = matches!(e.kind, Kind::Title | Kind::Part | Kind::Article);
        let level = match (e.kind, o.bookmarks.as_str()) {
            (Kind::Title, "sections" | "parts") => Some(0),
            (Kind::Part, "parts") => Some(1),
            (Kind::Title | Kind::Part, _) => Some(9),
            _ => None,
        };
        with_para(doc, b, |p| {
            if o.keep_with_next && (heading || next_is_end) {
                p.props.keep_next = Some(true);
            }
            if level.is_some() {
                p.props.outline_level = level;
            }
        });
        if e.kind == Kind::End && o.end_of_section != "asis" {
            let want = if o.end_of_section == "id" && !number.is_empty() { format!("END OF SECTION {number}") } else { "END OF SECTION".to_string() };
            let (from, to) = (e.range.content_start().off, e.range.end.off);
            with_para(doc, b, |p| {
                let props = p.props_at(from).clone();
                if p.delete(from, to).is_ok() {
                    let _ = p.insert_text(from, &want, &props);
                }
            });
        }
    }
    if o.widow_control {
        for b in 0..doc.body.len() {
            if doc.body.get(b).and_then(|x| x.as_para()).is_some_and(|p| p.props.widow_control != Some(true)) {
                with_para(doc, b, |p| p.props.widow_control = Some(true));
            }
        }
    }
    if !o.hyperlinks {
        unlink(doc);
    }
    if o.draft {
        doc.settings.watermark = Some(Watermark {
            text: if o.watermark.trim().is_empty() { "DRAFT".into() } else { o.watermark.trim().to_string() },
            diagonal: true,
            semitransparent: true,
            color: Rgb::parse("C0C0C0").unwrap_or_default(),
            ..Default::default()
        });
    }
}

/// The published file's name (no extension) for a section (M22 `fileNames`).
pub fn file_stem(o: &PrintOptions, file: &str, number: &str, title: &str) -> String {
    let own = Path::new(file).file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let clean =
        |t: String| t.chars().filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')).collect::<String>().trim().to_string();
    let named = match o.file_names.as_str() {
        "number" if !number.is_empty() => clean(number.to_string()),
        "numberTitle" if !number.is_empty() => clean(format!("{number} {}", title_case(title))),
        _ => own,
    };
    if o.draft { format!("{named} (Draft)") } else { named }
}

// ---------------------------------------------------------------------------------------------
// Reference standards report (M20)

fn standards_cmd(_: &mut Session, v: &Value) -> CmdResult {
    let path = PathBuf::from(p::req_str(v, "path")?);
    let f = project::load(&path)?;
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    // key → (sections citing it in printing paragraphs, sections citing it only in off ones)
    let mut by: BTreeMap<String, (Vec<String>, Vec<String>)> = BTreeMap::new();
    let mut unread = Vec::new();
    for sec in f.sections.iter().filter(|s| s.included) {
        let doc = crate::io::read_document(&dir.join(&sec.file)).and_then(|b| crate::io::open_bytes(&sec.file, &b));
        let Ok(doc) = doc else {
            unread.push(sec.file.clone());
            continue;
        };
        for (key, prints) in super::terms::standards_in(&doc) {
            let row = by.entry(key).or_default();
            let list = if prints { &mut row.0 } else { &mut row.1 };
            if !list.contains(&sec.number) {
                list.push(sec.number.clone());
            }
        }
    }
    // Standards the project lists that no section cites any more stay in the report.
    for k in f.standards.keys() {
        by.entry(k.clone()).or_default();
    }
    let rows: Vec<Value> = by
        .into_iter()
        .map(|(key, (on, off))| {
            let info = f.standards.get(&key).cloned().unwrap_or_default();
            json!({"key": key, "title": info.title, "edition": info.edition, "sections": on, "offOnly": off, "cited": !on.is_empty()})
        })
        .collect();
    Ok(json!({"count": rows.len(), "standards": rows, "unread": unread}))
}

fn set_standard_cmd(_: &mut Session, v: &Value) -> CmdResult {
    let path = PathBuf::from(p::req_str(v, "path")?);
    let key = p::req_str(v, "key")?.trim().to_string();
    if key.is_empty() || key.chars().count() > 120 {
        return Err(CmdError::Params("`key` is 1 to 120 characters".into()));
    }
    let mut f = project::load(&path)?;
    let entry = f.standards.entry(key.clone()).or_default();
    for (k, slot) in [("title", &mut entry.title), ("edition", &mut entry.edition)] {
        if let Some(x) = p::str(v, k) {
            if x.chars().count() > 300 || x.contains(['\n', '\r']) {
                return Err(CmdError::Params(format!("`{k}` is one line of at most 300 characters")));
            }
            *slot = x.trim().to_string();
        }
    }
    if entry.title.is_empty() && entry.edition.is_empty() {
        f.standards.remove(&key);
    }
    if f.standards.len() > 5_000 {
        return Err(CmdError::Params("a project lists at most 5000 standards".into()));
    }
    project::store_file(&path, &f)?;
    Ok(json!({"key": key, "standard": f.standards.get(&key)}))
}

/// Body text of `doc` with objects left out (for tests).
pub fn body_text(doc: &Document) -> String {
    doc.plain_text(StoryRef::Body).replace(OBJ, "")
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
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

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("speccraft-print-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("masters")).unwrap();
        d
    }

    fn master(number: &str, title: &str, extra: usize) -> Document {
        let mut d = Document::new();
        d.body = vec![
            para("SCT", &format!("SECTION {number} - {title}")),
            para("PRT", "GENERAL"),
            para("ART", "SUMMARY"),
            para("CMT", "Retain for projects with existing finishes."),
            para("PR1", "Work for <Insert Project Name>: [primed] [painted] finish."),
            para("PR1", "Related requirements: Section 07 9200 \"Sealants\"."),
        ];
        for i in 0..extra {
            d.body.push(para("PR1", &format!("Requirement {i}, written long enough to take up a good part of a line on the printed page.")));
        }
        d.body.push(para("EOS", &format!("END OF SECTION {number}")));
        d
    }

    /// A project with two sections, every paragraph of each switched on.
    fn project(name: &str) -> (PathBuf, PathBuf) {
        let d = scratch(name);
        let proj = d.join("project");
        std::fs::create_dir_all(&proj).unwrap();
        let path = proj.join("P.speccraft");
        let mut s = Session::new(Document::new());
        run(&mut s, "spec.project.new", json!({"path": path.to_string_lossy(), "name": "Riverside Library"}));
        for (n, t, extra) in [("09 2900", "GYPSUM BOARD", 0), ("07 9200", "JOINT SEALANTS", 60)] {
            let m = d.join("masters").join(format!("{}.docx", n.replace(' ', "")));
            crate::io::save_path(&m, &master(n, t, extra)).unwrap();
            run(&mut s, "spec.project.addSection", json!({"path": path.to_string_lossy(), "master": m.to_string_lossy()}));
        }
        let info = run(&mut s, "spec.project.info", json!({"path": path.to_string_lossy()}));
        for sec in info["sections"].as_array().unwrap() {
            let file = proj.join(sec["file"].as_str().unwrap());
            let mut x = Session::new(crate::io::open_bytes("x.docx", &std::fs::read(&file).unwrap()).unwrap());
            x.path = Some(file.clone());
            let ids: Vec<String> = sections::outline(&x.doc).iter().filter(|e| e.kind != Kind::Note).map(|e| e.id.clone()).collect();
            run(&mut x, "spec.activate", json!({"ids": ids}));
            run(&mut x, "spec.fields.detect", json!({}));
            crate::io::save_path(&file, &x.doc).unwrap();
        }
        (path, proj)
    }

    fn open(proj: &Path, file: &str) -> Session {
        let p = proj.join(file);
        let mut s = Session::new(crate::io::open_bytes(file, &std::fs::read(&p).unwrap()).unwrap());
        s.path = Some(p);
        s
    }

    fn part_text(d: &Document, id: Option<u32>) -> String {
        d.parts
            .get(&id.unwrap())
            .unwrap()
            .blocks
            .iter()
            .filter_map(|b| b.as_para())
            .map(|p| p.plain_text().replace(OBJ, ""))
            .collect::<Vec<_>>()
            .join(" | ")
    }

    #[test]
    fn settings_are_checked_and_kept() {
        let (path, _) = project("settings");
        let mut s = Session::new(Document::new());
        let p = path.to_string_lossy().to_string();
        let r = run(&mut s, "spec.project.settings", json!({"path": p, "print": {"draft": true, "endOfSection": "id"}, "units": "us"}));
        assert_eq!((r["print"]["draft"].clone(), r["print"]["bookmarks"].clone()), (json!(true), json!("sections")));
        assert_eq!(r["editing"]["units"], "us");
        assert!(s.run("spec.project.settings", &json!({"path": p, "print": {"bookmarks": "all"}})).is_err());
        assert!(s.run("spec.project.settings", &json!({"path": p, "print": {"colour": true}})).is_err());
        assert!(s.run("spec.project.settings", &json!({"path": p, "footer": {"left": ["a\tb"]}})).is_err());
        let again = project::load(&path).unwrap();
        assert!(again.print.draft);
        assert_eq!(again.print.end_of_section, "id");
    }

    #[test]
    fn sync_brings_terms_references_units_and_the_footer() {
        let (path, proj) = project("sync");
        let p = path.to_string_lossy().to_string();
        let mut s = Session::new(Document::new());
        run(
            &mut s,
            "spec.project.set",
            json!({"path": p, "values": {"Project ID": "LANL-0042", "Rev No": "2", "Deliverable Date": "December 2026"}}),
        );
        run(
            &mut s,
            "spec.project.settings",
            json!({"path": p, "units": "us", "autoSelectNew": false, "footer": {"left": ["LANL Project I.D. {Project ID}", "Rev. {Rev No}, {Deliverable Date}"], "right": ["{Section Name}", "{Section ID} - {Page} of {Pages}"]}}),
        );
        let info = run(&mut s, "spec.project.info", json!({"path": p}));
        let file = info["sections"].as_array().unwrap().iter().find(|x| x["number"] == "09 2900").unwrap()["file"].as_str().unwrap().to_string();
        let mut sec = open(&proj, &file);
        let r = run(&mut sec, "spec.project.sync", json!({}));
        assert_eq!(r["furniture"], true, "{r}");
        let body = body_text(&sec.doc);
        assert!(body.contains("Work for Riverside Library"), "{body}");
        assert!(body.contains("Section 07 9200 \"Joint Sealants\""), "{body}");
        let footer = part_text(&sec.doc, sec.doc.last_section.footers.default);
        assert_eq!(footer, "LANL Project I.D. LANL-0042\tGypsum Board | Rev. 2, December 2026\t09 2900 - 1 of 1", "{footer}");
    }

    fn file_of(path: &Path, number: &str) -> String {
        let mut s = Session::new(Document::new());
        let info = run(&mut s, "spec.project.info", json!({"path": path.to_string_lossy()}));
        info["sections"].as_array().unwrap().iter().find(|x| x["number"] == number).unwrap()["file"].as_str().unwrap().to_string()
    }

    #[test]
    fn print_options_shape_the_issued_copy() {
        let (path, proj) = project("options");
        let mut sec = open(&proj, &file_of(&path, "09 2900"));
        let plain = super::super::output::issued_copy_as(&sec, None, super::super::forms::Form::Full).unwrap();
        let text = body_text(&plain.copy);
        assert!(!text.contains("Retain for projects"), "notes don't print");
        assert!(text.contains("END OF SECTION 09 2900"));
        assert!(plain.copy.settings.watermark.is_none());
        let changes = json!({"draft": true, "endOfSection": "plain", "showAllChoices": true, "bookmarks": "parts"});
        // Choose one option so the review copy underlines it.
        let list = run(&mut sec, "spec.fields.list", json!({}));
        let choice = list["fields"].as_array().unwrap().iter().find(|f| f["kind"] == "choice").unwrap()["id"].as_str().unwrap().to_string();
        run(&mut sec, "spec.fields.set", json!({"id": choice, "option": 1}));
        let d = super::super::output::issued_copy_with(&sec, None, super::super::forms::Form::Full, Some(&changes)).unwrap().copy;
        let text = body_text(&d);
        assert!(text.contains("Retain for projects"), "a draft has the notes: {text}");
        assert!(text.contains("[primed] [painted] finish"), "{text}");
        assert!(text.contains("END OF SECTION\n") || text.trim_end().ends_with("END OF SECTION"), "{text}");
        assert_eq!(d.settings.watermark.as_ref().map(|w| w.text.as_str()), Some("DRAFT"));
        let entries = sections::outline(&d);
        let title = entries.iter().find(|e| e.kind == Kind::Title).unwrap();
        let tp = d.para_at(&title.range.start).unwrap();
        assert_eq!((tp.props.outline_level, tp.props.keep_next), (Some(0), Some(true)));
        let part = entries.iter().find(|e| e.kind == Kind::Part).unwrap();
        assert_eq!(d.para_at(&part.range.start).unwrap().props.outline_level, Some(1));
        // The chosen option is underlined.
        let choice = d.body.iter().filter_map(|b| b.as_para()).find(|p| p.plain_text().contains("[primed]")).unwrap();
        // (props_at reads the character before an offset.)
        let at = choice.text.find("painted").unwrap() + 1;
        assert_eq!(choice.props_at(at).underline, Some(Underline::Single));
        assert_ne!(choice.props_at(choice.text.find("primed").unwrap() + 1).underline, Some(Underline::Single));
        assert!(s_err(&sec, json!({"bookmarks": "every"})));
    }

    fn s_err(sec: &Session, changes: Value) -> bool {
        super::super::output::issued_copy_with(sec, None, super::super::forms::Form::Full, Some(&changes)).is_err()
    }

    #[test]
    fn publishing_names_files_and_the_manual_starts_sections_on_odd_pages() {
        let (path, proj) = project("publish");
        let p = path.to_string_lossy().to_string();
        let mut s = Session::new(Document::new());
        run(&mut s, "spec.project.settings", json!({"path": p, "print": {"fileNames": "numberTitle"}}));
        let r = run(&mut s, "spec.project.publish", json!({"path": p, "force": true}));
        let files: Vec<String> =
            r["sections"].as_array().unwrap().iter().map(|x| x["pdf"].as_str().unwrap().rsplit(['/', '\\']).next().unwrap().to_string()).collect();
        assert_eq!(files, ["07 9200 Joint Sealants.pdf", "09 2900 Gypsum Board.pdf"]);
        // Only division 09, as a draft.
        let r =
            run(&mut s, "spec.project.publish", json!({"path": p, "force": true, "division": "09", "print": {"draft": true, "fileNames": "number"}}));
        let files: Vec<String> =
            r["sections"].as_array().unwrap().iter().map(|x| x["pdf"].as_str().unwrap().rsplit(['/', '\\']).next().unwrap().to_string()).collect();
        assert_eq!(files, ["09 2900 (Draft).pdf"]);
        // The manual: 07 9200 runs to an odd number of pages, so a blank page follows it.
        let plain = run(&mut s, "spec.project.manual", json!({"path": p, "force": true, "contents": false}));
        let odd =
            run(&mut s, "spec.project.manual", json!({"path": p, "force": true, "contents": false, "name": "odd", "print": {"oddPages": true}}));
        let first = odd["sections"][0]["pages"].as_u64().unwrap();
        let (a, b) = (plain["pages"].as_u64().unwrap(), odd["pages"].as_u64().unwrap());
        assert_eq!(b, a + first % 2, "{plain} {odd}");
        let bytes = std::fs::read(proj.join("Published").join("odd.pdf")).unwrap();
        assert!(bytes.len() > 1000);
        // The table of contents alone.
        let toc = run(&mut s, "spec.project.manual", json!({"path": p, "force": true, "contentsOnly": true}));
        assert_eq!(toc["pages"], 1, "{toc}");
        assert!(toc["files"]["pdf"].as_str().unwrap().contains("Table of Contents"));
    }

    #[test]
    fn the_standards_report_lists_what_sections_cite() {
        let (path, proj) = project("standards");
        let file = file_of(&path, "09 2900");
        let mut sec = open(&proj, &file);
        run(&mut sec, "spec.addParagraph", json!({"after": "p0006", "text": "Panels: ASTM C 1396/C 1396M; fire-rated to UL 263."}));
        crate::io::save_path(&proj.join(&file), &sec.doc).unwrap();
        let p = path.to_string_lossy().to_string();
        let mut s = Session::new(Document::new());
        run(
            &mut s,
            "spec.project.setStandard",
            json!({"path": p, "key": "ASTM C1396/C1396M", "title": "Standard Specification for Gypsum Board", "edition": "2017"}),
        );
        let r = run(&mut s, "spec.project.standards", json!({"path": p}));
        assert_eq!(r["count"], 2, "{r}");
        let first = &r["standards"][0];
        assert_eq!(
            (first["key"].clone(), first["edition"].clone(), first["sections"].clone()),
            (json!("ASTM C1396/C1396M"), json!("2017"), json!(["09 2900"]))
        );
        // Clearing both forgets it.
        run(&mut s, "spec.project.setStandard", json!({"path": p, "key": "ASTM C1396/C1396M", "title": "", "edition": ""}));
        assert!(project::load(&path).unwrap().standards.is_empty());
    }

    #[test]
    fn titles_read_as_titles() {
        assert_eq!(title_case("WORK BY OWNER-SELF PERFORM"), "Work by Owner-Self Perform");
        assert_eq!(title_case("DOORS AND FRAMES"), "Doors and Frames");
        assert_eq!(title_case("Joint Sealants"), "Joint Sealants");
    }
}
