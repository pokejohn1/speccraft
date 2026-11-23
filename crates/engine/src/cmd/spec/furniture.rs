//! The project's header and footer file (W10) and its keywords (W11). Added by the Speccraft
//! fork; not part of the original WordCraft.
//!
//! Each project has a Word file beside its project file, "Riverside Library - Header and
//! Footer.docx". Its header and footer are the project's: every section copies them, with their
//! fonts, sizes, tables, lines and pictures, when it is brought up to date with the project
//! (`spec.project.sync`, switching to a section's tab, publishing) or all at once
//! (`spec.project.syncAll`). The file can be edited in Speccraft or Word and sent to consultants
//! so their sections match.
//!
//! Keywords in braces are filled in for each section: `{Section ID}`, `{Section Name}`,
//! `{Division ID}`, `{Division Name}`, `{Page}`, `{Pages}` (pages of the section), the project's
//! `{Project Name}`, `{Project ID}`, `{Project Location}`, `{Issue}`, `{Date}`, and any global term
//! by name (`{Architect Name}`).
//!
//! The Project Information page edits each band as lines of three cells (left, centre, right)
//! with one font and size: `spec.project.furniture` reads them from the file and
//! `spec.project.setFurniture` writes them back. A band made in Word that doesn't fit that shape
//! (tables, pictures, mixed formatting) is reported as `custom`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use wordcraft_doc::para::{InlineObject, OBJ};
use wordcraft_doc::props::{TabAlign, TabLeader, TabStop};
use wordcraft_doc::section::HeaderSet;
use wordcraft_doc::{Block, CharProps, Document, Paragraph, PartKind, StyleSheet};

use super::project::{self, ProjectFile};
use crate::{CmdError, CmdResult, CommandSpec, Session, p};

const LOC: &str = "Specs › Project";
/// Most lines in a band edited as cells.
const MAX_LINES: usize = 6;
/// Longest cell, in characters.
const MAX_CELL: usize = 300;
/// Deepest table nesting walked in a header or footer.
const MAX_DEPTH: usize = 8;
/// Largest header and footer file read, in bytes.
const MAX_FILE: usize = 64 << 20;

/// The keywords, with what each one gives.
pub const KEYWORDS: [(&str, &str); 12] = [
    ("Section ID", "the section's number, e.g. 09 2900"),
    ("Section Name", "the section's title"),
    ("Division ID", "the division's number, e.g. 09"),
    ("Division Name", "the division's name, e.g. Finishes"),
    ("Page", "the page number"),
    ("Pages", "the number of pages in the section"),
    ("Project Name", "from Project Information"),
    ("Project ID", "from Project Information"),
    ("Project Location", "from Project Information"),
    ("Issue", "from Project Information"),
    ("Date", "from Project Information"),
    ("Today", "the date the file is printed"),
];

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("spec.project.furniture", "Project Header and Footer", LOC, get_cmd)
            .params(r#"{"path": string (the project's .speccraft file)}"#)
            .pure(),
        CommandSpec::new("spec.project.setFurniture", "Set Project Header and Footer", LOC, set_cmd).params(
            r#"{"path": string, "header"?: band, "footer"?: band} where band = {"lines": [{"left": string, "center": string, "right": string}], "font"?: string, "size"?: number, "bold"?: bool}"#,
        ).pure(),
        CommandSpec::new("spec.project.createFurniture", "Create Header and Footer File", LOC, create_cmd)
            .params(r#"{"path": string, "replace"?: bool (write a new file over the one there)}"#)
            .pure(),
        CommandSpec::new("spec.project.syncAll", "Update Every Section from the Project", LOC, sync_all_cmd)
            .params(r#"{"path": string, "skip"?: [file] (sections open with changes: brought up to date in their tabs instead)}"#)
            .pure(),
    ]
}

// ---------------------------------------------------------------------------------------------
// Keywords

/// A keyword's value: text, or a page field.
pub enum Piece {
    Text(String),
    Field(&'static str),
}

/// What `{name}` stands for in section `number` / `title` of a project with `values` (the
/// project's placeholder values, by lowercased name). `None` for a name it doesn't know.
pub fn keyword(name: &str, number: &str, title: &str, values: &BTreeMap<String, String>) -> Option<Piece> {
    let low = name.trim().to_lowercase();
    let division = || number.chars().filter(char::is_ascii_digit).take(2).collect::<String>();
    Some(match low.as_str() {
        "page" | "page number" => Piece::Field("PAGE"),
        "pages" | "section pages" | "section page count" => Piece::Field("SECTIONPAGES"),
        "section id" | "section number" => Piece::Text(number.to_string()),
        "section name" | "section title" => Piece::Text(super::print::title_case(title)),
        "division id" | "division number" => Piece::Text(division()),
        "division name" => Piece::Text(super::library::division_name(&division()).to_string()),
        "today" => Piece::Field("DATE"),
        // Project Information left empty prints nothing rather than the keyword.
        "project name" | "project id" | "project number" | "project location" | "issue" | "issue date" | "date" => {
            Piece::Text(values.get(&low).cloned().unwrap_or_default())
        }
        _ => Piece::Text(values.get(&low)?.clone()),
    })
}

/// `text`'s `{…}` keywords: (byte range with the braces, the name).
fn keyword_spans(text: &str) -> Vec<(std::ops::Range<usize>, String)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(a) = text.get(from..).and_then(|t| t.find('{')).map(|i| i + from) {
        let Some(b) = text.get(a..).and_then(|t| t.find('}')).map(|i| i + a) else { break };
        let inner = text.get(a + 1..b).unwrap_or("");
        if inner.contains(['{', OBJ]) || inner.chars().count() > 80 {
            from = a + 1;
            continue;
        }
        out.push((a..b + 1, inner.to_string()));
        from = b + 1;
    }
    out
}

/// Fill the keywords of `p`, each taking the formatting of its first character.
fn fill_keywords(p: &mut Paragraph, number: &str, title: &str, values: &BTreeMap<String, String>) {
    for (range, name) in keyword_spans(&p.text).into_iter().rev() {
        let Some(piece) = keyword(&name, number, title, values) else { continue };
        let props = p.props_at(range.start + 1).clone();
        if p.delete(range.start, range.end).is_err() {
            continue;
        }
        match piece {
            Piece::Text(t) => {
                let _ = p.insert_text(range.start, &t, &props);
            }
            Piece::Field(instr) => {
                let obj =
                    InlineObject::Field { instr: instr.into(), result: "1".into(), locked: false, dirty: true, code: false, form: String::new() };
                let _ = p.insert_object(range.start, obj, &props);
            }
        }
    }
    p.touch();
}

// ---------------------------------------------------------------------------------------------
// The file

/// The header and footer file's name for project `name`.
pub fn file_name(name: &str) -> String {
    let clean: String = name.chars().filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') && !c.is_control()).collect();
    let clean = clean.trim();
    format!("{} - Header and Footer.docx", if clean.is_empty() { "Project" } else { clean })
}

/// The header and footer file of `f`, when it has one.
pub fn path_of(f: &ProjectFile) -> Option<PathBuf> {
    let name = f.furniture_file.as_deref()?;
    if name.contains(['/', '\\']) || name.starts_with('.') {
        return None;
    }
    Some(f.dir.clone().unwrap_or_default().join(name))
}

/// One line of a band: what goes on the left, in the middle and on the right.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Line {
    pub left: String,
    pub center: String,
    pub right: String,
}

/// A header or footer as lines of three cells, in one font.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Band {
    pub lines: Vec<Line>,
    pub font: Option<String>,
    pub size: Option<f32>,
    pub bold: bool,
}

impl Band {
    fn to_json(&self, custom: bool) -> Value {
        let lines: Vec<Value> = self.lines.iter().map(|l| json!({"left": l.left, "center": l.center, "right": l.right})).collect();
        json!({"lines": lines, "font": self.font, "size": self.size, "bold": self.bold, "custom": custom})
    }

    fn from_json(v: &Value) -> Result<Band, CmdError> {
        let bad =
            || CmdError::Params("a header or footer is {\"lines\": [{\"left\", \"center\", \"right\"}], \"font\"?, \"size\"?, \"bold\"?}".into());
        let lines = v.get("lines").and_then(Value::as_array).ok_or_else(bad)?;
        if lines.len() > MAX_LINES {
            return Err(CmdError::Params(format!("at most {MAX_LINES} lines")));
        }
        let cell = |l: &Value, k: &str| -> Result<String, CmdError> {
            match l.get(k) {
                None | Some(Value::Null) => Ok(String::new()),
                Some(Value::String(s)) if s.chars().count() <= MAX_CELL && !s.contains(['\t', '\n', '\r', OBJ]) => Ok(s.trim().to_string()),
                _ => Err(CmdError::Params(format!("`{k}` must be one line of at most {MAX_CELL} characters"))),
            }
        };
        let mut out = Band::default();
        for l in lines {
            out.lines.push(Line { left: cell(l, "left")?, center: cell(l, "center")?, right: cell(l, "right")? });
        }
        out.font = match v.get("font") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) if !s.trim().is_empty() && s.chars().count() <= 100 => Some(s.trim().to_string()),
            _ => return Err(CmdError::Params("`font` must be a font name".into())),
        };
        out.size = match v.get("size") {
            None | Some(Value::Null) => None,
            Some(x) => {
                Some(x.as_f64().filter(|n| (4.0..=72.0).contains(n)).ok_or_else(|| CmdError::Params("`size` must be 4 to 72 points".into()))? as f32)
            }
        };
        out.bold = v.get("bold").and_then(Value::as_bool).unwrap_or(false);
        Ok(out)
    }
}

/// The text width of `doc`'s last section, points.
fn width_of(doc: &Document) -> f32 {
    let s = &doc.last_section;
    (s.page_w - s.margin_left - s.margin_right).max(72.0)
}

/// A band's paragraphs: `left` tab `center` tab `right`, with a centre stop in the middle and a
/// right stop at the margin, in the band's font.
fn band_blocks(band: &Band, width: f32, style: &str) -> Vec<Arc<Block>> {
    let chr = CharProps { font: band.font.clone(), size: band.size, bold: band.bold.then_some(true), ..Default::default() };
    let lines = if band.lines.is_empty() { vec![Line::default()] } else { band.lines.clone() };
    lines
        .iter()
        .map(|l| {
            let text = format!("{}\t{}\t{}", l.left, l.center, l.right);
            let mut p = Paragraph::with_text(&text, chr.clone()).styled(style);
            p.mark = chr.clone();
            p.props.tabs = Some(vec![
                TabStop { pos: width / 2.0, align: TabAlign::Center, leader: TabLeader::None },
                TabStop { pos: width, align: TabAlign::Right, leader: TabLeader::None },
            ]);
            p.touch();
            Arc::new(Block::Para(p))
        })
        .collect()
}

/// Point every section of `doc` (and the last) at header or footer part `id`.
fn point_all(doc: &mut Document, kind: PartKind, set: HeaderSet) {
    let apply = |s: &mut wordcraft_doc::SectionProps| {
        if kind == PartKind::Header {
            s.headers = set;
        } else {
            s.footers = set;
        }
    };
    for b in doc.body.iter_mut() {
        if b.as_para().is_some_and(|p| p.section.is_some())
            && let Block::Para(p) = Arc::make_mut(b)
            && let Some(s) = p.section.as_deref_mut()
        {
            apply(s);
        }
    }
    apply(&mut doc.last_section);
}

/// The text a template shows in its body: what the file is and its keywords. Not copied.
fn explain(doc: &mut Document, f: &ProjectFile) {
    let para = |text: &str, style: &str| Arc::new(Block::Para(Paragraph::with_text(text, CharProps::default()).styled(style)));
    let name = if f.project.name.is_empty() { "this project".to_string() } else { f.project.name.clone() };
    let mut body = vec![
        para(&format!("Header and footer for {name}"), "Heading1"),
        para(
            "Speccraft copies the header and footer of this file into every section of the project. Edit them here (double-click the top or bottom of the page) or on the Project Information page in Speccraft, and send this file to consultants so their sections match.",
            "Normal",
        ),
        para("Words in braces are filled in for each section:", "Normal"),
    ];
    for (k, what) in KEYWORDS {
        body.push(para(&format!("{{{k}}}  {what}"), "Normal"));
    }
    body.push(para(
        "Any global term from Project Information can be used the same way, e.g. {Architect Name}. Text on this page is not copied.",
        "Normal",
    ));
    doc.body = body;
}

/// A new header and footer file for `f`: its M21 lines if it has them, otherwise a header with
/// the project's name and number and a footer with the section's name, number and page.
fn template(f: &ProjectFile) -> Document {
    let mut doc = Document::new();
    explain(&mut doc, f);
    let lines_of = |furn: &Option<super::print::Furniture>| -> Option<Band> {
        let furn = furn.as_ref()?;
        let n = furn.left.len().max(furn.right.len());
        Some(Band {
            lines: (0..n)
                .map(|i| Line {
                    left: furn.left.get(i).cloned().unwrap_or_default(),
                    center: String::new(),
                    right: furn.right.get(i).cloned().unwrap_or_default(),
                })
                .collect(),
            font: Some("Arial".into()),
            size: Some(9.0),
            bold: false,
        })
    };
    let header = lines_of(&f.header).unwrap_or_else(|| Band {
        lines: vec![Line { left: "{Project Name}".into(), center: String::new(), right: "{Project ID}".into() }],
        font: Some("Arial".into()),
        size: Some(9.0),
        bold: false,
    });
    let footer = lines_of(&f.footer).unwrap_or_else(|| Band {
        lines: vec![Line { left: "{Section Name}".into(), center: String::new(), right: "{Section ID} - {Page}".into() }],
        font: Some("Arial".into()),
        size: Some(9.0),
        bold: false,
    });
    put_bands(&mut doc, Some(&header), Some(&footer));
    doc
}

/// Replace `doc`'s header and/or footer with bands.
fn put_bands(doc: &mut Document, header: Option<&Band>, footer: Option<&Band>) {
    let width = width_of(doc);
    for (band, kind, style) in [(header, PartKind::Header, "Header"), (footer, PartKind::Footer, "Footer")] {
        let Some(band) = band else { continue };
        let id = doc.add_part(kind, band_blocks(band, width, style));
        point_all(doc, kind, HeaderSet { default: Some(id), first: None, even: None });
    }
    drop_unused_parts(doc);
}

/// Remove header and footer parts no section points at any more.
fn drop_unused_parts(doc: &mut Document) {
    let mut used = std::collections::HashSet::new();
    let mut note = |s: &wordcraft_doc::SectionProps| {
        for set in [&s.headers, &s.footers] {
            used.extend([set.default, set.first, set.even].into_iter().flatten());
        }
    };
    for b in &doc.body {
        if let Some(s) = b.as_para().and_then(|p| p.section.as_deref()) {
            note(s);
        }
    }
    note(&doc.last_section);
    doc.parts.retain(|id, part| !matches!(part.kind, PartKind::Header | PartKind::Footer) || used.contains(id));
}

/// Make `f`'s header and footer file in the project folder of `project` and record it. An
/// existing file of that name is kept and linked, unless `replace`.
pub fn create(project: &Path, f: &mut ProjectFile, replace: bool) -> Result<String, CmdError> {
    let name = file_name(&f.project.name);
    let path = project.parent().map(Path::to_path_buf).unwrap_or_default().join(&name);
    if replace || !path.is_file() {
        crate::io::save_path(&path, &template(f)).map_err(CmdError::Failed)?;
    }
    f.furniture_file = Some(name.clone());
    Ok(name)
}

/// M21 header or footer lines (left and right) written into `f`'s header and footer file, when it
/// has one.
pub fn write_lines(f: &ProjectFile, header: Option<&super::print::Furniture>, footer: Option<&super::print::Furniture>) -> Result<(), CmdError> {
    let Some(path) = path_of(f).filter(|p| p.is_file()) else { return Ok(()) };
    let band = |furn: &super::print::Furniture| {
        let n = furn.left.len().max(furn.right.len());
        Band {
            lines: (0..n)
                .map(|i| Line {
                    left: furn.left.get(i).cloned().unwrap_or_default(),
                    center: String::new(),
                    right: furn.right.get(i).cloned().unwrap_or_default(),
                })
                .collect(),
            ..Default::default()
        }
    };
    let (mut doc, _) = open(&path)?;
    // Keep the file's font and size.
    let keep = |mut b: Band, kind: PartKind| {
        let (old, _) = read_band(&doc, kind);
        b.font = old.font;
        b.size = old.size;
        b.bold = old.bold;
        b
    };
    let h = header.map(|x| keep(band(x), PartKind::Header));
    let ft = footer.map(|x| keep(band(x), PartKind::Footer));
    put_bands(&mut doc, h.as_ref(), ft.as_ref());
    crate::io::save_path(&path, &doc).map_err(CmdError::Failed)
}

/// Read the header and footer file.
fn open(path: &Path) -> Result<(Document, Vec<u8>), CmdError> {
    let bytes = crate::io::read_document(path).map_err(CmdError::Failed)?;
    if bytes.len() > MAX_FILE {
        return Err(CmdError::Failed(format!("{}: larger than 64 MB", path.display())));
    }
    let doc = crate::io::open_bytes(&path.to_string_lossy(), &bytes).map_err(CmdError::Failed)?;
    Ok((doc, bytes))
}

/// A band read back from `doc`'s default header or footer: `(band, custom)`, `custom` when it
/// holds more than lines of up to three cells in one font (tables, pictures, other fields, mixed
/// formatting), which saving from the cells would lose.
fn read_band(doc: &Document, kind: PartKind) -> (Band, bool) {
    let set = if kind == PartKind::Header { doc.last_section.headers } else { doc.last_section.footers };
    let Some(part) = set.default.and_then(|id| doc.parts.get(&id)) else { return (Band::default(), false) };
    let mut band = Band::default();
    let mut custom = set.first.is_some() || set.even.is_some();
    let mut formats: Vec<(Option<String>, Option<f32>, bool)> = Vec::new();
    for b in &part.blocks {
        let Block::Para(p) = &**b else {
            custom = true;
            continue;
        };
        let (_, sc) = doc.styles.para_style_props(p.props.style.as_deref().unwrap_or("Normal"));
        for r in &p.runs {
            let mut c = sc.clone();
            c.overlay(&r.props);
            let f = (c.font.clone(), c.size, c.bold.unwrap_or(false));
            if !formats.contains(&f) {
                formats.push(f);
            }
        }
        let mut text = String::new();
        let mut objects = p.objects.iter();
        for ch in p.text.chars() {
            if ch == OBJ {
                match objects.next() {
                    Some(InlineObject::Field { instr, .. }) => match instr.split_whitespace().next().map(str::to_uppercase).as_deref() {
                        Some("PAGE") => text.push_str("{Page}"),
                        Some("SECTIONPAGES" | "NUMPAGES") => text.push_str("{Pages}"),
                        Some("DATE") => text.push_str("{Today}"),
                        _ => custom = true,
                    },
                    _ => custom = true,
                }
            } else {
                text.push(ch);
            }
        }
        let cells: Vec<&str> = text.split('\t').collect();
        let line = match cells.as_slice() {
            [l] => Line { left: (*l).into(), ..Default::default() },
            [l, r] => Line { left: (*l).into(), center: String::new(), right: (*r).into() },
            [l, c, r] => Line { left: (*l).into(), center: (*c).into(), right: (*r).into() },
            _ => {
                custom = true;
                Line { left: text.replace('\t', " "), ..Default::default() }
            }
        };
        band.lines.push(line);
    }
    // A last empty line (Word's paragraph mark) is not a line.
    while band.lines.len() > 1 && band.lines.last().is_some_and(|l| *l == Line::default()) {
        band.lines.pop();
    }
    if formats.len() > 1 || band.lines.len() > MAX_LINES {
        custom = true;
    }
    if let Some((font, size, bold)) = formats.into_iter().next() {
        band.font = font;
        band.size = size;
        band.bold = bold;
    }
    (band, custom)
}

fn project_param(v: &Value) -> Result<(PathBuf, ProjectFile), CmdError> {
    let path = PathBuf::from(p::req_str(v, "path")?);
    let f = project::load(&path)?;
    Ok((path, f))
}

/// The keyword and global term lists for the menus, with their values where the project has them.
fn menus(f: &ProjectFile) -> Value {
    let values = project::placeholder_values(f);
    let keywords: Vec<Value> = KEYWORDS.iter().map(|(k, what)| json!({"name": k, "about": what, "value": values.get(&k.to_lowercase())})).collect();
    let terms: Vec<Value> = f.values.iter().map(|(k, v)| json!({"name": k, "value": v})).collect();
    json!({"keywords": keywords, "terms": terms})
}

fn get_cmd(_: &mut Session, v: &Value) -> CmdResult {
    let (_, f) = project_param(v)?;
    let mut out = menus(&f);
    out["file"] = json!(f.furniture_file);
    let Some(path) = path_of(&f).filter(|p| p.is_file()) else {
        out["exists"] = json!(false);
        return Ok(out);
    };
    let (doc, _) = open(&path)?;
    let (h, hc) = read_band(&doc, PartKind::Header);
    let (ft, fc) = read_band(&doc, PartKind::Footer);
    out["exists"] = json!(true);
    out["filePath"] = json!(path.to_string_lossy());
    out["header"] = h.to_json(hc);
    out["footer"] = ft.to_json(fc);
    Ok(out)
}

fn set_cmd(_: &mut Session, v: &Value) -> CmdResult {
    let (project_path, mut f) = project_param(v)?;
    let header = v.get("header").filter(|x| !x.is_null()).map(Band::from_json).transpose()?;
    let footer = v.get("footer").filter(|x| !x.is_null()).map(Band::from_json).transpose()?;
    let path = match path_of(&f).filter(|p| p.is_file()) {
        Some(p) => p,
        None => {
            create(&project_path, &mut f, false)?;
            project::store_file(&project_path, &f)?;
            path_of(&f).ok_or_else(|| CmdError::Failed("no header and footer file".into()))?
        }
    };
    let (mut doc, _) = open(&path)?;
    put_bands(&mut doc, header.as_ref(), footer.as_ref());
    crate::io::save_path(&path, &doc).map_err(CmdError::Failed)?;
    get_cmd(&mut Session::new(Document::new()), &json!({"path": project_path.to_string_lossy()}))
}

fn create_cmd(_: &mut Session, v: &Value) -> CmdResult {
    let (path, mut f) = project_param(v)?;
    let name = create(&path, &mut f, p::bool(v, "replace").unwrap_or(false))?;
    project::store_file(&path, &f)?;
    Ok(json!({"file": name}))
}

/// Bring every section file of the project up to date with it (W10): open each, sync it, save
/// it when anything changed. `skip`: sections not to touch on disk.
fn sync_all_cmd(_: &mut Session, v: &Value) -> CmdResult {
    let (_, f) = project_param(v)?;
    let skip: Vec<String> =
        v.get("skip").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
    let dir = f.dir.clone().unwrap_or_default();
    let (mut updated, mut same, mut failed) = (Vec::new(), 0usize, Vec::new());
    for sec in &f.sections {
        if skip.contains(&sec.file) || sec.file.contains(['/', '\\']) {
            continue;
        }
        let path = dir.join(&sec.file);
        if !path.is_file() {
            continue;
        }
        let doc = match crate::io::open_path(&path) {
            Ok(d) => d,
            Err(e) => {
                failed.push(json!({"file": sec.file, "why": e}));
                continue;
            }
        };
        let mut s = Session::new(doc.clone());
        s.path = Some(path.clone());
        if let Err(e) = super::print::sync(&mut s, &f, Some(&sec.file)) {
            failed.push(json!({"file": sec.file, "why": e.to_string()}));
            continue;
        }
        if s.doc == doc {
            same += 1;
            continue;
        }
        match crate::io::save_path(&path, &s.doc) {
            Ok(()) => updated.push(sec.file.clone()),
            Err(e) => failed.push(json!({"file": sec.file, "why": e})),
        }
    }
    Ok(json!({"updated": updated, "unchanged": same, "failed": failed}))
}

// ---------------------------------------------------------------------------------------------
// Copying into a section

/// Paragraph `p` of `src` with its style's formatting made direct, so it looks the same in a
/// document whose styles differ. Lists are dropped (they belong to `src`).
fn flatten(p: &mut Paragraph, src: &StyleSheet) {
    let style = p.props.style.clone().unwrap_or_else(|| "Normal".into());
    let (mut pp, cp) = src.para_style_props(&style);
    let tabs = match (pp.tabs.take(), p.props.tabs.clone()) {
        (Some(base), Some(add)) => {
            let mut merged: Vec<TabStop> = base.into_iter().filter(|t| !add.iter().any(|a| (a.pos - t.pos).abs() < 0.5)).collect();
            merged.extend(add);
            Some(merged)
        }
        (a, b) => b.or(a),
    };
    pp.overlay(&p.props);
    pp.tabs = tabs.map(|t| t.into_iter().filter(|s| s.align != TabAlign::Clear).collect());
    pp.style = None;
    pp.numbering = None;
    p.props = pp;
    let direct = |c: &CharProps| {
        let mut out = cp.clone();
        if let Some(st) = c.style.as_deref().and_then(|id| src.get(id)) {
            out.overlay(&st.chr);
        }
        out.overlay(c);
        out.style = None;
        out
    };
    for r in &mut p.runs {
        r.props = direct(&r.props);
    }
    p.mark = direct(&p.mark);
    p.touch();
}

/// Every paragraph in `b`, tables included.
fn each_para(b: &mut Block, depth: usize, f: &mut dyn FnMut(&mut Paragraph)) {
    if depth > MAX_DEPTH {
        return;
    }
    match b {
        Block::Para(p) => f(p),
        Block::Table(t) => {
            for row in &mut t.rows {
                for cell in &mut row.cells {
                    for cb in &mut cell.blocks {
                        each_para(Arc::make_mut(cb), depth + 1, f);
                    }
                }
            }
        }
    }
}

/// A copy of part `id` of `tpl` ready for `doc`: formatting direct, tab stops at the template's
/// centre and margin moved to `doc`'s, keywords filled, pictures brought along.
fn copy_part(doc: &mut Document, tpl: &Document, id: u32, number: &str, title: &str, values: &BTreeMap<String, String>) -> Option<Vec<Arc<Block>>> {
    let part = tpl.parts.get(&id)?;
    let (from_w, to_w) = (width_of(tpl), width_of(doc));
    let mut blocks: Vec<Block> = part.blocks.iter().map(|b| (**b).clone()).collect();
    for b in &mut blocks {
        each_para(b, 0, &mut |p| {
            flatten(p, &tpl.styles);
            if let Some(tabs) = p.props.tabs.as_mut() {
                for t in tabs.iter_mut() {
                    if (t.pos - from_w).abs() < 1.0 {
                        t.pos = to_w;
                    } else if (t.pos - from_w / 2.0).abs() < 1.0 {
                        t.pos = to_w / 2.0;
                    }
                }
            }
            fill_keywords(p, number, title, values);
        });
    }
    // Pictures: the template's media the copy names, under new keys of ours.
    let mut text = serde_json::to_string(&blocks).ok()?;
    for (key, data) in &tpl.media {
        let quoted = serde_json::to_string(key).ok()?;
        if text.contains(&quoted) {
            let ext = key.rsplit_once('.').map_or("png", |(_, e)| e);
            let new = doc.add_media(data.to_vec(), ext);
            text = text.replace(&quoted, &serde_json::to_string(&new).ok()?);
        }
    }
    let blocks: Vec<Block> = serde_json::from_str(&text).ok()?;
    Some(blocks.into_iter().map(Arc::new).collect())
}

/// Replace `doc`'s headers and footers with the template's (W10): default, first page and even
/// pages as the template has them, its header and footer distances, keywords filled for section
/// `number` / `title`.
pub fn put_from_template(doc: &mut Document, tpl: &Document, number: &str, title: &str, values: &BTreeMap<String, String>) {
    let src = tpl.last_section.clone();
    for (kind, set) in [(PartKind::Header, src.headers), (PartKind::Footer, src.footers)] {
        let mut out = HeaderSet::default();
        for (slot, id) in [(&mut out.default, set.default), (&mut out.first, set.first), (&mut out.even, set.even)] {
            if let Some(blocks) = id.and_then(|id| copy_part(doc, tpl, id, number, title, values)) {
                *slot = Some(doc.add_part(kind, blocks));
            }
        }
        point_all(doc, kind, out);
    }
    let apply = |s: &mut wordcraft_doc::SectionProps| {
        s.header = src.header;
        s.footer = src.footer;
        s.title_page = src.title_page && src.headers.first.or(src.footers.first).is_some();
    };
    for b in doc.body.iter_mut() {
        if b.as_para().is_some_and(|p| p.section.is_some())
            && let Block::Para(p) = Arc::make_mut(b)
            && let Some(s) = p.section.as_deref_mut()
        {
            apply(s);
        }
    }
    apply(&mut doc.last_section);
    drop_unused_parts(doc);
}

/// The template for a sync: the document and a fingerprint of its bytes, or `None` when the
/// project has no header and footer file (or it is missing).
pub fn template_for(f: &ProjectFile) -> Result<Option<(Document, String)>, CmdError> {
    let Some(path) = path_of(f).filter(|p| p.is_file()) else { return Ok(None) };
    let (doc, bytes) = open(&path)?;
    Ok(Some((doc, project::fingerprint(&bytes))))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("speccraft-furniture-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn para(style: &str, text: &str) -> Arc<Block> {
        Arc::new(Block::Para(Paragraph::with_text(text, CharProps::default()).styled(style)))
    }

    fn section(number: &str, title: &str) -> Document {
        let mut d = Document::new();
        d.body = vec![para("SCT", &format!("SECTION {number} - {title}")), para("PRT", "GENERAL"), para("EOS", &format!("END OF SECTION {number}"))];
        d
    }

    fn save_section(path: &Path, number: &str, title: &str) {
        let mut s = Session::new(section(number, title));
        run(&mut s, "spec.import", json!({}));
        crate::io::save_path(path, &s.doc).unwrap();
    }

    fn run(s: &mut Session, cmd: &str, v: Value) -> Value {
        s.run(cmd, &v).unwrap_or_else(|e| panic!("{cmd}: {e}"))
    }

    fn band_text(doc: &Document, kind: PartKind) -> Vec<String> {
        let set = if kind == PartKind::Header { doc.last_section.headers } else { doc.last_section.footers };
        let part = doc.parts.get(&set.default.unwrap()).unwrap();
        part.blocks.iter().filter_map(|b| b.as_para()).map(|p| p.text.clone()).collect()
    }

    #[test]
    fn a_new_project_gets_a_header_and_footer_file_every_section_copies() {
        let dir = scratch("new");
        let project = dir.join("Riverside.speccraft");
        let mut s = Session::new(Document::new());
        let info = run(
            &mut s,
            "spec.project.new",
            json!({"path": project.to_string_lossy(), "name": "Riverside Library", "number": "2026-14", "location": "Riverside, CA"}),
        );
        assert_eq!(info["furnitureFile"], "Riverside Library - Header and Footer.docx");
        assert_eq!(info["furnitureExists"], true);
        assert_eq!(info["location"], "Riverside, CA");

        // The cells read back from the file.
        let got = run(&mut s, "spec.project.furniture", json!({"path": project.to_string_lossy()}));
        assert_eq!(got["header"]["lines"], json!([{"left": "{Project Name}", "center": "", "right": "{Project ID}"}]));
        assert_eq!(got["footer"]["custom"], false);
        assert!(got["keywords"].as_array().unwrap().iter().any(|k| k["name"] == "Division Name"));

        // Set them: three cells, a font and a size, a global term.
        run(&mut s, "spec.project.set", json!({"path": project.to_string_lossy(), "values": {"Architect Name": "Ruiz Partners"}}));
        let got = run(
            &mut s,
            "spec.project.setFurniture",
            json!({"path": project.to_string_lossy(),
                "header": {"lines": [{"left": "{Project Name}", "center": "{Architect Name}", "right": "{Project Location}"}], "font": "Arial", "size": 9},
                "footer": {"lines": [{"left": "{Division ID} - {Division Name}", "center": "{Section Name}", "right": "{Section ID} - {Page} of {Pages}"}], "font": "Arial", "size": 8, "bold": true}}),
        );
        assert_eq!(got["header"]["font"], "Arial");
        assert_eq!(got["footer"]["size"], 8.0);
        assert_eq!(got["footer"]["bold"], true);
        assert_eq!(got["terms"][0]["name"], "Architect Name");

        // A section copies them, keywords filled, in the file's font.
        let sec = dir.join("092900.docx");
        save_section(&sec, "09 2900", "GYPSUM BOARD");
        let mut t = Session::new(crate::io::open_path(&sec).unwrap());
        t.path = Some(sec.clone());
        let r = run(&mut t, "spec.project.sync", json!({}));
        assert_eq!(r["furniture"], true);
        assert_eq!(band_text(&t.doc, PartKind::Header), ["Riverside Library\tRuiz Partners\tRiverside, CA"]);
        let footer = band_text(&t.doc, PartKind::Footer);
        assert_eq!(footer[0].replace(OBJ, "#"), "09 - Finishes\tGypsum Board\t09 2900 - # of #");
        let set = t.doc.last_section.footers;
        let p = t.doc.parts.get(&set.default.unwrap()).unwrap().blocks[0].as_para().unwrap().clone();
        assert_eq!((p.runs[0].props.font.as_deref(), p.runs[0].props.size, p.runs[0].props.bold), (Some("Arial"), Some(8.0), Some(true)));
        // Brought up to date again, nothing changes; the parts don't pile up.
        let parts = t.doc.parts.len();
        assert_eq!(run(&mut t, "spec.project.sync", json!({}))["furniture"], false);
        assert_eq!(t.doc.parts.len(), parts);

        // A change to the file reaches every section on disk.
        crate::io::save_path(&sec, &t.doc).unwrap();
        let sec2 = dir.join("079200.docx");
        save_section(&sec2, "07 9200", "JOINT SEALANTS");
        run(&mut s, "spec.project.addSection", json!({"path": project.to_string_lossy(), "file": "092900.docx"}));
        run(&mut s, "spec.project.addSection", json!({"path": project.to_string_lossy(), "file": "079200.docx"}));
        run(
            &mut s,
            "spec.project.setFurniture",
            json!({"path": project.to_string_lossy(), "header": {"lines": [{"left": "{Project Name}", "right": "Issued for Bid"}]}}),
        );
        let r = run(&mut s, "spec.project.syncAll", json!({"path": project.to_string_lossy()}));
        assert_eq!(r["updated"], json!(["079200.docx", "092900.docx"]), "{r}");
        let d = crate::io::open_path(&sec2).unwrap();
        assert_eq!(band_text(&d, PartKind::Header), ["Riverside Library\t\tIssued for Bid"]);
        let footer = band_text(&d, PartKind::Footer);
        assert!(footer[0].starts_with("07 - Thermal and Moisture Protection\tJoint Sealants\t07 9200"), "{footer:?}");
        assert_eq!(run(&mut s, "spec.project.syncAll", json!({"path": project.to_string_lossy()}))["updated"], json!([]));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_header_made_in_word_keeps_its_formatting_and_is_reported_custom() {
        let mut tpl = Document::new();
        let mut a =
            Paragraph::with_text("Riverside ", CharProps { font: Some("Garamond".into()), size: Some(11.0), ..Default::default() }).styled("Header");
        let _ = a.insert_text(a.text.len(), "{Section ID}", &CharProps { italic: Some(true), ..Default::default() });
        let id = tpl.add_part(PartKind::Header, vec![Arc::new(Block::Para(a))]);
        tpl.last_section.headers.default = Some(id);
        let (band, custom) = read_band(&tpl, PartKind::Header);
        assert!(custom, "two formats");
        assert_eq!(band.lines[0].left, "Riverside {Section ID}");
        let mut doc = section("09 2900", "GYPSUM BOARD");
        put_from_template(&mut doc, &tpl, "09 2900", "GYPSUM BOARD", &BTreeMap::new());
        let p = band_text(&doc, PartKind::Header);
        assert_eq!(p, ["Riverside 09 2900"]);
        let part = doc.parts.get(&doc.last_section.headers.default.unwrap()).unwrap();
        let para = part.blocks[0].as_para().unwrap();
        assert_eq!(para.props_at(1).font.as_deref(), Some("Garamond"));
        assert_eq!(para.props_at(12).italic, Some(true), "the keyword keeps its own formatting");
        assert_eq!(para.props.style, None);
    }

    #[test]
    fn keywords_and_bad_cells() {
        let v: BTreeMap<String, String> = [("architect name".to_string(), "Ruiz".to_string())].into();
        let text = |n: &str| match keyword(n, "09 2900", "GYPSUM BOARD", &v) {
            Some(Piece::Text(t)) => t,
            Some(Piece::Field(f)) => format!("<{f}>"),
            None => "?".into(),
        };
        assert_eq!(
            [text("Division ID"), text("division name"), text("Pages"), text("Architect Name"), text("Owner")],
            ["09", "Finishes", "<SECTIONPAGES>", "Ruiz", "?"]
        );
        assert!(Band::from_json(&json!({"lines": [{"left": "a\tb"}]})).is_err());
        assert!(Band::from_json(&json!({"lines": [], "size": 200})).is_err());
        assert_eq!(file_name("A/B: C"), "AB C - Header and Footer.docx");
    }
}
