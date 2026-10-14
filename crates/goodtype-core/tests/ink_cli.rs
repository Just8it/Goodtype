use std::{fs, process::Command};

use goodtype_core::{InkLayer, Stroke, StrokePattern, StrokePoint, StrokeTool, Transform};

fn layer() -> InkLayer {
    InkLayer {
        schema_version: goodtype_core::SCHEMA_VERSION,
        id: "ink-layer-001".into(),
        page_id: "page-001".into(),
        strokes: vec![Stroke {
            id: "stroke-001".into(),
            z_index: goodtype_core::DEFAULT_INK_Z_INDEX,
            tool: StrokeTool::Pen,
            color: "#101418".into(),
            width_pt: 1.6,
            pattern: StrokePattern::Dashed,
            pattern_spacing: 1.0,
            pressure: true,
            taper: 0.12,
            opacity: 1.0,
            group_id: None,
            points: vec![StrokePoint {
                x: -0.0,
                y: 842.0,
                pressure: 0.625,
                time_ms: 12.5,
                tilt_x: -17.0,
                tilt_y: 9.0,
            }],
            transform: Transform {
                translate_x: 0.0,
                translate_y: 0.0,
                scale_x: 1.0,
                scale_y: 1.0,
                rotation: 0.0,
            },
        }],
    }
}

#[test]
fn decode_prints_ordinary_pretty_ink_json() {
    let temporary = tempfile::tempdir().unwrap();
    let input = temporary.path().join("layer.ink");
    fs::write(&input, goodtype_core::ink_codec::encode(&layer()).unwrap()).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_goodtype-ink"))
        .arg("decode")
        .arg(&input)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert!(String::from_utf8_lossy(&output.stdout).contains("\n  \"schemaVersion\": "));
    let decoded: InkLayer = serde_json::from_slice(&output.stdout).unwrap();
    let point = &decoded.strokes[0].points[0];
    assert_eq!(point.x.to_bits(), (-0.0_f64).to_bits());
    assert_eq!(
        serde_json::to_value(decoded).unwrap(),
        serde_json::to_value(layer()).unwrap()
    );
}

#[test]
fn malformed_input_fails_without_writing_json() {
    let temporary = tempfile::tempdir().unwrap();
    let input = temporary.path().join("broken.ink");
    fs::write(&input, b"not Goodtype ink").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_goodtype-ink"))
        .arg("decode")
        .arg(&input)
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("goodtype-ink:"));
}

#[test]
fn missing_arguments_fail_with_usage() {
    let output = Command::new(env!("CARGO_BIN_EXE_goodtype-ink"))
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("usage: goodtype-ink decode INPUT"));
}
