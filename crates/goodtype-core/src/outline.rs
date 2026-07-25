//! Variable-width stroke geometry, shared by the live canvas and the PDF export.
//!
//! A stroke is not a stroked centreline: `stroke-width` is constant along a path, so a stroked
//! polyline can never vary with pressure. Instead this computes the ink's silhouette — offset the
//! centreline by half the local width on each side, and close the two sides into one polygon.
//!
//! This mirrors `apps/desktop/src/lib/ink/outline.ts`. The two are pinned together by
//! `fixtures/ink/outline.json`, so a change to one that is not made to the other fails
//! verification rather than silently making the export disagree with the screen.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutlinePoint {
    pub x: f64,
    pub y: f64,
    pub pressure: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutlineOptions {
    /// Nominal stroke width in points.
    pub width_pt: f64,
    /// When false, pressure is ignored and the stroke keeps a constant width.
    pub pressure: bool,
    /// Fraction of the stroke length over which the ends taper to a point; 0 disables.
    pub taper: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub x: f64,
    pub y: f64,
}

/// Narrowest a pressure-varying stroke gets, as a fraction of its nominal width.
const MIN_PRESSURE_SCALE: f64 = 0.25;

/// Samples closer together than this carry no usable direction — the vector between them is
/// dominated by pointer jitter, so the normal computed from it flips and bites a notch out of the
/// silhouette. Drawing slowly is what produces them, which is why a slow highlighter sweep came
/// out ragged while a quick one did not.
const MIN_SAMPLE_SPACING_PT: f64 = 0.05;

/// How far apart the two samples behind a central difference must be before the direction they
/// give is trusted. Larger than the dedup spacing, so a dense but legitimate curve is still
/// differentiated over a span long enough to mean something. Well-spaced samples reach this on
/// the immediate neighbours and behave exactly as a plain central difference.
const MIN_DIRECTION_SPAN_PT: f64 = 0.5;

/// A taper runs over at most this many nib widths.
///
/// It used to be a pure fraction of arc length, which made a full-page sweep taper over
/// centimetres while a flick tapered over millimetres. A real nib tapers over a distance set by
/// the nib, not by how far the hand happened to travel.
const TAPER_MAX_NIB_WIDTHS: f64 = 6.0;

/// Deliberately mimics JavaScript's `Math.round` (half toward positive infinity) rather than
/// using `f64::round` (half away from zero), because the two disagree on negative halves and the
/// mirrored implementations must produce identical numbers.
fn quantize(value: f64) -> f64 {
    (value * 1000.0 + 0.5).floor() / 1000.0
}

/// Distance between two samples, used to decide whether they are far enough apart to mean
/// anything.
fn span(a: OutlinePoint, b: OutlinePoint) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

fn width_at(point: &OutlinePoint, options: &OutlineOptions) -> f64 {
    if !options.pressure {
        return options.width_pt;
    }
    let clamped = point.pressure.clamp(0.0, 1.0);
    options.width_pt * (MIN_PRESSURE_SCALE + (1.0 - MIN_PRESSURE_SCALE) * clamped)
}

/// The closed silhouette of a stroke. Empty when there is nothing to draw; a single point becomes
/// a small square so a tap still leaves a mark.
pub fn outline_points(points: &[OutlinePoint], options: &OutlineOptions) -> Vec<Vertex> {
    if points.is_empty() {
        return Vec::new();
    }

    // Samples too close together carry no direction and would produce a noisy normal. Exact
    // duplicates were already dropped here; near-duplicates were not, and they are what a slowly
    // drawn stroke is made of.
    let mut path: Vec<OutlinePoint> = vec![points[0]];
    for point in &points[1..] {
        let previous = path[path.len() - 1];
        if (point.x - previous.x).hypot(point.y - previous.y) > MIN_SAMPLE_SPACING_PT {
            path.push(*point);
        }
    }

    if path.len() == 1 {
        let half = width_at(&path[0], options) / 2.0;
        let (x, y) = (path[0].x, path[0].y);
        return vec![
            Vertex {
                x: quantize(x - half),
                y: quantize(y - half),
            },
            Vertex {
                x: quantize(x + half),
                y: quantize(y - half),
            },
            Vertex {
                x: quantize(x + half),
                y: quantize(y + half),
            },
            Vertex {
                x: quantize(x - half),
                y: quantize(y + half),
            },
        ];
    }

    // Arc length drives the taper, so a slow stroke and a fast one taper over the same distance.
    let mut cumulative = Vec::with_capacity(path.len());
    cumulative.push(0.0);
    for index in 1..path.len() {
        let dx = path[index].x - path[index - 1].x;
        let dy = path[index].y - path[index - 1].y;
        cumulative.push(cumulative[index - 1] + dx.hypot(dy));
    }
    let total = cumulative[cumulative.len() - 1];
    let taper_length = if options.taper > 0.0 {
        (total * options.taper).min(options.width_pt * TAPER_MAX_NIB_WIDTHS)
    } else {
        0.0
    };

    let mut left = Vec::with_capacity(path.len());
    let mut right = Vec::with_capacity(path.len());

    // Carried so a stretch too short to differentiate reuses the last direction that meant
    // something, rather than snapping to a fixed axis and folding the outline over itself.
    let mut heading = (1.0_f64, 0.0_f64);

    for index in 0..path.len() {
        // Central difference, but taken over a span rather than over the immediate neighbours:
        // two samples a hundredth of a point apart describe jitter, not direction. Well-spaced
        // samples reach the span on their neighbours, so this is the plain central difference
        // wherever the stroke is not crawling.
        let mut back = index;
        while back > 0 && span(path[back], path[index]) < MIN_DIRECTION_SPAN_PT {
            back -= 1;
        }
        let mut forward = index;
        while forward + 1 < path.len() && span(path[forward], path[index]) < MIN_DIRECTION_SPAN_PT {
            forward += 1;
        }
        let mut dx = path[forward].x - path[back].x;
        let mut dy = path[forward].y - path[back].y;
        let length = dx.hypot(dy);
        if length > 0.0 {
            dx /= length;
            dy /= length;
            heading = (dx, dy);
        } else {
            (dx, dy) = heading;
        }

        let mut half = width_at(&path[index], options) / 2.0;
        if taper_length > 0.0 {
            let from_start = cumulative[index];
            let from_end = total - cumulative[index];
            let ramp = 1.0_f64
                .min(from_start / taper_length)
                .min(from_end / taper_length);
            half *= ramp;
        }

        // Normal is the direction rotated a quarter turn.
        let nx = -dy * half;
        let ny = dx * half;
        left.push(Vertex {
            x: quantize(path[index].x + nx),
            y: quantize(path[index].y + ny),
        });
        right.push(Vertex {
            x: quantize(path[index].x - nx),
            y: quantize(path[index].y - ny),
        });
    }

    right.reverse();
    left.extend(right);
    left
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Case {
        name: String,
        points: Vec<[f64; 3]>,
        width_pt: f64,
        pressure: bool,
        taper: f64,
        expected: Vec<[f64; 2]>,
    }

    /// The same fixture the frontend asserts against. If the two implementations drift, one of
    /// them fails here and the mismatch cannot reach a PDF unnoticed.
    #[test]
    fn matches_the_shared_outline_fixture() {
        let raw = include_str!("../../../fixtures/ink/outline.json");
        let cases: Vec<Case> = serde_json::from_str(raw).unwrap();
        assert!(!cases.is_empty(), "fixture should describe cases");

        for case in cases {
            let points: Vec<OutlinePoint> = case
                .points
                .iter()
                .map(|point| OutlinePoint {
                    x: point[0],
                    y: point[1],
                    pressure: point[2],
                })
                .collect();
            let produced = outline_points(
                &points,
                &OutlineOptions {
                    width_pt: case.width_pt,
                    pressure: case.pressure,
                    taper: case.taper,
                },
            );
            let produced: Vec<[f64; 2]> = produced.iter().map(|v| [v.x, v.y]).collect();
            assert_eq!(produced, case.expected, "case `{}` drifted", case.name);
        }
    }

    #[test]
    fn pressure_widens_the_silhouette() {
        let points = [
            OutlinePoint {
                x: 0.0,
                y: 0.0,
                pressure: 0.0,
            },
            OutlinePoint {
                x: 10.0,
                y: 0.0,
                pressure: 1.0,
            },
        ];
        let options = OutlineOptions {
            width_pt: 4.0,
            pressure: true,
            taper: 0.0,
        };
        let polygon = outline_points(&points, &options);
        // First point at zero pressure is the narrowest allowed; the last is the full width.
        assert!((polygon[0].y - 0.5).abs() < 1e-9, "{polygon:?}");
        assert!((polygon[1].y - 2.0).abs() < 1e-9, "{polygon:?}");
    }

    #[test]
    fn a_single_point_still_leaves_a_mark() {
        let polygon = outline_points(
            &[OutlinePoint {
                x: 5.0,
                y: 5.0,
                pressure: 1.0,
            }],
            &OutlineOptions {
                width_pt: 2.0,
                pressure: false,
                taper: 0.0,
            },
        );
        assert_eq!(polygon.len(), 4);
    }
}
