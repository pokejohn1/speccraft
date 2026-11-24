//! Rule lines (W12): a line above or below a paragraph, from margin to margin. Added by the
//! Speccraft fork; not part of the original WordCraft.
//!
//! A rule line is the paragraph's top or bottom border, so it always spans the text width,
//! never pushes text aside, and is an ordinary paragraph border in Word. Its weight is the
//! border's width and its gap the border's distance from the text.

use serde_json::{Value, json};
use wordcraft_doc::props::{Border, BorderStyle, Borders, Rgb};

use crate::{CmdError, CmdResult, CommandSpec, Session, p};

const LOC: &str = "Specs › Lines";
/// Weights and gaps allowed, points (Word's border limits).
const WEIGHT: (f32, f32) = (0.25, 12.0);
const GAP: (f32, f32) = (0.0, 31.0);
/// A new rule line's weight and gap.
pub const DEFAULT_WEIGHT: f32 = 1.0;
pub const DEFAULT_GAP: f32 = 1.0;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("spec.ruleLine", "Rule Line", LOC, rule_line).params(
            r#"{"line"?: "below"|"above"|"both"|"none" (default: as it is, or below), "weight"?: pt 0.25–12, "gap"?: pt 0–31 (distance from the text), "color"?: "RRGGBB"}: on every selected paragraph, in any story"#,
        ),
        CommandSpec::new("spec.ruleLineInfo", "Rule Line at the Caret", LOC, info).pure(),
    ]
}

/// Which sides a paragraph's rule lines are on, and their weight, gap and colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rule {
    pub above: bool,
    pub below: bool,
    pub weight: f32,
    pub gap: f32,
    pub color: Option<Rgb>,
}

impl Rule {
    pub fn of(b: &Borders) -> Rule {
        let vis = |x: &Option<Border>| x.filter(|b| b.is_visible());
        let (top, bottom) = (vis(&b.top), vis(&b.bottom));
        let any = bottom.or(top);
        Rule {
            above: top.is_some(),
            below: bottom.is_some(),
            weight: any.map_or(DEFAULT_WEIGHT, |b| b.width),
            gap: any.map_or(DEFAULT_GAP, |b| b.space),
            color: any.and_then(|b| b.color),
        }
    }

    pub fn line(&self) -> &'static str {
        match (self.above, self.below) {
            (true, true) => "both",
            (true, false) => "above",
            (false, true) => "below",
            (false, false) => "none",
        }
    }

    /// `b` with these rule lines (left and right borders and the rest kept).
    pub fn apply(&self, mut b: Borders) -> Borders {
        let border = Border { style: BorderStyle::Single, width: self.weight, color: self.color, space: self.gap };
        b.top = self.above.then_some(border);
        b.bottom = self.below.then_some(border);
        b
    }

    pub fn to_json(&self) -> Value {
        json!({"line": self.line(), "weight": self.weight, "gap": self.gap, "color": self.color.map(|c| c.hex())})
    }

    /// A rule changed by `v`'s `line`, `weight`, `gap` and `color`.
    pub fn with(mut self, v: &Value) -> Result<Rule, CmdError> {
        match p::str(v, "line") {
            None => {
                if !self.above && !self.below {
                    self.below = true;
                }
            }
            Some("below") => (self.above, self.below) = (false, true),
            Some("above") => (self.above, self.below) = (true, false),
            Some("both") => (self.above, self.below) = (true, true),
            Some("none") => (self.above, self.below) = (false, false),
            Some(x) => return Err(CmdError::Params(format!("`line`: below, above, both or none, not `{x}`"))),
        }
        let num = |k: &str, (lo, hi): (f32, f32)| -> Result<Option<f32>, CmdError> {
            match v.get(k) {
                None | Some(Value::Null) => Ok(None),
                Some(x) => x
                    .as_f64()
                    .map(|n| n as f32)
                    .filter(|n| n.is_finite() && (lo..=hi).contains(n))
                    .map(Some)
                    .ok_or_else(|| CmdError::Params(format!("`{k}` must be {lo} to {hi} pt"))),
            }
        };
        if let Some(w) = num("weight", WEIGHT)? {
            self.weight = w;
        }
        if let Some(g) = num("gap", GAP)? {
            self.gap = g;
        }
        if let Some(c) = v.get("color") {
            self.color = match c.as_str() {
                Some(h) => Some(Rgb::parse(h).ok_or_else(|| CmdError::Params("`color` must be RRGGBB".into()))?),
                None => None,
            };
        }
        self.weight = self.weight.clamp(WEIGHT.0, WEIGHT.1);
        self.gap = self.gap.clamp(GAP.0, GAP.1);
        Ok(self)
    }
}

/// The rule lines of the paragraph at the caret (its style's borders included).
fn at_caret(s: &Session) -> Rule {
    let props = s.doc.para(s.sel.focus.story, &s.sel.focus.path).map(|p| p.props.clone()).unwrap_or_default();
    Rule::of(&s.doc.styles.resolve_para(&props).borders.unwrap_or_default())
}

fn info(s: &mut Session, _: &Value) -> CmdResult {
    Ok(at_caret(s).to_json())
}

fn rule_line(s: &mut Session, v: &Value) -> CmdResult {
    let rule = at_caret(s).with(v)?;
    let story = s.sel.focus.story;
    for path in crate::cmd::selected_paths(s) {
        let resolved = match s.doc.para(story, &path) {
            Some(p) => s.doc.styles.resolve_para(&p.props).borders.unwrap_or_default(),
            None => continue,
        };
        let para = s.doc.para_mut(story, &path)?;
        para.props.borders = Some(rule.apply(resolved));
        para.touch();
    }
    Ok(rule.to_json())
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wordcraft_doc::{CharProps, Document, Paragraph, para_block};

    use crate::Session;

    fn run(s: &mut Session, cmd: &str, v: serde_json::Value) -> serde_json::Value {
        s.run(cmd, &v).unwrap_or_else(|e| panic!("{cmd}: {e}"))
    }

    #[test]
    fn a_rule_line_goes_above_or_below_with_its_weight_and_gap() {
        let mut d = Document::new();
        d.body = vec![para_block(Paragraph::with_text("Riverside Library", CharProps::default()))];
        let mut s = Session::new(d);
        assert_eq!(run(&mut s, "spec.ruleLineInfo", json!({}))["line"], "none");
        // First press: a line below, 1 pt, 1 pt from the text.
        assert_eq!(run(&mut s, "spec.ruleLine", json!({})), json!({"line": "below", "weight": 1.0, "gap": 1.0, "color": null}));
        // Weight and gap change the line that is there.
        run(&mut s, "spec.ruleLine", json!({"weight": 3.0, "gap": 6.0}));
        let b = s.doc.body[0].as_para().unwrap().props.borders.unwrap();
        assert_eq!((b.top, b.bottom.map(|x| (x.width, x.space))), (None, Some((3.0, 6.0))));
        assert_eq!(run(&mut s, "spec.ruleLine", json!({"line": "both"}))["weight"], 3.0);
        assert!(s.doc.body[0].as_para().unwrap().props.borders.unwrap().top.is_some());
        run(&mut s, "spec.ruleLine", json!({"line": "none"}));
        assert_eq!(run(&mut s, "spec.ruleLineInfo", json!({}))["line"], "none");
        assert!(s.run("spec.ruleLine", &json!({"weight": 40})).is_err());
        assert!(s.run("spec.ruleLine", &json!({"line": "left"})).is_err());
    }
}
