//! Canonical geometric shapes.
//!
//! A shape is stored as editable geometry, never as an SVG string. The frontend may use SVG to
//! render it and the exporter may generate SVG for Typst, but both are projections of these
//! points. Keeping the geometry small and bounded makes a page inspectable and prevents a fitted
//! freehand curve from becoming an unbounded second ink layer.

use serde::{Deserialize, Serialize};

use crate::{nonnegative, positive, valid_hex_color, valid_page_dimension};

/// A fitted curve should describe intent, not preserve every sampled pen event.
pub const MAX_SPLINE_NODES: usize = 256;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShapePoint {
    pub x: f64,
    pub y: f64,
}

impl ShapePoint {
    fn finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

/// One cubic-Bézier knot. Handles are vectors relative to `point`, so moving a knot does not
/// require rewriting two more absolute coordinates and preserves its tangent by construction.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BezierNode {
    pub point: ShapePoint,
    pub handle_in: Option<ShapePoint>,
    pub handle_out: Option<ShapePoint>,
}

impl BezierNode {
    fn valid(&self) -> bool {
        self.point.finite()
            && self.handle_in.is_none_or(|handle| handle.finite())
            && self.handle_out.is_none_or(|handle| handle.finite())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ShapeGeometry {
    Line {
        start: ShapePoint,
        end: ShapePoint,
    },
    Rectangle {
        width_pt: f64,
        height_pt: f64,
        corner_radius_pt: f64,
    },
    Ellipse {
        width_pt: f64,
        height_pt: f64,
    },
    Spline {
        nodes: Vec<BezierNode>,
        closed: bool,
    },
}

impl ShapeGeometry {
    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::Line { start, end } => {
                if !start.finite() || !end.finite() || start == end {
                    return Err("shape line needs two distinct finite points");
                }
            }
            Self::Rectangle {
                width_pt,
                height_pt,
                corner_radius_pt,
            } => {
                if !valid_page_dimension(*width_pt) || !valid_page_dimension(*height_pt) {
                    return Err("shape rectangle dimensions must be finite and positive");
                }
                let maximum = width_pt.min(*height_pt) / 2.0;
                if !nonnegative(*corner_radius_pt) || *corner_radius_pt > maximum {
                    return Err("shape corner radius must fit inside the rectangle");
                }
            }
            Self::Ellipse {
                width_pt,
                height_pt,
            } => {
                if !valid_page_dimension(*width_pt) || !valid_page_dimension(*height_pt) {
                    return Err("shape ellipse dimensions must be finite and positive");
                }
            }
            Self::Spline { nodes, closed } => {
                let minimum = if *closed { 3 } else { 2 };
                if nodes.len() < minimum || nodes.len() > MAX_SPLINE_NODES {
                    return Err("shape spline has an invalid number of nodes");
                }
                if nodes.iter().any(|node| !node.valid()) {
                    return Err("shape spline points and handles must be finite");
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShapeStyle {
    pub stroke_color: String,
    pub stroke_width_pt: f64,
    pub fill_color: Option<String>,
    pub opacity: f64,
}

impl ShapeStyle {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !valid_hex_color(&self.stroke_color)
            || self
                .fill_color
                .as_deref()
                .is_some_and(|color| !valid_hex_color(color))
        {
            return Err("shape colours must use canonical hex notation");
        }
        if !positive(self.stroke_width_pt) || self.stroke_width_pt > 256.0 {
            return Err("shape stroke width must be finite and bounded");
        }
        if !self.opacity.is_finite() || !(0.0..=1.0).contains(&self.opacity) {
            return Err("shape opacity must be between zero and one");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_degenerate_and_unbounded_geometry() {
        let point = ShapePoint { x: 4.0, y: 8.0 };
        assert!(
            ShapeGeometry::Line {
                start: point,
                end: point
            }
            .validate()
            .is_err()
        );
        assert!(
            ShapeGeometry::Rectangle {
                width_pt: 40.0,
                height_pt: 20.0,
                corner_radius_pt: 11.0,
            }
            .validate()
            .is_err()
        );
        assert!(
            ShapeGeometry::Spline {
                nodes: vec![],
                closed: false,
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn accepts_relative_bezier_handles_and_transparent_fill() {
        let geometry = ShapeGeometry::Spline {
            nodes: vec![
                BezierNode {
                    point: ShapePoint { x: 0.0, y: 0.0 },
                    handle_in: None,
                    handle_out: Some(ShapePoint { x: 12.0, y: 0.0 }),
                },
                BezierNode {
                    point: ShapePoint { x: 40.0, y: 20.0 },
                    handle_in: Some(ShapePoint { x: -12.0, y: 0.0 }),
                    handle_out: None,
                },
            ],
            closed: false,
        };
        let style = ShapeStyle {
            stroke_color: "#16212b".into(),
            stroke_width_pt: 1.6,
            fill_color: None,
            opacity: 1.0,
        };
        assert!(geometry.validate().is_ok());
        assert!(style.validate().is_ok());
    }
}
