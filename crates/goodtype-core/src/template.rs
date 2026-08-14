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
//! - The layout is symmetric, so anything else placed about the same centre agrees with it —
//!   which is what lets a grid's major lines be the *same* lines, drawn heavier.
//!
//! Spacing itself is never adjusted to fit. A 5mm square is 5mm or the paper is lying.
//!
//! Squared paper is one `Grid` rather than two line sets for a related reason: independent sets
//! each span the whole area, so they overshoot each other's outermost lines and leave a stub
//! cell with a tail hanging off it all the way round the edge.
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
    /// Squared paper: both axes at once, and deliberately not two line sets.
    ///
    /// Independent sets each span the whole area, but their first and last lines sit inset from
    /// it, so the rules overshoot the outermost lines of the other axis and every cell around
    /// the edge is a stub with a tail hanging off it. A grid knows both extents, so it can run
    /// each rule from the first line of the other axis to the last and close its own corners.
    Grid {
        area: Area,
        spacing_pt: f64,
        color: String,
        weight_pt: f64,
        /// Every n-th line from the centre drawn heavier — what makes a graph grid countable
        /// rather than just dense. `None` for plain squares.
        major: Option<GridMajor>,
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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GridMajor {
    /// Counted outwards from the centre line, so the heavy lines stay symmetric with the grid
    /// they sit on however the page is sized.
    pub every: u32,
    pub color: String,
    pub weight_pt: f64,
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

/// Resolve a template against a page, silently truncating at [`MAX_SHAPES`].
///
/// The truncation is the real ceiling, not a backstop behind validation. [`validate`] never sees
/// a [`PageGeometry`], and shape count is a function of the page as much as the definition, so it
/// could not enforce this even in principle — it bounds spacing and insets, which is what it can
/// see. Silent rather than an error because a renderer is the wrong place to raise one: by the
/// time a page is being drawn the notebook is already open, and refusing to draw the paper would
/// take the writing with it.
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
            TemplateElement::Grid {
                spacing_pt,
                color,
                weight_pt,
                major,
                ..
            } => {
                let xs = centred(box_.left, box_.right, *spacing_pt);
                let ys = centred(box_.top, box_.bottom, *spacing_pt);
                let (Some(&x0), Some(&x1), Some(&y0), Some(&y1)) =
                    (xs.first(), xs.last(), ys.first(), ys.last())
                else {
                    continue;
                };
                let heavy = |index: usize, count: usize| {
                    major.as_ref().filter(|major| {
                        major.every > 0
                            && index
                                .abs_diff(count / 2)
                                .is_multiple_of(major.every as usize)
                    })
                };
                // Minor lines first, then the major ones over them, so a heavy rule reads as
                // sitting on the grid rather than being interrupted by it.
                for want_major in [false, true] {
                    for (index, &y) in ys.iter().enumerate() {
                        let found = heavy(index, ys.len());
                        if found.is_some() != want_major {
                            continue;
                        }
                        shapes.push(TemplateShape::Line {
                            x1: x0,
                            y1: y,
                            x2: x1,
                            y2: y,
                            color: found.map_or(color.as_str(), |major| major.color.as_str()),
                            weight_pt: found.map_or(*weight_pt, |major| major.weight_pt),
                        });
                    }
                    for (index, &x) in xs.iter().enumerate() {
                        let found = heavy(index, xs.len());
                        if found.is_some() != want_major {
                            continue;
                        }
                        shapes.push(TemplateShape::Line {
                            x1: x,
                            y1: y0,
                            x2: x,
                            y2: y1,
                            color: found.map_or(color.as_str(), |major| major.color.as_str()),
                            weight_pt: found.map_or(*weight_pt, |major| major.weight_pt),
                        });
                    }
                }
            }
            TemplateElement::Dots {
                spacing_pt,
                color,
                radius_pt,
                ..
            } => {
                let columns = centred(box_.left, box_.right, *spacing_pt);
                // The one element whose cost is the product of two axes rather than their sum,
                // so the ceiling is enforced as it draws instead of once the grid is complete.
                'rows: for cy in centred(box_.top, box_.bottom, *spacing_pt) {
                    for cx in &columns {
                        if shapes.len() >= MAX_SHAPES {
                            break 'rows;
                        }
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
        | TemplateElement::Grid { area, .. }
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
    // Capped here rather than trusted from the caller. `count` is allocated up front and
    // `as usize` saturates on a large float, so without this an extreme page produced either a
    // multi-gigabyte allocation or — once the cast saturated at `usize::MAX` — an overflow on
    // the `+ 1`. The cap is far above any real ruling, so it changes nothing a page can ask for.
    let count = ((reach * 2.0) as usize).saturating_add(1).min(MAX_STEPS);
    let first = centre - reach * spacing;
    (0..count)
        .map(|index| first + index as f64 * spacing)
        .collect()
}

/// The most steps one axis of one element may contribute.
///
/// This bounds the *allocation*, not the drawing: [`MAX_SHAPES`] bounds that. Set well clear of
/// real paper — the tightest ruling this format allows on the longest page is
/// `MAX_PAGE_DIMENSION_PT / MIN_SPACING_PT`, and ordinary A4 graph paper is around 150 — so a
/// template that reaches this was never describing paper.
const MAX_STEPS: usize = MAX_SHAPES;

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
            TemplateElement::Grid {
                spacing_pt,
                color,
                weight_pt,
                major,
                ..
            } => {
                valid_spacing(*spacing_pt)?;
                if let Some(major) = major {
                    if major.every > 64 {
                        return Err("template grid major interval must be at most 64");
                    }
                    if !valid_color(&major.color) {
                        return Err("template element colour must be a hex colour");
                    }
                    if !major.weight_pt.is_finite() || !(0.0..=8.0).contains(&major.weight_pt) {
                        return Err("template line weight and dot radius must be 0 to 8pt");
                    }
                }
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

use crate::valid_hex_color as valid_color;

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

    /// A grid's rules have to stop on the outermost line of the other axis. Two independent line
    /// sets each span the whole area, so they overshoot each other and leave a stub cell with a
    /// tail hanging off it all the way round the edge — which is what this replaced.
    #[test]
    fn a_grid_closes_its_own_corners() {
        let squared = template(vec![TemplateElement::Grid {
            area: area(10.0),
            spacing_pt: 25.0,
            color: "#DDDDDD".into(),
            weight_pt: 0.4,
            major: None,
        }]);
        let resolved = resolve(
            &squared,
            &PageGeometry {
                width_pt: 90.0,
                height_pt: 70.0,
            },
        );
        let horizontals: Vec<_> = resolved
            .iter()
            .filter(|shape| matches!(shape, TemplateShape::Line { y1, y2, .. } if y1 == y2))
            .collect();
        let verticals: Vec<_> = resolved
            .iter()
            .filter(|shape| matches!(shape, TemplateShape::Line { x1, x2, .. } if x1 == x2))
            .collect();
        assert_eq!((horizontals.len(), verticals.len()), (3, 3));

        let TemplateShape::Line { x1: left, .. } = verticals[0] else {
            panic!("expected a line");
        };
        let TemplateShape::Line { x1: right, .. } = verticals[2] else {
            panic!("expected a line");
        };
        for horizontal in horizontals {
            let TemplateShape::Line { x1, x2, .. } = horizontal else {
                panic!("expected a line");
            };
            assert_eq!((x1, x2), (left, right), "a rule overshot the grid");
        }
    }

    /// Graph paper is a fine grid with every fifth line heavier. As one grid the heavy lines are
    /// the same lines, so they cannot drift off the fine ones or stop short of them.
    #[test]
    fn major_lines_sit_exactly_on_the_grid_they_count() {
        let graph = template(vec![TemplateElement::Grid {
            area: area(36.0),
            spacing_pt: 5.0,
            color: "#DDDDDD".into(),
            weight_pt: 0.3,
            major: Some(GridMajor {
                every: 5,
                color: "#BBBBBB".into(),
                weight_pt: 0.7,
            }),
        }]);
        let resolved = resolve(
            &graph,
            &PageGeometry {
                width_pt: 595.2756,
                height_pt: 841.8898,
            },
        );
        let xs = |weight: f64| -> Vec<f64> {
            resolved
                .iter()
                .filter_map(|shape| match shape {
                    TemplateShape::Line {
                        x1, x2, weight_pt, ..
                    } if x1 == x2 && *weight_pt == weight => Some(*x1),
                    _ => None,
                })
                .collect()
        };
        let fine = xs(0.3);
        let heavy = xs(0.7);
        assert!(!heavy.is_empty());
        // Every fifth line is promoted, so the fine set is short by exactly what the heavy set
        // holds rather than the two being drawn on top of each other.
        assert!(!fine.iter().any(|light| heavy.contains(light)));
        for line in &heavy {
            let steps = (line - heavy[0]) / 25.0;
            assert!(
                (steps - steps.round()).abs() < 1e-9,
                "heavy line at {line} is not on the 25pt interval"
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

    /// The ceiling has to hold while the shapes are being made, not once they all exist.
    ///
    /// A dot grid costs the product of its two axes, so the page that used to run out of memory
    /// was not an enormous one — `MAX_PAGE_DIMENSION_PT` at the tightest legal spacing is about
    /// 33,000 steps a side, and the square of that is a hundred billion dots nobody can hold.
    #[test]
    fn the_largest_legal_page_still_resolves_promptly() {
        let dotted = template(vec![TemplateElement::Dots {
            area: area(0.0),
            spacing_pt: MIN_SPACING_PT,
            color: "#CCCCCC".into(),
            radius_pt: 0.6,
        }]);
        let resolved = resolve(
            &dotted,
            &PageGeometry {
                width_pt: crate::MAX_PAGE_DIMENSION_PT,
                height_pt: crate::MAX_PAGE_DIMENSION_PT,
            },
        );
        assert_eq!(resolved.len(), MAX_SHAPES);
    }

    /// Ordinary paper must be nowhere near the ceiling, or the cap that makes the pathological
    /// case safe would be quietly truncating real rulings.
    #[test]
    fn real_paper_is_far_below_the_ceiling() {
        let graph = template(vec![TemplateElement::Grid {
            area: area(36.0),
            spacing_pt: 5.0,
            color: "#DDDDDD".into(),
            weight_pt: 0.3,
            major: None,
        }]);
        let a4 = PageGeometry {
            width_pt: 595.2756,
            height_pt: 841.8898,
        };
        let resolved = resolve(&graph, &a4);
        // Both axes present in full: 5pt graph paper on A4 is ~150 rows, which an earlier
        // per-axis cap would have clipped.
        let horizontals = resolved
            .iter()
            .filter(|shape| matches!(shape, TemplateShape::Line { y1, y2, .. } if y1 == y2))
            .count();
        assert!(horizontals > 140, "{horizontals} rows is a truncated page");
        assert!(resolved.len() < MAX_SHAPES / 2, "{}", resolved.len());
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
