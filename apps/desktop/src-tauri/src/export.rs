use goodtype_core::{PageObject, storage};
use goodtype_typst::export::{
    ExportImage, ExportPage, ExportPoint, ExportStroke, ExportTransform, ExportTypstBlock,
    export_pages,
};

use crate::notebook::{NotebookHistories, with_notebook};
use crate::settings::RemotePackages;
use crate::workspace::{AllowedRoots, ensure_allowed};

/// Export the whole notebook as one ordered multi-page PDF built from the canonical files,
/// not from frontend state. The manifest defines the page order.
#[tauri::command]
pub async fn export_notebook_pdf(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    packages: tauri::State<'_, RemotePackages>,
    root: String,
    output_name: String,
    page_text_baseline_grid: bool,
) -> Result<String, String> {
    let root = ensure_allowed(&roots, &root)?;
    let histories = histories.inner().clone();
    // Match the on-screen preview: an export resolves packages under the same policy.
    let allow_remote_packages = packages.allowed();
    tauri::async_runtime::spawn_blocking(move || {
        with_notebook(&histories, || {
            let notebook_root = root.as_path();
            let manifest = storage::open_notebook(notebook_root)
                .map_err(|error| error.to_string())?
                .manifest;

            let mut pages = Vec::with_capacity(manifest.pages.len());
            for reference in &manifest.pages {
                let bundle = storage::open_page(notebook_root, &reference.id)
                    .map_err(|error| format!("page {}: {error}", reference.id))?;
                pages.push(export_page_from_bundle(&bundle, page_text_baseline_grid)?);
            }

            export_pages(notebook_root, &output_name, &pages, allow_remote_packages)
                .map_err(|error| error.to_string())
                .and_then(|result| crate::workspace::path_string(result.output_path))
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

fn export_page_from_bundle(
    bundle: &storage::NotebookSnapshot,
    page_text_baseline_grid: bool,
) -> Result<ExportPage, String> {
    let source_for = |path: &str| -> Result<String, String> {
        bundle
            .blocks
            .iter()
            .find(|file| file.path == path)
            .map(|file| String::from_utf8_lossy(&file.bytes).into_owned())
            .ok_or_else(|| format!("missing Typst source {path}"))
    };

    let mut blocks = Vec::new();
    let mut page_typst = None;
    let mut images = Vec::new();
    for (order, object) in bundle.page.objects.iter().enumerate() {
        match object {
            PageObject::Typst {
                fields,
                source_path,
                layout_width_pt,
                ..
            } => blocks.push(ExportTypstBlock {
                x: fields.x,
                y: fields.y,
                layout_width_pt: *layout_width_pt,
                scale: fields.scale,
                rotation_degrees: fields.rotation,
                z_index: fields.z_index,
                order,
                source: source_for(source_path)?,
            }),
            PageObject::PageTypst { source_path, .. } => {
                page_typst = Some(source_for(source_path)?);
            }
            PageObject::Image {
                fields,
                source_path,
                width_pt,
                height_pt,
                ..
            } => images.push(ExportImage {
                relative_path: source_path.clone(),
                x: fields.x,
                y: fields.y,
                width_pt: *width_pt,
                height_pt: *height_pt,
                scale: fields.scale,
                rotation_degrees: fields.rotation,
                z_index: fields.z_index,
                order,
            }),
            _ => {}
        }
    }

    let strokes = bundle
        .ink_layers
        .iter()
        .flat_map(|layer| &layer.strokes)
        .map(|stroke| ExportStroke {
            z_index: stroke.z_index,
            color: stroke.color.clone(),
            width_pt: stroke.width_pt,
            // Taken from the stroke, never re-derived from the tool: the nib that drew it already
            // resolved these, and re-deciding here is exactly how the PDF drifted from the screen.
            pressure: stroke.pressure,
            taper: stroke.taper,
            opacity: stroke.opacity,
            points: stroke
                .points
                .iter()
                .map(|point| ExportPoint {
                    x: point.x,
                    y: point.y,
                    pressure: point.pressure,
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
        .collect();

    Ok(ExportPage {
        width_pt: bundle.page.geometry.width_pt,
        height_pt: bundle.page.geometry.height_pt,
        shared_style: bundle
            .manifest
            .shared_style_path
            .as_deref()
            .map(source_for)
            .transpose()?,
        background: bundle.page.background.clone(),
        page_text_baseline_grid,
        page_typst,
        blocks,
        strokes,
        images,
    })
}
