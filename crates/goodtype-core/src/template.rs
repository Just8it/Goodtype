//! Page templates — ruled, dotted, squared, Cornell — described as numbers rather than drawn.
//!
//! A template is not an image and not an SVG file. It is a handful of repeat rules that get
//! resolved against the page's own geometry. Three reasons that shape is worth the constraint:
//!
//! - **It adapts.** A grid described as "14pt spacing" works on A4, Letter, and whatever custom
//!   size somebody types in. A grid *drawn* at A4 has to be stretched into being wrong.
//! - **It is safe.** Templates are the first part of the format meant to be shared and imported,
//!   and a definition here can express nothing but numbers and labels — no expressions, no code,
//!   no external references. An SVG file could express all three.
//! - **It renders the same twice.** Screen and PDF have separate renderers, so anything drawn on
//!   the page has to be computed identically in both. Numbers can be; artwork has to be trusted.
//!
//! `resolve` is the half that must not drift from its TypeScript mirror in
//! `apps/desktop/src/lib/page/template.ts`. Both are pinned by `fixtures/templates/resolved.json`.
//! Steps are computed as `start + index * spacing` rather than accumulated, so the two agree bit
//! for bit instead of drifting apart one rounding error at a time.

use serde::{Deserialize, Serialize};

use crate::PageGeometry;

/// Below this, a template stops being paper and starts being a fill pattern that costs thousands
/// of shapes to draw. It is a validation floor, not a taste preference.
pub const MIN_SPACING_PT: f64 = 3.0;
pub const MAX_ELEMENTS: usize = 16;
/// A ceiling on what one page can be asked to draw, so a legal definition still cannot be a
/// denial of service against the renderer.
pub const MAX_SHAPES: usize = 20_000;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PageTemplate {
    /// Stable across notebooks, so the picker can show which template a page already uses.
    pub id: String,
    /// What the writer called it. Travels with the definition, since the page is the only place
    /// the name is guaranteed to still exist.
    pub name: String,
    /// The paper itself.
    pub background_color: String,
    /// Inset from every edge that the repeating elements stay inside.
    pub margin_pt: f64,
    pub elements: Vec<TemplateElement>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum TemplateElement {
    /// Rules repeating down the page: ruled paper, and the horizontal half of squared paper.
    HorizontalLines {
        spacing_pt: f64,
        /// Extra distance from the top margin before the first rule.
        offset_pt: f64,
        color: String,
        weight_pt: f64,
    },
    /// Rules repeating across the page: columns, and the vertical half of squared paper.
    VerticalLines {
        spacing_pt: f64,
        offset_pt: f64,
        color: String,
        weight_pt: f64,
    },
    /// A dot grid. One spacing for both axes — dotted paper with different spacings reads as a
    /// mistake rather than a choice.
    Dots {
        spacing_pt: f64,
        offset_pt: f64,
        color: String,
        radius_pt: f64,
    },
    /// A single rule at a fixed distance from one edge: a legal pad's margin, Cornell's cue
    /// column and summary bar.
    Rule {
        edge: Edge,
        offset_pt: f64,
        color: String,
        weight_pt: f64,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

/// What a template becomes once it knows the page it is on: flat geometry both renderers draw
/// without having to agree on anything but numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TemplateShape<'a> {
    Line {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        color: &'a str,
        weight_pt: f64,
    },
    Dot {
        cx: f64,
        cy: f64,
        radius_pt: f64,
        color: &'a str,
    },
}

/// Resolve a template against a page. Silently truncates at `MAX_SHAPES`; validation is what
/// rejects definitions that would get there, and a renderer is the wrong place to raise it.
pub fn resolve<'a>(template: &'a PageTemplate, geometry: &PageGeometry) -> Vec<TemplateShape<'a>> {
    let left = template.margin_pt;
    let top = template.margin_pt;
    let right = geometry.width_pt - template.margin_pt;
    let bottom = geometry.height_pt - template.margin_pt;
    let mut shapes = Vec::new();
    if right <= left || bottom <= top {
        return shapes;
    }

    for element in &template.elements {
        match element {
            TemplateElement::HorizontalLines {
                spacing_pt,
                offset_pt,
                color,
                weight_pt,
            } => {
                for index in 0..steps(top + offset_pt, bottom, *spacing_pt) {
                    let y = top + offset_pt + index as f64 * spacing_pt;
                    shapes.push(TemplateShape::Line {
                        x1: left,
                        y1: y,
                        x2: right,
                        y2: y,
                        color,
                        weight_pt: *weight_pt,
                    });
                }
            }
            TemplateElement::VerticalLines {
                spacing_pt,
                offset_pt,
                color,
                weight_pt,
            } => {
                for index in 0..steps(left + offset_pt, right, *spacing_pt) {
                    let x = left + offset_pt + index as f64 * spacing_pt;
                    shapes.push(TemplateShape::Line {
                        x1: x,
                        y1: top,
                        x2: x,
                        y2: bottom,
                        color,
                        weight_pt: *weight_pt,
                    });
                }
            }
            TemplateElement::Dots {
                spacing_pt,
                offset_pt,
                color,
                radius_pt,
            } => {
                let rows = steps(top + offset_pt, bottom, *spacing_pt);
                let columns = steps(left + offset_pt, right, *spacing_pt);
                for row in 0..rows {
                    let cy = top + offset_pt + row as f64 * spacing_pt;
                    for column in 0..columns {
                        shapes.push(TemplateShape::Dot {
                            cx: left + offset_pt + column as f64 * spacing_pt,
                            cy,
                            radius_pt: *radius_pt,
                            color,
                        });
                    }
                }
            }
            TemplateElement::Rule {
                edge,
                offset_pt,
                color,
                weight_pt,
            } => {
                // Measured from the page edge rather than the margin: a legal pad's rule is a
                // margin of its own, so making it relative to another one reads backwards.
                let shape = match edge {
                    Edge::Left => TemplateShape::Line {
                        x1: *offset_pt,
                        y1: 0.0,
                        x2: *offset_pt,
                        y2: geometry.height_pt,
                        color,
                        weight_pt: *weight_pt,
                    },
                    Edge::Right => TemplateShape::Line {
                        x1: geometry.width_pt - offset_pt,
                        y1: 0.0,
                        x2: geometry.width_pt - offset_pt,
                        y2: geometry.height_pt,
                        color,
                        weight_pt: *weight_pt,
                    },
                    Edge::Top => TemplateShape::Line {
                        x1: 0.0,
                        y1: *offset_pt,
                        x2: geometry.width_pt,
                        y2: *offset_pt,
                        color,
                        weight_pt: *weight_pt,
                    },
                    Edge::Bottom => TemplateShape::Line {
                        x1: 0.0,
                        y1: geometry.height_pt - offset_pt,
                        x2: geometry.width_pt,
                        y2: geometry.height_pt - offset_pt,
                        color,
                        weight_pt: *weight_pt,
                    },
                };
                shapes.push(shape);
            }
        }
        if shapes.len() >= MAX_SHAPES {
            shapes.truncate(MAX_SHAPES);
            break;
        }
    }
    shapes
}

/// How many steps of `spacing` fit from `start` up to and including `end`.
fn steps(start: f64, end: f64, spacing: f64) -> usize {
    if !spacing.is_finite() || spacing < MIN_SPACING_PT || start > end {
        return 0;
    }
    ((end - start) / spacing).floor() as usize + 1
}

/// Whether a definition is safe and sane to store. Called from storage on the way in, so a page
/// that made it to disk can be resolved without further checking.
pub fn validate(template: &PageTemplate) -> Result<(), &'static str> {
    if template.id.is_empty() || template.id.len() > 64 {
        return Err("template id must be present and bounded");
    }
    if template.name.is_empty() || template.name.len() > 96 {
        return Err("template name must be present and bounded");
    }
    if !valid_color(&template.background_color) {
        return Err("template background must be a hex colour");
    }
    if !(0.0..=200.0).contains(&template.margin_pt) {
        return Err("template margin must be between 0 and 200pt");
    }
    if template.elements.len() > MAX_ELEMENTS {
        return Err("template has more elements than a page can usefully carry");
    }
    for element in &template.elements {
        let (color, measure) = match element {
            TemplateElement::HorizontalLines {
                spacing_pt,
                offset_pt,
                color,
                weight_pt,
            }
            | TemplateElement::VerticalLines {
                spacing_pt,
                offset_pt,
                color,
                weight_pt,
            } => {
                if !spacing_pt.is_finite() || *spacing_pt < MIN_SPACING_PT || *spacing_pt > 500.0 {
                    return Err("template spacing must be between 3 and 500pt");
                }
                if !(0.0..=500.0).contains(offset_pt) {
                    return Err("template offset must be between 0 and 500pt");
                }
                (color, *weight_pt)
            }
            TemplateElement::Dots {
                spacing_pt,
                offset_pt,
                color,
                radius_pt,
            } => {
                if !spacing_pt.is_finite() || *spacing_pt < MIN_SPACING_PT || *spacing_pt > 500.0 {
                    return Err("template spacing must be between 3 and 500pt");
                }
                if !(0.0..=500.0).contains(offset_pt) {
                    return Err("template offset must be between 0 and 500pt");
                }
                (color, *radius_pt)
            }
            TemplateElement::Rule {
                offset_pt,
                color,
                weight_pt,
                ..
            } => {
                if !(0.0..=2000.0).contains(offset_pt) {
                    return Err("template rule offset must be between 0 and 2000pt");
                }
                (color, *weight_pt)
            }
        };
        if !valid_color(color) {
            return Err("template element colour must be a hex colour");
        }
        if !measure.is_finite() || measure <= 0.0 || measure > 8.0 {
            return Err("template line weight and dot radius must be between 0 and 8pt");
        }
    }
    Ok(())
}

fn valid_color(color: &str) -> bool {
    matches!(color.len(), 7 | 9)
        && color.starts_with('#')
        && color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape encoding the shared fixture uses. Owned rather than borrowed so it can be
    /// deserialized, and compared against `TemplateShape` field by field.
    #[derive(serde::Deserialize)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    enum ExpectedShape {
        #[serde(rename_all = "camelCase")]
        Line {
            x1: f64,
            y1: f64,
            x2: f64,
            y2: f64,
            color: String,
            weight_pt: f64,
        },
        #[serde(rename_all = "camelCase")]
        Dot {
            cx: f64,
            cy: f64,
            radius_pt: f64,
            color: String,
        },
    }

    #[derive(serde::Deserialize)]
    struct Case {
        name: String,
        geometry: PageGeometry,
        template: PageTemplate,
        expected: Vec<ExpectedShape>,
    }

    /// The same fixture the frontend asserts against. If the two resolvers drift, one of them
    /// fails here and the screen cannot start disagreeing with the PDF unnoticed.
    #[test]
    fn matches_the_shared_template_fixture() {
        let raw = include_str!("../../../fixtures/templates/resolved.json");
        let cases: Vec<Case> = serde_json::from_str(raw).unwrap();
        assert!(!cases.is_empty(), "fixture should describe cases");

        for case in &cases {
            let produced = resolve(&case.template, &case.geometry);
            assert_eq!(
                produced.len(),
                case.expected.len(),
                "case `{}` produced a different number of shapes",
                case.name
            );
            for (index, (made, want)) in produced.iter().zip(&case.expected).enumerate() {
                let same = match (made, want) {
                    (
                        TemplateShape::Line {
                            x1,
                            y1,
                            x2,
                            y2,
                            color,
                            weight_pt,
                        },
                        ExpectedShape::Line {
                            x1: ex1,
                            y1: ey1,
                            x2: ex2,
                            y2: ey2,
                            color: ecolor,
                            weight_pt: eweight,
                        },
                    ) => {
                        (x1, y1, x2, y2, *color, weight_pt)
                            == (ex1, ey1, ex2, ey2, ecolor.as_str(), eweight)
                    }
                    (
                        TemplateShape::Dot {
                            cx,
                            cy,
                            radius_pt,
                            color,
                        },
                        ExpectedShape::Dot {
                            cx: ecx,
                            cy: ecy,
                            radius_pt: eradius,
                            color: ecolor,
                        },
                    ) => (cx, cy, radius_pt, *color) == (ecx, ecy, eradius, ecolor.as_str()),
                    _ => false,
                };
                assert!(same, "case `{}` drifted at shape {index}", case.name);
            }
        }
    }

    fn geometry() -> PageGeometry {
        PageGeometry {
            width_pt: 100.0,
            height_pt: 100.0,
        }
    }

    fn template(elements: Vec<TemplateElement>) -> PageTemplate {
        PageTemplate {
            id: "test".into(),
            name: "Test".into(),
            background_color: "#FFFFFF".into(),
            margin_pt: 10.0,
            elements,
        }
    }

    #[test]
    fn repeating_rules_stay_inside_the_margin() {
        let ruled = template(vec![TemplateElement::HorizontalLines {
            spacing_pt: 20.0,
            offset_pt: 0.0,
            color: "#DDDDDD".into(),
            weight_pt: 0.5,
        }]);
        let resolved = resolve(&ruled, &geometry());
        // 10, 30, 50, 70, 90 — the last one lands exactly on the bottom margin and counts.
        assert_eq!(resolved.len(), 5);
        let TemplateShape::Line { x1, y1, x2, .. } = resolved[0] else {
            panic!("expected a line");
        };
        assert_eq!((x1, y1, x2), (10.0, 10.0, 90.0));
        let TemplateShape::Line { y1: last, .. } = resolved[4] else {
            panic!("expected a line");
        };
        assert_eq!(last, 90.0);
    }

    #[test]
    fn a_rule_is_measured_from_the_page_edge_not_the_margin() {
        let legal = template(vec![TemplateElement::Rule {
            edge: Edge::Left,
            offset_pt: 25.0,
            color: "#E5645E".into(),
            weight_pt: 1.0,
        }]);
        let resolved = resolve(&legal, &geometry());
        assert_eq!(
            resolved,
            vec![TemplateShape::Line {
                x1: 25.0,
                y1: 0.0,
                x2: 25.0,
                y2: 100.0,
                color: "#E5645E",
                weight_pt: 1.0,
            }]
        );
    }

    #[test]
    fn a_page_smaller_than_its_margins_draws_nothing() {
        let mut small = template(vec![TemplateElement::Dots {
            spacing_pt: 10.0,
            offset_pt: 0.0,
            color: "#CCCCCC".into(),
            radius_pt: 0.6,
        }]);
        small.margin_pt = 60.0;
        assert!(resolve(&small, &geometry()).is_empty());
    }

    #[test]
    fn validation_rejects_what_would_flood_the_page() {
        let mut dense = template(vec![TemplateElement::HorizontalLines {
            spacing_pt: 0.01,
            offset_pt: 0.0,
            color: "#DDDDDD".into(),
            weight_pt: 0.5,
        }]);
        assert!(validate(&dense).is_err());

        dense.elements = vec![TemplateElement::Dots {
            spacing_pt: 14.0,
            offset_pt: 0.0,
            color: "not a colour".into(),
            radius_pt: 0.6,
        }];
        assert!(validate(&dense).is_err());

        assert!(validate(&template(vec![])).is_ok());
    }
}
