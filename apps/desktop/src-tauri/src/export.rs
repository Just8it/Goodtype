use goodtype_core::{PageBackground, PageObject, StrokeTool, storage};
use goodtype_typst::export::{
    ExportImage, ExportPage, ExportPoint, ExportShape, ExportStroke, ExportTransform,
    ExportTypstBlock, export_pages, render_page_pdf,
};

use crate::notebook::{NotebookHistories, with_notebook};
use crate::settings::RemotePackages;
use crate::workspace::{AllowedRoots, ensure_allowed};

/// Export the notebook, or the pages named in `page_ids`, as one ordered multi-page PDF built
/// from the canonical files, not from frontend state. The manifest defines the page order; the
/// selection only filters it, and naming a page the manifest does not have is an error rather
/// than a silently shorter PDF.
#[tauri::command]
#[allow(clippy::too_many_arguments)] // Tauri injects state separately from export options.
pub async fn export_notebook_pdf(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    packages: tauri::State<'_, RemotePackages>,
    root: String,
    output_name: String,
    page_text_baseline_grid: bool,
    split_calculations_a4: Option<bool>,
    ui_language: Option<String>,
    page_ids: Option<Vec<String>>,
    include: Option<ExportInclude>,
) -> Result<String, String> {
    let include = include.unwrap_or_default();
    let root = ensure_allowed(&roots, &root)?;
    let histories = histories.inner().clone();
    // Match the on-screen preview: an export resolves packages under the same policy.
    let allow_remote_packages = packages.allowed();
    tauri::async_runtime::spawn_blocking(move || {
        // Take one coherent canonical snapshot while saves are excluded, then release the
        // storage lock before the potentially expensive Typst/PDF compilation.
        let pages = with_notebook(&histories, || {
            let notebook_root = root.as_path();
            let manifest = storage::open_notebook(notebook_root)
                .map_err(|error| error.to_string())?
                .manifest;

            let references = selected_pages(&manifest.pages, page_ids.as_deref())?;
            let mut pages = Vec::with_capacity(references.len());
            for reference in references {
                let bundle = storage::open_page(notebook_root, &reference.id)
                    .map_err(|error| format!("page {}: {error}", reference.id))?;
                let mut page = export_page_from_bundle(&bundle, page_text_baseline_grid)?;
                if page.calculation.is_some() && !include.calculations {
                    continue;
                }
                include.apply(&mut page);
                if let Some(calculation) = &mut page.calculation {
                    calculation.split_a4 = split_calculations_a4.unwrap_or(false);
                    calculation.german = ui_language.as_deref() == Some("de");
                }
                pages.push(page);
            }

            if pages.is_empty() {
                return Err("nothing to export: every selected page is a Jupyter page and Jupyter pages are switched off".to_owned());
            }
            Ok(pages)
        })?;
        export_pages(root.as_path(), &output_name, &pages, allow_remote_packages)
            .map_err(|error| error.to_string())
            .and_then(|result| crate::workspace::path_string(result.output_path))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Which kinds of content an export keeps. Everything is on unless the writer switches it off;
/// switching something off leaves the notebook untouched and only thins this one PDF.
#[derive(Clone, Copy, Debug, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct ExportInclude {
    pub handwriting: bool,
    pub highlighter: bool,
    pub typst: bool,
    pub page_text: bool,
    pub images: bool,
    pub shapes: bool,
    /// Paper templates, paper colour and imported PDF pages. Off exports on plain white.
    pub background: bool,
    pub calculations: bool,
}

impl Default for ExportInclude {
    fn default() -> Self {
        Self {
            handwriting: true,
            highlighter: true,
            typst: true,
            page_text: true,
            images: true,
            shapes: true,
            background: true,
            calculations: true,
        }
    }
}

impl ExportInclude {
    fn apply(self, page: &mut goodtype_typst::export::ExportPage) {
        page.strokes.retain(|stroke| match stroke.tool {
            StrokeTool::Pen => self.handwriting,
            StrokeTool::Highlighter => self.highlighter,
        });
        if !self.typst {
            page.blocks.clear();
        }
        if !self.page_text {
            page.page_typst = None;
        }
        if !self.images {
            page.images.clear();
        }
        if !self.shapes {
            page.shapes.clear();
        }
        if !self.background {
            page.background = PageBackground::Plain {
                color: "#ffffff".to_owned(),
            };
        }
    }
}

/// The manifest's pages in manifest order, kept to `wanted` when a selection is given.
fn selected_pages<'a>(
    pages: &'a [goodtype_core::PageReference],
    wanted: Option<&[String]>,
) -> Result<Vec<&'a goodtype_core::PageReference>, String> {
    let Some(wanted) = wanted else {
        return Ok(pages.iter().collect());
    };
    if let Some(unknown) = wanted
        .iter()
        .find(|id| !pages.iter().any(|page| &page.id == *id))
    {
        return Err(format!("page {unknown} is not in this notebook"));
    }
    Ok(pages
        .iter()
        .filter(|page| wanted.contains(&page.id))
        .collect())
}

/// A disposable first-page preview uses the exact same material/order as ordinary export.
#[tauri::command]
pub async fn render_notebook_cover(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    page_text_baseline_grid: bool,
) -> Result<tauri::ipc::Response, String> {
    let root = ensure_allowed(&roots, &root)?;
    let histories = histories.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let page = with_notebook(&histories, || {
            let bundle = storage::open_notebook(&root).map_err(|error| error.to_string())?;
            export_page_from_bundle(&bundle, page_text_baseline_grid)
        })?;
        let bytes = render_page_pdf(&root, &page).map_err(|error| error.to_string())?;
        if bytes.len() > 64 * 1024 * 1024 {
            return Err("notebook cover exceeds the 64 MiB rendering limit".to_owned());
        }
        Ok(tauri::ipc::Response::new(bytes))
    })
    .await
    .map_err(|error| error.to_string())?
}

pub(crate) fn export_page_from_bundle(
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
    let mut shapes = Vec::new();
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
            PageObject::Shape {
                fields,
                geometry,
                style,
            } => shapes.push(ExportShape {
                x: fields.x,
                y: fields.y,
                rotation_degrees: fields.rotation,
                scale: fields.scale,
                z_index: fields.z_index,
                order,
                geometry: geometry.clone(),
                style: style.clone(),
            }),
            PageObject::PdfMaterial { fields, .. } => {
                return Err(format!(
                    "PDF object {} cannot be exported yet; no PDF was written",
                    fields.id
                ));
            }
            PageObject::InkGroup { .. } | PageObject::Group { .. } => {}
        }
    }

    let strokes = bundle
        .ink_layers
        .iter()
        .flat_map(|layer| &layer.strokes)
        .map(|stroke| ExportStroke {
            tool: stroke.tool.clone(),
            z_index: stroke.z_index,
            color: stroke.color.clone(),
            width_pt: stroke.width_pt,
            pattern: stroke.pattern,
            pattern_spacing: stroke.pattern_spacing,
            // Taken from the stroke, never re-derived from the tool: the nib that drew it already
            // resolved these, and re-deciding here is exactly how the PDF drifted from the screen.
            pressure: stroke.pressure,
            taper: stroke.taper,
            opacity: stroke.opacity,
            trim: stroke.trim,
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

    let calculation = bundle
        .page
        .calculation
        .as_ref()
        .map(|reference| {
            let bytes = bundle
                .calculations
                .iter()
                .find(|file| file.path == reference.source_path)
                .ok_or_else(|| "calculation source is missing".to_owned())?;
            let document = goodtype_core::calculation::Calculation::parse(&bytes.bytes)
                .map_err(|error| error.to_string())?;
            Ok::<_, String>(goodtype_typst::calculation::ExportCalculation {
                document,
                part_id: reference.part_id.clone(),
                german: false,
                split_a4: false,
            })
        })
        .transpose()?;
    Ok(ExportPage {
        calculation,
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
        shapes,
        images,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_pdf_objects_cannot_silently_disappear_from_export() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../fixtures/notebooks/minimal");
        let bundle = storage::open_notebook(&root).unwrap();
        let error = export_page_from_bundle(&bundle, true).unwrap_err();
        assert!(error.contains("PDF object pdf-001 cannot be exported yet"));
    }

    #[test]
    fn a_page_selection_keeps_manifest_order_and_rejects_unknown_pages() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../fixtures/notebooks/minimal");
        let mut pages = storage::open_notebook(&root).unwrap().manifest.pages;
        let mut second = pages[0].clone();
        second.id = "page-second".to_owned();
        pages.push(second);
        let ids = |selected: Vec<&goodtype_core::PageReference>| {
            selected
                .iter()
                .map(|page| page.id.clone())
                .collect::<Vec<_>>()
        };

        assert_eq!(selected_pages(&pages, None).unwrap().len(), 2);
        let reversed = vec!["page-second".to_owned(), pages[0].id.clone()];
        assert_eq!(
            ids(selected_pages(&pages, Some(&reversed)).unwrap()),
            vec![pages[0].id.clone(), "page-second".to_owned()]
        );
        assert!(selected_pages(&pages, Some(&["missing".to_owned()])).is_err());
    }

    #[test]
    fn switched_off_content_is_left_out_of_the_export_page() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../fixtures/notebooks/minimal");
        let mut bundle = storage::open_notebook(&root).unwrap();
        bundle
            .page
            .objects
            .retain(|object| !matches!(object, PageObject::PdfMaterial { .. }));
        let full = export_page_from_bundle(&bundle, false).unwrap();
        let mut page = full.clone();
        ExportInclude {
            typst: false,
            images: false,
            background: false,
            ..ExportInclude::default()
        }
        .apply(&mut page);
        assert!(page.blocks.is_empty());
        assert!(page.images.is_empty());
        assert!(
            matches!(page.background, PageBackground::Plain { ref color } if color == "#ffffff")
        );
        assert_eq!(page.strokes.len(), full.strokes.len());

        let mut untouched = full.clone();
        ExportInclude::default().apply(&mut untouched);
        assert_eq!(untouched, full);
    }
}
