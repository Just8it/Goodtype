//! Page templates — ruled, dotted, squared, Cornell — described as numbers rather than drawn.
//!
//! A template is not an image and not an SVG file. It is a handful of repeat rules that get
//! resolved against the page's own geometry. Three reasons that shape is worth the constraint:
//!
//! - **It adapts.** A grid described as "5mm spacing" works on A4, Letter, and whatever custom
//!   size somebody types in. A grid *drawn* at A4 has to be stretched into being wrong.
//! - **It is safe.** Templates are the first part of the format meant to be shared and imported,
//!   and a definition here can express nothing but numbers and labels — no expressions, no code,
//!   no external references. An SVG file could express all three.
//! - **It renders the same twice.** Screen and PDF have separate renderers, so anything drawn on
//!   the page has to be computed identically in both. Numbers can be; artwork has to be trusted.
//!
//! # Where the lines actually go
//!
//! Real squared paper has a whole number of exact squares with **equal margins on both sides**:
//! the manufacturer picks the ruling, cuts the sheet, and the leftover is split between the two
//! edges. Stepping from one margin until you run out is what a naive implementation does, and it
//! dumps the entire remainder against the opposite edge — visibly lopsided, and wrong.
//!
//! So a repeating element is laid out **symmetrically about the centre of its area**: positions
//! are `centre ± k * spacing`, for as many whole steps as fit. Two consequences, both wanted:
//!
//! - The first and last lines are equidistant from their edges, whatever the page size, because
//!   `(available / 2) - n * spacing` is the same gap at each end by construction.
//! - Spacings that are multiples of one another **land on each other**. Graph paper is a 5mm grid
//!   with every fifth line heavier; as two elements sharing an area and a centre, the 25mm lines
//!   fall exactly on 5mm lines instead of drifting through them.
//!
//! Spacing itself is never adjusted to fit. A 5mm square is 5mm or the paper is lying.
//!
//! `resolve` is the half that must not drift from its TypeScript mirror in
//! `apps/desktop/src/lib/page/template.ts`. Both are pinned by `fixtures/templates/resolved.json`.
//! Steps are computed as `first + index * spacing` rather than accumulated, so the two agree bit
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
    pub elements: Vec<TemplateElement>,
}

/// The rectangle an element lives in, as insets from each page edge.
///
/// Carried per element rather than once per template because that is what the layouts need:
/// a legal pad wants a wider left inset than the rest, and Cornell's ruling has to start at the
/// cue column and stop at the summary band rather than running through both.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Area {
    pub top_pt: f64,
    pub right_pt: f64,
    pub bottom_pt: f64,
    pub left_pt: f64,
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
        area: Area,
        spacing_pt: f64,
        color: String,
        weight_pt: f64,
    },
    /// Rules repeating across the page: columns, and the vertical half of squared paper.
    VerticalLines {
        area: Area,
        spacing_pt: f64,
        color: String,
        weight_pt: f64,
    },
    /// A dot grid. One spacing for both axes — dotted paper with different spacings reads as a
    /// mistake rather than a choice.
    Dots {
        area: Area,
        spacing_pt: f64,
        color: String,
        radius_pt: f64,
    },
    /// A single rule at a fixed distance from one page edge, spanning its area the other way:
    /// a legal pad's margin, Cornell's cue column and summary band.
    ///
    /// Not centred — the whole point of it is to sit at a measured distance.
    Rule {
        area: Area,
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
    /// Offset from the page's vertical centreline. A column divider has to stay in the middle
    /// whatever the page size, and a fixed distance from an edge cannot say that.
    CenterX,
    /// Offset from the page's horizontal centreline.
    CenterY,
}

impl Edge {
    fn vertical(self) -> bool {
        matches!(self, Edge::Left | Edge::Right | Edge::CenterX)
    }
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
    let mut shapes = Vec::new();

    for element in &template.elements {
        let Some(box_) = bounds(element_area(element), geometry) else {
            continue;
        };
        match element {
            TemplateElement::HorizontalLines {
                spacing_pt,
                color,
                weight_pt,
                ..
            } => {
                for y in centred(box_.top, box_.bottom, *spacing_pt) {
                    shapes.push(TemplateShape::Line {
                        x1: box_.left,
                        y1: y,
                        x2: box_.right,
                        y2: y,
                        color,
                        weight_pt: *weight_pt,
                    });
                }
            }
            TemplateElement::VerticalLines {
                spacing_pt,
                color,
                weight_pt,
                ..
            } => {
                for x in centred(box_.left, box_.right, *spacing_pt) {
                    shapes.push(TemplateShape::Line {
                        x1: x,
                        y1: box_.top,
                        x2: x,
                        y2: box_.bottom,
                        color,
                        weight_pt: *weight_pt,
                    });
                }
            }
            TemplateElement::Dots {
                spacing_pt,
                color,
                radius_pt,
                ..
            } => {
                let columns = centred(box_.left, box_.right, *spacing_pt);
                for cy in centred(box_.top, box_.bottom, *spacing_pt) {
                    for cx in &columns {
                        shapes.push(TemplateShape::Dot {
                            cx: *cx,
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
                ..
            } => {
                // Measured from the page edge rather than the area, because the offset is the
                // whole point of it: "1.75in from the left" has to mean that.
                let along = match edge {
                    Edge::Left => *offset_pt,
                    Edge::Right => geometry.width_pt - offset_pt,
                    Edge::Top => *offset_pt,
                    Edge::Bottom => geometry.height_pt - offset_pt,
                    Edge::CenterX => geometry.width_pt / 2.0 + offset_pt,
                    Edge::CenterY => geometry.height_pt / 2.0 + offset_pt,
                };
                shapes.push(if edge.vertical() {
                    TemplateShape::Line {
                        x1: along,
                        y1: box_.top,
                        x2: along,
                        y2: box_.bottom,
                        color,
                        weight_pt: *weight_pt,
                    }
                } else {
                    TemplateShape::Line {
                        x1: box_.left,
                        y1: along,
                        x2: box_.right,
                        y2: along,
                        color,
                        weight_pt: *weight_pt,
                    }
                });
            }
        }
        if shapes.len() >= MAX_SHAPES {
            shapes.truncate(MAX_SHAPES);
            break;
        }
    }
    shapes
}

struct Bounds {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

fn bounds(area: &Area, geometry: &PageGeometry) -> Option<Bounds> {
    let box_ = Bounds {
        left: area.left_pt,
        top: area.top_pt,
        right: geometry.width_pt - area.right_pt,
        bottom: geometry.height_pt - area.bottom_pt,
    };
    (box_.right > box_.left && box_.bottom > box_.top).then_some(box_)
}

fn element_area(element: &TemplateElement) -> &Area {
    match element {
        TemplateElement::HorizontalLines { area, .. }
        | TemplateElement::VerticalLines { area, .. }
        | TemplateElement::Dots { area, .. }
        | TemplateElement::Rule { area, .. } => area,
    }
}

/// Positions of `spacing`-apart steps laid out symmetrically about the middle of `start..end`.
///
/// This is what gives equal margins on both sides and makes multiples of a spacing land on each
/// other. See the module comment for why that matters.
fn centred(start: f64, end: f64, spacing: f64) -> Vec<f64> {
    if !spacing.is_finite() || spacing < MIN_SPACING_PT || end <= start {
        return Vec::new();
    }
    let centre = (start + end) / 2.0;
    // Whole steps that fit between the centre and either edge.
    let reach = ((end - start) / 2.0 / spacing).floor();
    let count = (reach * 2.0) as usize + 1;
    let first = centre - reach * spacing;
    (0..count)
        .map(|index| first + index as f64 * spacing)
        .collect()
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
    if template.elements.len() > MAX_ELEMENTS {
        return Err("template has more elements than a page can usefully carry");
    }
    for element in &template.elements {
        let area = element_area(element);
        for inset in [area.top_pt, area.right_pt, area.bottom_pt, area.left_pt] {
            if !(0.0..=2000.0).contains(&inset) {
                return Err("template area insets must be between 0 and 2000pt");
            }
        }
        let (color, measure) = match element {
            TemplateElement::HorizontalLines {
                spacing_pt,
                color,
                weight_pt,
                ..
            }
            | TemplateElement::VerticalLines {
                spacing_pt,
                color,
                weight_pt,
                ..
            } => {
                valid_spacing(*spacing_pt)?;
                (color, *weight_pt)
            }
            TemplateElement::Dots {
                spacing_pt,
                color,
                radius_pt,
                ..
            } => {
                valid_spacing(*spacing_pt)?;
                (color, *radius_pt)
            }
            TemplateElement::Rule {
                offset_pt,
                color,
                weight_pt,
                ..
            } => {
                // Signed, because a centre-relative rule can sit either side of the centreline.
                if !(-2000.0..=2000.0).contains(offset_pt) {
                    return Err("template rule offset must be within 2000pt");
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

fn valid_spacing(spacing_pt: f64) -> Result<(), &'static str> {
    if !spacing_pt.is_finite() || !(MIN_SPACING_PT..=500.0).contains(&spacing_pt) {
        return Err("template spacing must be between 3 and 500pt");
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

    fn area(inset: f64) -> Area {
        Area {
            top_pt: inset,
            right_pt: inset,
            bottom_pt: inset,
            left_pt: inset,
        }
    }

    fn template(elements: Vec<TemplateElement>) -> PageTemplate {
        PageTemplate {
            id: "test".into(),
            name: "Test".into(),
            background_color: "#FFFFFF".into(),
            elements,
        }
    }

    /// The complaint that produced this layout: stepping from one margin dumps the whole
    /// remainder against the opposite edge. Real paper splits it.
    #[test]
    fn margins_come_out_equal_on_both_sides() {
        // 100 wide, 10pt inset, 80pt of room, 25pt spacing: three lines span 50pt, so 15pt is
        // left over and 7.5pt of it belongs at each end.
        let ruled = template(vec![TemplateElement::VerticalLines {
            area: area(10.0),
            spacing_pt: 25.0,
            color: "#DDDDDD".into(),
            weight_pt: 0.5,
        }]);
        let geometry = PageGeometry {
            width_pt: 100.0,
            height_pt: 100.0,
        };
        let resolved = resolve(&ruled, &geometry);
        assert_eq!(resolved.len(), 3);
        let x_of = |shape: &TemplateShape| match shape {
            TemplateShape::Line { x1, .. } => *x1,
            _ => panic!("expected a line"),
        };
        assert_eq!(x_of(&resolved[0]), 25.0);
        assert_eq!(x_of(&resolved[2]), 75.0);
        // The gap at the left edge and the gap at the right edge are the same.
        assert_eq!(x_of(&resolved[0]) - 10.0, 90.0 - x_of(&resolved[2]));
    }

    /// Graph paper is a fine grid with every fifth line heavier. If the two spacings were each
    /// centred on their own leftover, the heavy lines would drift through the fine ones.
    #[test]
    fn a_coarse_spacing_lands_on_the_fine_one_it_is_a_multiple_of() {
        let graph = template(vec![
            TemplateElement::VerticalLines {
                area: area(36.0),
                spacing_pt: 5.0,
                color: "#DDDDDD".into(),
                weight_pt: 0.3,
            },
            TemplateElement::VerticalLines {
                area: area(36.0),
                spacing_pt: 25.0,
                color: "#BBBBBB".into(),
                weight_pt: 0.7,
            },
        ]);
        let geometry = PageGeometry {
            width_pt: 595.2756,
            height_pt: 841.8898,
        };
        let resolved = resolve(&graph, &geometry);
        let fine: Vec<f64> = resolved
            .iter()
            .filter_map(|shape| match shape {
                TemplateShape::Line {
                    x1, weight_pt: 0.3, ..
                } => Some(*x1),
                _ => None,
            })
            .collect();
        let coarse: Vec<f64> = resolved
            .iter()
            .filter_map(|shape| match shape {
                TemplateShape::Line {
                    x1, weight_pt: 0.7, ..
                } => Some(*x1),
                _ => None,
            })
            .collect();
        assert!(!coarse.is_empty());
        for heavy in coarse {
            assert!(
                fine.iter().any(|light| (light - heavy).abs() < 1e-9),
                "heavy line at {heavy} misses every fine line"
            );
        }
    }

    #[test]
    fn a_rule_is_measured_from_the_page_edge_and_spans_its_area() {
        let legal = template(vec![TemplateElement::Rule {
            area: Area {
                top_pt: 20.0,
                right_pt: 10.0,
                bottom_pt: 30.0,
                left_pt: 10.0,
            },
            edge: Edge::Left,
            offset_pt: 25.0,
            color: "#E5645E".into(),
            weight_pt: 1.0,
        }]);
        let resolved = resolve(
            &legal,
            &PageGeometry {
                width_pt: 100.0,
                height_pt: 100.0,
            },
        );
        assert_eq!(
            resolved,
            vec![TemplateShape::Line {
                x1: 25.0,
                y1: 20.0,
                x2: 25.0,
                y2: 70.0,
                color: "#E5645E",
                weight_pt: 1.0,
            }]
        );
    }

    #[test]
    fn a_page_smaller_than_its_margins_draws_nothing() {
        let cramped = template(vec![TemplateElement::Dots {
            area: area(60.0),
            spacing_pt: 10.0,
            color: "#CCCCCC".into(),
            radius_pt: 0.6,
        }]);
        assert!(
            resolve(
                &cramped,
                &PageGeometry {
                    width_pt: 100.0,
                    height_pt: 100.0
                }
            )
            .is_empty()
        );
    }

    #[test]
    fn validation_rejects_what_would_flood_the_page() {
        let mut dense = template(vec![TemplateElement::HorizontalLines {
            area: area(0.0),
            spacing_pt: 0.01,
            color: "#DDDDDD".into(),
            weight_pt: 0.5,
        }]);
        assert!(validate(&dense).is_err());

        dense.elements = vec![TemplateElement::Dots {
            area: area(0.0),
            spacing_pt: 14.0,
            color: "not a colour".into(),
            radius_pt: 0.6,
        }];
        assert!(validate(&dense).is_err());

        assert!(validate(&template(vec![])).is_ok());
    }
}
