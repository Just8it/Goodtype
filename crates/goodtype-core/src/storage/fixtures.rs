//! Notebook fixtures shared by the storage tests.
//!
//! One notebook that exercises every kind of object the store can hold, so a test asserting on
//! one concern still fails if another regresses underneath it.

use crate::{
    InkLayer, InkLayerReference, NotebookManifest, ObjectFields, Page, PageBackground,
    PageDefaults, PageGeometry, PageObject, PageReference, SCHEMA_VERSION, Stroke, StrokePoint,
    StrokeTool, Transform,
};

use super::{NotebookSnapshot, StoredFile};

pub(super) const IMAGE_BYTES: &[u8] =
    include_bytes!("../../../../fixtures/notebooks/phase0b/original-image.bin");

pub(super) fn fields(id: &str, reading_order: u32) -> ObjectFields {
    ObjectFields {
        id: id.into(),
        x: 72.0,
        y: 96.0,
        rotation: 0.0,
        scale: 1.0,
        z_index: reading_order as i32,
        reading_order,
        group_id: None,
        created_at: "2026-07-23T00:00:00Z".into(),
        modified_at: "2026-07-23T00:00:00Z".into(),
    }
}

pub(super) fn snapshot() -> NotebookSnapshot {
    NotebookSnapshot {
        manifest: NotebookManifest {
            schema_version: SCHEMA_VERSION,
            id: "notebook-phase0b".into(),
            title: "Phase 0B persistence".into(),
            pages: vec![PageReference {
                id: "page-001".into(),
                path: "pages/page-001.json".into(),
                geometry: PageGeometry {
                    width_pt: 595.0,
                    height_pt: 842.0,
                },
            }],
            default_page: PageDefaults {
                geometry: PageGeometry {
                    width_pt: 595.0,
                    height_pt: 842.0,
                },
                background: PageBackground::Plain {
                    color: "#ffffff".into(),
                },
            },
            shared_style_path: None,
            created_at: "2026-07-23T00:00:00Z".into(),
            modified_at: "2026-07-23T00:00:00Z".into(),
        },
        page: Page {
            schema_version: SCHEMA_VERSION,
            id: "page-001".into(),
            revision: 1,
            geometry: PageGeometry {
                width_pt: 595.0,
                height_pt: 842.0,
            },
            background: PageBackground::Plain {
                color: "#ffffff".into(),
            },
            objects: vec![
                PageObject::Typst {
                    fields: fields("typst-001", 0),
                    source_path: "blocks/equation.typ".into(),
                    layout_width_pt: 220.0,
                    measured_width_pt: 180.0,
                    measured_height_pt: 32.0,
                },
                PageObject::Image {
                    fields: fields("image-001", 1),
                    source_path: "assets/diagram.png".into(),
                    width_pt: 120.0,
                    height_pt: 80.0,
                    alt_text: "Original pasted diagram".into(),
                },
            ],
            reading_order: vec!["typst-001".into(), "image-001".into()],
            ink_layers: vec![InkLayerReference {
                id: "ink-layer-001".into(),
                path: "ink/page-001-layer-001.json".into(),
            }],
        },
        blocks: vec![StoredFile {
            path: "blocks/equation.typ".into(),
            bytes: b"$ F = m a $".to_vec(),
        }],
        assets: vec![StoredFile {
            path: "assets/diagram.png".into(),
            bytes: IMAGE_BYTES.to_vec(),
        }],
        ink_layers: vec![InkLayer {
            schema_version: SCHEMA_VERSION,
            id: "ink-layer-001".into(),
            page_id: "page-001".into(),
            strokes: vec![Stroke {
                id: "stroke-001".into(),
                z_index: crate::DEFAULT_INK_Z_INDEX,
                tool: StrokeTool::Pen,
                color: "#111111".into(),
                width_pt: 2.0,
                pressure: true,
                taper: 0.0,
                opacity: 1.0,
                group_id: None,
                points: vec![StrokePoint {
                    x: 80.0,
                    y: 140.0,
                    pressure: 0.75,
                    time_ms: 0.0,
                    tilt_x: 0.0,
                    tilt_y: 0.0,
                }],
                transform: Transform {
                    translate_x: 0.0,
                    translate_y: 0.0,
                    scale_x: 1.0,
                    scale_y: 1.0,
                    rotation: 0.0,
                },
            }],
        }],
    }
}

/// Samples shaped like the frontend writes them: quantized to the precision in
/// `apps/desktop/src/lib/ink/pipeline.ts`, so the size this test measures is the
/// size a real notebook reaches.
pub(super) fn handwriting_layer(strokes: usize, points_per_stroke: usize) -> InkLayer {
    InkLayer {
        schema_version: SCHEMA_VERSION,
        id: "ink-layer-001".into(),
        page_id: "page-001".into(),
        strokes: (0..strokes)
            .map(|stroke| Stroke {
                id: format!("stroke-{stroke:06}"),
                z_index: crate::DEFAULT_INK_Z_INDEX,
                tool: StrokeTool::Pen,
                color: "#101418".into(),
                width_pt: 1.6,
                pressure: true,
                taper: 0.0,
                opacity: 1.0,
                group_id: None,
                points: (0..points_per_stroke)
                    .map(|point| {
                        let step = (stroke * points_per_stroke + point) as f64;
                        StrokePoint {
                            x: ((step * 7.31) % 59500.0).round() / 100.0,
                            y: ((step * 11.17) % 84200.0).round() / 100.0,
                            pressure: ((step % 1000.0) / 1000.0 * 1000.0).round() / 1000.0,
                            time_ms: (step * 83.0).round() / 10.0,
                            tilt_x: ((step % 900.0) / 10.0 - 45.0).round(),
                            tilt_y: ((step % 700.0) / 10.0 - 35.0).round(),
                        }
                    })
                    .collect(),
                transform: Transform {
                    translate_x: 0.0,
                    translate_y: 0.0,
                    scale_x: 1.0,
                    scale_y: 1.0,
                    rotation: 0.0,
                },
            })
            .collect(),
    }
}
