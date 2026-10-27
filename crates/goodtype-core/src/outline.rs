//! Variable-width stroke geometry, shared by the live canvas and the PDF export.
//!
//! A stroke is not a stroked centreline: `stroke-width` is constant along a path, so a stroked
//! polyline can never vary with pressure. Instead this computes the ink's silhouette — offset the
//! centreline by half the local width on each side. Writing pens add rounded caps and outer
//! join arcs as consistently wound subpaths, filled once; highlighters retain their flat band.
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
    /// For a fragment left by the precise eraser: how much of the original stroke lies before and
    /// after it, in the same units as the points. `None` for a stroke drawn whole.
    pub trim: Option<OutlineTrim>,
}

/// Arc length of the original stroke cut away before and after a fragment (ADR 0037).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutlineTrim {
    pub start: f64,
    pub end: f64,
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
pub(crate) fn quantize(value: f64) -> f64 {
    (value * 1000.0 + 0.5).floor() / 1000.0
}

/// Distance between two samples, used to decide whether they are far enough apart to mean
/// anything.
fn span(a: OutlinePoint, b: OutlinePoint) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

pub(crate) fn width_at(point: &OutlinePoint, options: &OutlineOptions) -> f64 {
    if !options.pressure {
        return options.width_pt;
    }
    let clamped = point.pressure.clamp(0.0, 1.0);
    options.width_pt * (MIN_PRESSURE_SCALE + (1.0 - MIN_PRESSURE_SCALE) * clamped)
}

/// Round mark, with the same winding as a stroke silhouette.
pub(crate) fn outline_dot(center: OutlinePoint, radius: f64) -> Vec<Vertex> {
    (0..12)
        .map(|index| {
            let angle = -std::f64::consts::TAU * index as f64 / 12.0;
            Vertex {
                x: quantize(center.x + radius * angle.cos()),
                y: quantize(center.y + radius * angle.sin()),
            }
        })
        .collect()
}

/// Resolve filtered samples and their pressure/taper radii for both nib projections.
fn outline_samples(points: &[OutlinePoint], options: &OutlineOptions) -> Vec<(OutlinePoint, f64)> {
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

    // A fragment keeps its real geometry, however short: a full-width dot would be fatter than the
    // ink it stands for and could reach back into the area the eraser just cleared.
    if options.trim.is_none()
        && path
            .iter()
            .all(|point| span(*point, path[0]) <= options.width_pt / 2.0)
    {
        return vec![(path[0], options.width_pt / 2.0)];
    }

    let (before, after) = options
        .trim
        .map_or((0.0, 0.0), |trim| (trim.start, trim.end));

    // Two tapered endpoints have zero width; retain a middle section without a move sample. A cut
    // end keeps its width, so a fragment only needs this when both of its ends are original tips.
    if path.len() == 2 && options.taper > 0.0 && before == 0.0 && after == 0.0 {
        let (first, last) = (path[0], path[1]);
        path.insert(
            1,
            OutlinePoint {
                x: (first.x + last.x) / 2.0,
                y: (first.y + last.y) / 2.0,
                pressure: (first.pressure + last.pressure) / 2.0,
            },
        );
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
    // The taper belongs to the original stroke: a fragment measures it from the original ends.
    let taper_length = if options.taper > 0.0 {
        ((before + total + after) * options.taper).min(options.width_pt * TAPER_MAX_NIB_WIDTHS)
    } else {
        0.0
    };

    path.iter()
        .enumerate()
        .map(|(index, point)| {
            let ramp = if taper_length > 0.0 {
                1.0_f64
                    .min((before + cumulative[index]) / taper_length)
                    .min((after + total - cumulative[index]) / taper_length)
            } else {
                1.0
            };
            (*point, width_at(point, options) / 2.0 * ramp)
        })
        .collect()
}

pub fn outline_points(points: &[OutlinePoint], options: &OutlineOptions) -> Vec<Vertex> {
    let samples = outline_samples(points, options);
    if samples.is_empty() {
        return Vec::new();
    }
    if samples.len() == 1 {
        return outline_dot(samples[0].0, samples[0].1);
    }
    let path: Vec<OutlinePoint> = samples.iter().map(|(point, _)| *point).collect();
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

        let half = samples[index].1;

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

/// Round pen geometry: consistently wound strips and outside arcs, filled once as one path.
pub fn rounded_outline_points(
    points: &[OutlinePoint],
    options: &OutlineOptions,
) -> Vec<Vec<Vertex>> {
    use std::f64::consts::{FRAC_PI_2, PI};
    let samples = outline_samples(points, options);
    if samples.is_empty() {
        return Vec::new();
    }
    if samples.len() == 1 {
        return vec![outline_dot(samples[0].0, samples[0].1)];
    }
    let at = |index: usize, angle: f64| Vertex {
        x: quantize(samples[index].0.x + samples[index].1 * angle.cos()),
        y: quantize(samples[index].0.y + samples[index].1 * angle.sin()),
    };
    let mut polygons = Vec::new();
    let mut angles = Vec::new();
    for index in 0..samples.len() - 1 {
        let (from, to) = (samples[index].0, samples[index + 1].0);
        let angle = (to.y - from.y).atan2(to.x - from.x);
        angles.push(angle);
        polygons.push(vec![
            at(index, angle + FRAC_PI_2),
            at(index + 1, angle + FRAC_PI_2),
            at(index + 1, angle - FRAC_PI_2),
            at(index, angle - FRAC_PI_2),
        ]);
    }
    let mut arc = |index: usize, start: f64, sweep: f64| {
        if samples[index].1 <= 0.0 || sweep <= 0.0 {
            return;
        }
        let steps = (sweep / (PI / 6.0)).ceil().max(1.0) as usize;
        let mut polygon = vec![Vertex {
            x: quantize(samples[index].0.x),
            y: quantize(samples[index].0.y),
        }];
        polygon
            .extend((0..=steps).map(|step| at(index, start - sweep * step as f64 / steps as f64)));
        polygons.push(polygon);
    };
    arc(0, angles[0] - FRAC_PI_2, PI);
    arc(samples.len() - 1, angles[angles.len() - 1] + FRAC_PI_2, PI);
    for index in 1..samples.len() - 1 {
        let (before, after) = (angles[index - 1], angles[index]);
        let turn = (after - before).sin().atan2((after - before).cos());
        arc(
            index,
            if turn > 0.0 {
                after - FRAC_PI_2
            } else {
                before + FRAC_PI_2
            },
            turn.abs(),
        );
    }
    polygons
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
        #[serde(default)]
        trim: Option<[f64; 2]>,
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
                    trim: case.trim.map(|[start, end]| OutlineTrim { start, end }),
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
            trim: None,
        };
        let polygon = outline_points(&points, &options);
        // First point at zero pressure is the narrowest allowed; the last is the full width.
        assert!((polygon[0].y - 0.5).abs() < 1e-9, "{polygon:?}");
        assert!((polygon[1].y - 2.0).abs() < 1e-9, "{polygon:?}");
    }

    #[test]
    fn rounded_geometry_matches_the_shared_fixture() {
        #[derive(Deserialize)]
        struct RoundedCase {
            name: String,
            points: Vec<[f64; 3]>,
            width_pt: f64,
            pressure: bool,
            taper: f64,
            #[serde(default)]
            trim: Option<[f64; 2]>,
            expected: Vec<Vec<[f64; 2]>>,
        }
        let cases: Vec<RoundedCase> =
            serde_json::from_str(include_str!("../../../fixtures/ink/rounded-outline.json"))
                .unwrap();
        for case in cases {
            let points: Vec<OutlinePoint> = case
                .points
                .iter()
                .map(|p| OutlinePoint {
                    x: p[0],
                    y: p[1],
                    pressure: p[2],
                })
                .collect();
            let polygons = rounded_outline_points(
                &points,
                &OutlineOptions {
                    width_pt: case.width_pt,
                    pressure: case.pressure,
                    taper: case.taper,
                    trim: case.trim.map(|[start, end]| OutlineTrim { start, end }),
                },
            );
            let actual: Vec<Vec<[f64; 2]>> = polygons
                .iter()
                .map(|polygon| polygon.iter().map(|p| [p.x, p.y]).collect())
                .collect();
            assert_eq!(actual, case.expected, "{}", case.name);
        }
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
                trim: None,
            },
        );
        assert_eq!(polygon.len(), 12);
    }
}
