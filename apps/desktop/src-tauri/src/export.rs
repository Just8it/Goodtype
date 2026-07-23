use std::path::Path;

use goodtype_core::Stroke;
use goodtype_typst::export::{
    ExportImage, ExportPage, ExportPoint, ExportStroke, ExportTransform, ExportTypstBlock,
    export_page,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportPageRequest {
    width_pt: f64,
    height_pt: f64,
    blocks: Vec<ExportBlockRequest>,
    strokes: Vec<Stroke>,
    images: Vec<ExportImageRequest>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportBlockRequest {
    x: f64,
    y: f64,
    layout_width_pt: f64,
    scale: f64,
    source: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportImageRequest {
    relative_path: String,
    x: f64,
    y: f64,
    width_pt: f64,
    height_pt: f64,
    scale: f64,
}

#[tauri::command]
pub async fn export_pdf(
    root: String,
    output_name: String,
    page: ExportPageRequest,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let page = ExportPage {
            width_pt: page.width_pt,
            height_pt: page.height_pt,
            blocks: page
                .blocks
                .into_iter()
                .map(|block| ExportTypstBlock {
                    x: block.x,
                    y: block.y,
                    layout_width_pt: block.layout_width_pt,
                    scale: block.scale,
                    source: block.source,
                })
                .collect(),
            strokes: page
                .strokes
                .into_iter()
                .map(|stroke| ExportStroke {
                    color: stroke.color,
                    width_pt: stroke.width_pt,
                    points: stroke
                        .points
                        .into_iter()
                        .map(|point| ExportPoint {
                            x: point.x,
                            y: point.y,
                        })
                        .collect(),
                    transform: ExportTransform {
                        translate_x: stroke.transform.translate_x,
                        translate_y: stroke.transform.translate_y,
                        scale_x: stroke.transform.scale_x,
                        scale_y: stroke.transform.scale_y,
                        rotation_degrees: stroke.transform.rotation,
                    },
                })
                .collect(),
            images: page
                .images
                .into_iter()
                .map(|image| ExportImage {
                    relative_path: image.relative_path,
                    x: image.x,
                    y: image.y,
                    width_pt: image.width_pt,
                    height_pt: image.height_pt,
                    scale: image.scale,
                })
                .collect(),
        };
        export_page(Path::new(&root), &output_name, &page)
            .map_err(|error| error.to_string())
            .and_then(|result| {
                result
                    .output_path
                    .into_os_string()
                    .into_string()
                    .map_err(|_| "the PDF path is not valid Unicode".to_owned())
            })
    })
    .await
    .map_err(|error| error.to_string())?
}
