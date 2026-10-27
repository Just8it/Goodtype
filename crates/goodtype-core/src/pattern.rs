//! Filled geometry for solid, dashed, and dotted ink.
//!
//! Pattern spacing follows the centreline in page points, then every visible piece becomes a
//! filled silhouette. This keeps pressure-sensitive ink and PDF export on the same geometry.
//! The frontend mirror lives in `apps/desktop/src/lib/ink/pattern.ts`.

use crate::{
    StrokePattern,
    outline::{
        OutlineOptions, OutlinePoint, Vertex, outline_dot, outline_points, rounded_outline_points,
        width_at,
    },
};

#[derive(Clone, Copy)]
struct MeasuredPoint {
    point: OutlinePoint,
    distance: f64,
}

fn measured(points: &[OutlinePoint]) -> Vec<MeasuredPoint> {
    let Some(first) = points.first().copied() else {
        return Vec::new();
    };
    let mut path = vec![MeasuredPoint {
        point: first,
        distance: 0.0,
    }];
    for point in &points[1..] {
        let previous = path[path.len() - 1];
        let distance = (point.x - previous.point.x).hypot(point.y - previous.point.y);
        if distance > 0.0 {
            path.push(MeasuredPoint {
                point: *point,
                distance: previous.distance + distance,
            });
        }
    }
    path
}

fn interpolate(from: MeasuredPoint, to: MeasuredPoint, distance: f64) -> OutlinePoint {
    let span = to.distance - from.distance;
    let amount = if span > 0.0 {
        ((distance - from.distance) / span).clamp(0.0, 1.0)
    } else {
        0.0
    };
    OutlinePoint {
        x: from.point.x + (to.point.x - from.point.x) * amount,
        y: from.point.y + (to.point.y - from.point.y) * amount,
        pressure: from.point.pressure + (to.point.pressure - from.point.pressure) * amount,
    }
}

fn point_at(path: &[MeasuredPoint], distance: f64, segment: &mut usize) -> OutlinePoint {
    while *segment + 1 < path.len() && path[*segment + 1].distance < distance {
        *segment += 1;
    }
    path.get(*segment + 1)
        .map(|next| interpolate(path[*segment], *next, distance))
        .unwrap_or(path[path.len() - 1].point)
}

/// Bounds of the factor applied to the gap between marks; mirrors `ink/pattern.ts`.
pub const MIN_PATTERN_SPACING: f64 = 0.5;
pub const MAX_PATTERN_SPACING: f64 = 3.0;

fn dashed_runs(
    path: &[MeasuredPoint],
    width_pt: f64,
    spacing: f64,
    offset: f64,
) -> Vec<Vec<OutlinePoint>> {
    let total = path.last().map(|point| point.distance).unwrap_or(0.0);
    let dash = (width_pt * 4.0).max(6.0);
    let gap = (width_pt * 2.5).max(3.5) * spacing;
    let mut runs = Vec::new();
    // A fragment continues its original's dashes, so the first one may already be under way.
    let mut start = if offset > 0.0 {
        -(offset % (dash + gap))
    } else {
        0.0
    };
    let mut segment = 0;
    while start < total {
        let end = (start + dash).min(total);
        if end <= 0.0 {
            start += dash + gap;
            continue;
        }
        let mut run = vec![point_at(path, start.max(0.0), &mut segment)];
        let mut inside = segment + 1;
        while inside < path.len() && path[inside].distance < end {
            run.push(path[inside].point);
            inside += 1;
        }
        let last = point_at(path, end, &mut segment);
        if run.last() != Some(&last) {
            run.push(last);
        }
        runs.push(run);
        start += dash + gap;
    }
    runs
}

fn dotted_runs(
    path: &[MeasuredPoint],
    width_pt: f64,
    factor: f64,
    offset: f64,
) -> Vec<Vec<OutlinePoint>> {
    let total = path.last().map(|point| point.distance).unwrap_or(0.0);
    let mut segment = 0;
    let spacing = (width_pt * 2.75).max(4.0) * factor;
    // A fragment's first dot is the next one its original would have drawn.
    let mut distance = if offset > 0.0 {
        (spacing - offset % spacing) % spacing
    } else {
        0.0
    };
    let mut runs = Vec::new();

    // Keep the phase fixed at the stroke origin. Redistributing dots across the
    // current length makes every existing dot drift whenever a sample arrives.
    while distance <= total {
        runs.push(vec![point_at(path, distance, &mut segment)]);
        distance += spacing;
    }

    runs
}

/// Centreline runs for a stroke pattern. Public so screen/export parity can be fixture-tested.
/// `offset` is how far along its original stroke a fragment begins; the phase continues from it.
pub fn pattern_runs(
    points: &[OutlinePoint],
    width_pt: f64,
    pattern: StrokePattern,
    spacing: f64,
    offset: f64,
) -> Vec<Vec<OutlinePoint>> {
    if points.is_empty() {
        return Vec::new();
    }
    if pattern == StrokePattern::Solid {
        return vec![points.to_vec()];
    }
    let path = measured(points);
    if path.len() == 1 {
        return vec![vec![path[0].point]];
    }
    match pattern {
        StrokePattern::Dashed => dashed_runs(&path, width_pt, spacing, offset),
        StrokePattern::Dotted => dotted_runs(&path, width_pt, spacing, offset),
        StrokePattern::Solid => unreachable!(),
    }
}

/// Fillable polygons for the complete patterned stroke.
pub fn patterned_outline_points(
    points: &[OutlinePoint],
    options: &OutlineOptions,
    pattern: StrokePattern,
    spacing: f64,
    round: bool,
) -> Vec<Vec<Vertex>> {
    let offset = options.trim.map_or(0.0, |trim| trim.start);
    let runs = pattern_runs(points, options.width_pt, pattern, spacing, offset);
    if pattern == StrokePattern::Solid {
        return runs
            .iter()
            .flat_map(|run| {
                if round {
                    rounded_outline_points(run, options)
                } else {
                    vec![outline_points(run, options)]
                }
            })
            .filter(|polygon| !polygon.is_empty())
            .collect();
    }
    if pattern == StrokePattern::Dotted {
        return runs
            .iter()
            .map(|run| outline_dot(run[0], width_at(&run[0], options) / 2.0))
            .collect();
    }

    let segment_options = OutlineOptions {
        taper: 0.0,
        trim: None,
        ..*options
    };
    runs.iter()
        .flat_map(|run| {
            let first = run[0];
            let last = run[run.len() - 1];
            if run.len() == 1 {
                return vec![outline_dot(first, width_at(&first, options) / 2.0)];
            }
            let mut polygons = if round {
                rounded_outline_points(run, &segment_options)
            } else {
                vec![outline_points(run, &segment_options)]
            };
            polygons.push(outline_dot(first, width_at(&first, options) / 2.0));
            polygons.push(outline_dot(last, width_at(&last, options) / 2.0));
            polygons
        })
        .filter(|polygon| !polygon.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line() -> Vec<OutlinePoint> {
        vec![
            OutlinePoint {
                x: 0.0,
                y: 0.0,
                pressure: 1.0,
            },
            OutlinePoint {
                x: 20.0,
                y: 0.0,
                pressure: 1.0,
            },
        ]
    }

    #[test]
    fn dashed_runs_follow_page_distance() {
        let runs = pattern_runs(&line(), 2.0, StrokePattern::Dashed, 1.0, 0.0);
        let ends: Vec<[f64; 2]> = runs
            .iter()
            .map(|run| [run[0].x, run.last().unwrap().x])
            .collect();
        assert_eq!(ends, [[0.0, 8.0], [13.0, 20.0]]);
    }

    #[test]
    fn dotted_runs_are_anchored_at_the_stroke_origin() {
        let runs = pattern_runs(&line(), 2.0, StrokePattern::Dotted, 1.0, 0.0);
        let centers: Vec<f64> = runs.iter().map(|run| run[0].x).collect();
        assert_eq!(centers, [0.0, 5.5, 11.0, 16.5]);
    }

    #[test]
    fn spacing_scales_only_the_gaps_like_the_editor() {
        let dots = pattern_runs(&line(), 2.0, StrokePattern::Dotted, 2.0, 0.0);
        let centers: Vec<f64> = dots.iter().map(|run| run[0].x).collect();
        assert_eq!(centers, [0.0, 11.0]);
        let dashes = pattern_runs(&line(), 2.0, StrokePattern::Dashed, 2.0, 0.0);
        assert_eq!([dashes[0].last().unwrap().x, dashes[1][0].x], [8.0, 18.0]);
    }

    #[test]
    fn existing_pattern_marks_do_not_move_when_the_stroke_grows() {
        let longer_line = vec![
            OutlinePoint {
                x: 0.0,
                y: 0.0,
                pressure: 1.0,
            },
            OutlinePoint {
                x: 20.0,
                y: 0.0,
                pressure: 1.0,
            },
            OutlinePoint {
                x: 40.0,
                y: 0.0,
                pressure: 1.0,
            },
        ];
        let dots_before = pattern_runs(&line(), 2.0, StrokePattern::Dotted, 1.0, 0.0);
        let dots_after = pattern_runs(&longer_line, 2.0, StrokePattern::Dotted, 1.0, 0.0);
        assert_eq!(&dots_after[..dots_before.len()], dots_before.as_slice());

        let dash_prefix = vec![
            OutlinePoint {
                x: 0.0,
                y: 0.0,
                pressure: 1.0,
            },
            OutlinePoint {
                x: 20.0,
                y: 0.0,
                pressure: 1.0,
            },
            OutlinePoint {
                x: 24.0,
                y: 0.0,
                pressure: 1.0,
            },
        ];
        let completed_dashes = pattern_runs(&dash_prefix, 2.0, StrokePattern::Dashed, 1.0, 0.0);
        let mut extended_dash = dash_prefix.clone();
        extended_dash.push(OutlinePoint {
            x: 40.0,
            y: 0.0,
            pressure: 1.0,
        });
        let extended_dashes = pattern_runs(&extended_dash, 2.0, StrokePattern::Dashed, 1.0, 0.0);
        assert_eq!(
            &extended_dashes[..completed_dashes.len()],
            completed_dashes.as_slice()
        );
    }

    #[test]
    fn dash_bodies_and_caps_are_additive_in_a_compound_svg_path() {
        let polygons = patterned_outline_points(
            &line(),
            &OutlineOptions {
                width_pt: 2.0,
                pressure: false,
                taper: 0.0,
                trim: None,
            },
            StrokePattern::Dashed,
            1.0,
            false,
        );
        assert_eq!(polygons.len(), 6);
        assert!(polygons.iter().all(|polygon| {
            polygon
                .iter()
                .enumerate()
                .map(|(index, point)| {
                    let next = polygon[(index + 1) % polygon.len()];
                    point.x * next.y - next.x * point.y
                })
                .sum::<f64>()
                < 0.0
        }));

        let tap = patterned_outline_points(
            &[OutlinePoint {
                x: 4.0,
                y: 8.0,
                pressure: 1.0,
            }],
            &OutlineOptions {
                width_pt: 2.0,
                pressure: false,
                taper: 0.0,
                trim: None,
            },
            StrokePattern::Dashed,
            1.0,
            false,
        );
        assert_eq!(tap.len(), 1);
    }

    /// The precise eraser's promise: what is left of a patterned stroke keeps every mark where it
    /// was, because the fragment continues its original's phase instead of starting a new one.
    #[test]
    fn a_fragment_keeps_its_original_marks_in_place() {
        let from = |x: f64| {
            vec![
                OutlinePoint {
                    x,
                    y: 0.0,
                    pressure: 1.0,
                },
                OutlinePoint {
                    x: 60.0,
                    y: 0.0,
                    pressure: 1.0,
                },
            ]
        };
        let rounded = |value: f64| (value * 1e9).round() / 1e9;
        for pattern in [StrokePattern::Dashed, StrokePattern::Dotted] {
            for cut in [3.0, 9.5, 14.0, 27.25] {
                let whole = pattern_runs(&from(0.0), 2.0, pattern, 1.0, 0.0);
                let fragment = pattern_runs(&from(cut), 2.0, pattern, 1.0, cut);
                let expected: Vec<f64> = whole
                    .iter()
                    .filter(|run| run.last().unwrap().x > cut || run[0].x >= cut)
                    .map(|run| rounded(run[0].x.max(cut)))
                    .collect();
                let actual: Vec<f64> = fragment.iter().map(|run| rounded(run[0].x)).collect();
                assert_eq!(actual, expected, "{pattern:?} cut at {cut}");
            }
        }
    }
}
