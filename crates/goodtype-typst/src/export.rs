use std::path::{Path, PathBuf};

use goodtype_core::{
    PageBackground, PageGeometry,
    outline::{OutlineOptions, OutlinePoint, outline_points},
    template::{TemplateShape, resolve},
};

const MAX_SOURCE_BYTES: usize = 1024 * 1024;
const MAX_PAGE_ITEMS: usize = 10_000;
const MAX_STROKE_POINTS: usize = 1_000_000;
const MAX_DIMENSION_PT: f64 = 100_000.0;

#[derive(Debug, Clone, PartialEq)]
pub struct ExportPage {
    pub width_pt: f64,
    pub height_pt: f64,
    /// The paper. Carried through rather than assumed white, so a page written on ruled paper
    /// exports as ruled paper — the template is part of the page, not a screen decoration.
    pub background: PageBackground,
    pub blocks: Vec<ExportTypstBlock>,
    pub strokes: Vec<ExportStroke>,
    pub images: Vec<ExportImage>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExportTypstBlock {
    pub x: f64,
    pub y: f64,
    pub layout_width_pt: f64,
    pub scale: f64,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExportStroke {
    pub color: String,
    pub width_pt: f64,
    /// Whether the nib varied its width with pressure.
    pub pressure: bool,
    /// Fraction of the stroke's length over which each end tapers to a point; 0 disables it.
    pub taper: f64,
    /// Ink opacity, 0–1. A highlighter sweep is translucent; leaving it out is what made a
    /// highlighter come out solid in the PDF while it was see-through on screen.
    pub opacity: f64,
    pub points: Vec<ExportPoint>,
    pub transform: ExportTransform,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExportPoint {
    pub x: f64,
    pub y: f64,
    /// Normalised stylus force at this sample. Carried through so the exported PDF shows the
    /// same variable-width ink as the screen.
    pub pressure: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExportTransform {
    pub translate_x: f64,
    pub translate_y: f64,
    pub scale_x: f64,
    pub scale_y: f64,
    pub rotation_degrees: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExportImage {
    pub relative_path: String,
    pub x: f64,
    pub y: f64,
    pub width_pt: f64,
    pub height_pt: f64,
    pub scale: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportResult {
    pub output_path: PathBuf,
}

#[derive(Debug)]
pub enum ExportError {
    InvalidRoot(PathBuf),
    InvalidOutputName(String),
    InvalidPage(String),
    InvalidImagePath(String),
    CompilerFailed(String),
    Io(std::io::Error),
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRoot(path) => {
                write!(
                    formatter,
                    "export root is not a directory: {}",
                    path.display()
                )
            }
            Self::InvalidOutputName(name) => write!(formatter, "unsafe PDF output name: {name}"),
            Self::InvalidPage(message) => write!(formatter, "invalid page export: {message}"),
            Self::InvalidImagePath(path) => write!(formatter, "unsafe image path: {path}"),
            Self::CompilerFailed(message) => write!(formatter, "Typst export failed: {message}"),
            Self::Io(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ExportError {}

impl From<std::io::Error> for ExportError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

const MAX_EXPORT_PAGES: usize = 500;

pub fn export_page(
    notebook_root: &Path,
    output_name: &str,
    page: &ExportPage,
    allow_remote_packages: bool,
) -> Result<ExportResult, ExportError> {
    export_pages(
        notebook_root,
        output_name,
        std::slice::from_ref(page),
        allow_remote_packages,
    )
}

/// Export ordered pages as one PDF. Each page keeps its own physical geometry, ink stays
/// vector, and Typst text stays selectable. Page order is the caller's (manifest) order.
///
/// `allow_remote_packages` governs whether an uncached Typst package may be downloaded so the
/// export matches the on-screen preview.
pub fn export_pages(
    notebook_root: &Path,
    output_name: &str,
    pages: &[ExportPage],
    allow_remote_packages: bool,
) -> Result<ExportResult, ExportError> {
    let root = notebook_root
        .canonicalize()
        .map_err(|_| ExportError::InvalidRoot(notebook_root.to_path_buf()))?;
    if !root.is_dir() {
        return Err(ExportError::InvalidRoot(root));
    }
    validate_output_name(output_name)?;
    if pages.is_empty() || pages.len() > MAX_EXPORT_PAGES {
        return Err(ExportError::InvalidPage(format!(
            "export needs between 1 and {MAX_EXPORT_PAGES} pages"
        )));
    }
    for page in pages {
        validate_page(&root, page)?;
    }

    let pdf_bytes = compile_pdf(&root, pages, allow_remote_packages)?;

    let exports = root.join("exports");
    let canonical_exports = goodtype_core::paths::ensure_dir(&root, &exports, "exports")
        .map_err(|_| ExportError::InvalidRoot(exports))?;

    let destination = canonical_exports.join(output_name);
    let mut temporary = tempfile::NamedTempFile::new_in(&canonical_exports)?;
    std::io::Write::write_all(&mut temporary, &pdf_bytes)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(&destination)
        .map_err(|error| ExportError::Io(error.error))?;

    Ok(ExportResult {
        output_path: destination,
    })
}

/// Compile the ordered pages into PDF bytes in-process. Generated helper files
/// (blocks, ink SVGs) live only in memory — nothing is written inside the notebook tree.
fn compile_pdf(
    root: &Path,
    pages: &[ExportPage],
    allow_remote_packages: bool,
) -> Result<Vec<u8>, ExportError> {
    let mut overlay: Vec<(String, Vec<u8>)> = Vec::new();
    for (page_index, page) in pages.iter().enumerate() {
        for (index, block) in page.blocks.iter().enumerate() {
            overlay.push((
                format!("block-{page_index}-{index}.typ"),
                block.source.clone().into_bytes(),
            ));
        }
        if let Some(paper) = template_svg(page) {
            overlay.push((format!("paper-{page_index}.svg"), paper.into_bytes()));
        }
        overlay.push((format!("ink-{page_index}.svg"), ink_svg(page).into_bytes()));
    }

    crate::embedded::export_pdf(
        root,
        "page.typ",
        combined_typst_source(pages),
        overlay,
        allow_remote_packages,
    )
    .map_err(ExportError::CompilerFailed)
}

fn validate_output_name(name: &str) -> Result<(), ExportError> {
    let stem = name
        .strip_suffix(".pdf")
        .filter(|stem| !stem.is_empty())
        .ok_or_else(|| ExportError::InvalidOutputName(name.to_owned()))?;
    if !stem
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(ExportError::InvalidOutputName(name.to_owned()));
    }
    Ok(())
}

fn validate_page(root: &Path, page: &ExportPage) -> Result<(), ExportError> {
    valid_positive_dimension(page.width_pt, "page width")?;
    valid_positive_dimension(page.height_pt, "page height")?;
    if page.blocks.len() + page.strokes.len() + page.images.len() > MAX_PAGE_ITEMS {
        return Err(ExportError::InvalidPage("too many page items".to_owned()));
    }

    match &page.background {
        PageBackground::Plain { color } => {
            if !valid_color(color) {
                return Err(ExportError::InvalidPage("invalid page color".to_owned()));
            }
        }
        // Re-checked here even though storage already did: export is reached by its own command
        // and must not assume the page came through a write it can vouch for.
        PageBackground::Template { template } => goodtype_core::template::validate(template)
            .map_err(|reason| ExportError::InvalidPage(reason.to_owned()))?,
        PageBackground::Pdf { .. } => {}
    }

    for block in &page.blocks {
        valid_position(block.x, block.y)?;
        valid_positive_dimension(block.layout_width_pt, "block width")?;
        valid_positive_dimension(block.scale, "block scale")?;
        if block.source.len() > MAX_SOURCE_BYTES {
            return Err(ExportError::InvalidPage(
                "Typst block source is too large".to_owned(),
            ));
        }
    }

    let mut point_count = 0usize;
    for stroke in &page.strokes {
        if !valid_color(&stroke.color) {
            return Err(ExportError::InvalidPage("invalid stroke color".to_owned()));
        }
        valid_positive_dimension(stroke.width_pt, "stroke width")?;
        point_count = point_count
            .checked_add(stroke.points.len())
            .ok_or_else(|| ExportError::InvalidPage("too many stroke points".to_owned()))?;
        if point_count > MAX_STROKE_POINTS {
            return Err(ExportError::InvalidPage(
                "too many stroke points".to_owned(),
            ));
        }
        for point in &stroke.points {
            valid_position(point.x, point.y)?;
        }
        let transform = stroke.transform;
        for value in [
            transform.translate_x,
            transform.translate_y,
            transform.scale_x,
            transform.scale_y,
            transform.rotation_degrees,
        ] {
            if !value.is_finite() {
                return Err(ExportError::InvalidPage(
                    "non-finite stroke transform".to_owned(),
                ));
            }
        }
    }

    for image in &page.images {
        validate_relative_image(root, &image.relative_path)?;
        valid_position(image.x, image.y)?;
        valid_positive_dimension(image.width_pt, "image width")?;
        valid_positive_dimension(image.height_pt, "image height")?;
        valid_positive_dimension(image.scale, "image scale")?;
    }
    Ok(())
}

fn valid_positive_dimension(value: f64, name: &str) -> Result<(), ExportError> {
    if value.is_finite() && value > 0.0 && value <= MAX_DIMENSION_PT {
        Ok(())
    } else {
        Err(ExportError::InvalidPage(format!("invalid {name}")))
    }
}

fn valid_position(x: f64, y: f64) -> Result<(), ExportError> {
    if x.is_finite() && y.is_finite() {
        Ok(())
    } else {
        Err(ExportError::InvalidPage(
            "non-finite object position".to_owned(),
        ))
    }
}

fn valid_color(color: &str) -> bool {
    matches!(color.len(), 7 | 9)
        && color.starts_with('#')
        && color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// An image path out of the notebook, checked by the same rule the store applied when it wrote
/// one. These names come from canonical JSON, so the strict notebook rule is the right one — and
/// sharing it means export cannot end up accepting a name the store would have refused.
fn validate_relative_image(root: &Path, relative: &str) -> Result<(), ExportError> {
    goodtype_core::paths::resolve_file(root, relative)
        .map(|_| ())
        .map_err(|_| ExportError::InvalidImagePath(relative.to_owned()))
}

fn combined_typst_source(pages: &[ExportPage]) -> String {
    let mut source = String::new();
    for (page_index, page) in pages.iter().enumerate() {
        if page_index > 0 {
            // An explicit break keeps consecutive same-geometry pages distinct; the following
            // `#set page` then applies to the fresh empty page.
            source.push_str("#pagebreak()\n");
        }
        source.push_str(&generated_typst_source(page, page_index));
    }
    source
}

fn generated_typst_source(page: &ExportPage, page_index: usize) -> String {
    // The paper colour is the page fill rather than a placed rectangle, so it reaches the very
    // edge without depending on a shape being sized exactly right.
    let fill = match &page.background {
        PageBackground::Plain { color } => Some(color.clone()),
        PageBackground::Template { template } => Some(template.background_color.clone()),
        PageBackground::Pdf { .. } => None,
    };
    let mut source = match fill {
        Some(color) => format!(
            "#set page(width: {}pt, height: {}pt, margin: 0pt, fill: rgb(\"{}\"))\n",
            page.width_pt,
            page.height_pt,
            typst_string(&color),
        ),
        None => format!(
            "#set page(width: {}pt, height: {}pt, margin: 0pt)\n",
            page.width_pt, page.height_pt
        ),
    };
    // Placed first so everything else stacks on top of it: the template is paper, not content.
    if matches!(page.background, PageBackground::Template { .. }) {
        source.push_str(&format!(
            "#place(top + left)[#image(\"paper-{page_index}.svg\", width: {}pt, height: {}pt)]\n",
            page.width_pt, page.height_pt
        ));
    }
    for (index, block) in page.blocks.iter().enumerate() {
        source.push_str(&format!(
            "#place(top + left, dx: {}pt, dy: {}pt)[#scale(x: {}%, y: {}%, origin: top + left)[#block(width: {}pt)[#include \"block-{page_index}-{index}.typ\"]]]\n",
            block.x,
            block.y,
            block.scale * 100.0,
            block.scale * 100.0,
            block.layout_width_pt,
        ));
    }
    for image in &page.images {
        source.push_str(&format!(
            "#place(top + left, dx: {}pt, dy: {}pt)[#scale(x: {}%, y: {}%, origin: top + left)[#image(\"/{}\", width: {}pt, height: {}pt)]]\n",
            image.x,
            image.y,
            image.scale * 100.0,
            image.scale * 100.0,
            typst_string(&image.relative_path),
            image.width_pt,
            image.height_pt,
        ));
    }
    source.push_str(&format!(
        "#place(top + left)[#image(\"ink-{page_index}.svg\", width: {}pt, height: {}pt)]\n",
        page.width_pt, page.height_pt
    ));
    source
}

fn typst_string(value: &str) -> String {
    value
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

/// The page's template as SVG, or `None` when the paper carries no ruling.
///
/// The mirror of `templateSvg` in `apps/desktop/src/lib/page/template.ts`. Both go through
/// `goodtype_core::template::resolve`, whose two implementations are pinned by
/// `fixtures/templates/resolved.json` — which is what stops the PDF putting a line where the
/// screen did not.
///
/// The paper colour is not repeated here: it is the Typst page fill, so it reaches the edge.
fn template_svg(page: &ExportPage) -> Option<String> {
    let PageBackground::Template { template } = &page.background else {
        return None;
    };
    let geometry = PageGeometry {
        width_pt: page.width_pt,
        height_pt: page.height_pt,
    };
    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}pt" height="{}pt" viewBox="0 0 {} {}">"#,
        page.width_pt, page.height_pt, page.width_pt, page.height_pt
    );
    for shape in resolve(template, &geometry) {
        match shape {
            TemplateShape::Line {
                x1,
                y1,
                x2,
                y2,
                color,
                weight_pt,
            } => svg.push_str(&format!(
                r#"<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="{color}" stroke-width="{weight_pt}"/>"#
            )),
            TemplateShape::Dot {
                cx,
                cy,
                radius_pt,
                color,
            } => svg.push_str(&format!(
                r#"<circle cx="{cx}" cy="{cy}" r="{radius_pt}" fill="{color}"/>"#
            )),
        }
    }
    svg.push_str("</svg>");
    Some(svg)
}

fn ink_svg(page: &ExportPage) -> String {
    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}pt" height="{}pt" viewBox="0 0 {} {}">"#,
        page.width_pt, page.height_pt, page.width_pt, page.height_pt
    );
    // Ink is exported as its silhouette, filled — not as a stroked centreline.
    // A constant `stroke-width` cannot vary along a path, which is how pressure used to be
    // visible on screen and flat in the PDF. Filling the same polygon the canvas draws makes the
    // export match what was written.
    for stroke in &page.strokes {
        if stroke.points.is_empty() {
            continue;
        }
        let scale = stroke
            .transform
            .scale_x
            .abs()
            .max(stroke.transform.scale_y.abs());
        let points: Vec<OutlinePoint> = stroke
            .points
            .iter()
            .map(|point| {
                let placed = transformed_point(*point, stroke.transform);
                OutlinePoint {
                    x: placed.x,
                    y: placed.y,
                    pressure: point.pressure,
                }
            })
            .collect();
        let polygon = outline_points(
            &points,
            &OutlineOptions {
                width_pt: stroke.width_pt * scale,
                pressure: stroke.pressure,
                taper: stroke.taper,
            },
        );
        if polygon.is_empty() {
            continue;
        }
        svg.push_str(r#"<path d=""#);
        for (index, vertex) in polygon.iter().enumerate() {
            svg.push_str(if index == 0 { "M " } else { " L " });
            svg.push_str(&format!("{} {}", vertex.x, vertex.y));
        }
        // One filled shape per stroke, so translucency is honest: overlapping segments used to
        // double-darken at every joint, which is why an even alpha was not previously safe.
        let opacity = stroke.opacity.clamp(0.0, 1.0);
        svg.push_str(&format!(
            r#" Z" fill="{}" fill-opacity="{opacity}" fill-rule="nonzero"/>"#,
            stroke.color
        ));
    }
    svg.push_str("</svg>");
    svg
}

fn transformed_point(point: ExportPoint, transform: ExportTransform) -> ExportPoint {
    let x = point.x * transform.scale_x;
    let y = point.y * transform.scale_y;
    let radians = transform.rotation_degrees.to_radians();
    ExportPoint {
        x: x * radians.cos() - y * radians.sin() + transform.translate_x,
        y: x * radians.sin() + y * radians.cos() + transform.translate_y,
        // Placement moves a sample; how hard it was pressed does not change.
        pressure: point.pressure,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn page() -> ExportPage {
        ExportPage {
            width_pt: 595.0,
            height_pt: 842.0,
            background: PageBackground::Plain {
                color: "#ffffff".to_owned(),
            },
            blocks: vec![ExportTypstBlock {
                x: 72.0,
                y: 96.0,
                layout_width_pt: 240.0,
                scale: 1.25,
                source: include_str!("../../../fixtures/pdf/phase0b/block.typ").to_owned(),
            }],
            strokes: vec![ExportStroke {
                color: "#111111".to_owned(),
                width_pt: 2.0,
                pressure: false,
                taper: 0.0,
                opacity: 1.0,
                points: vec![
                    ExportPoint {
                        x: 10.0,
                        y: 20.0,
                        pressure: 1.0,
                    },
                    ExportPoint {
                        x: 20.0,
                        y: 30.0,
                        pressure: 1.0,
                    },
                ],
                transform: ExportTransform {
                    translate_x: 2.0,
                    translate_y: 3.0,
                    scale_x: 2.0,
                    scale_y: 2.0,
                    rotation_degrees: 0.0,
                },
            }],
            images: vec![],
        }
    }

    #[test]
    fn generates_native_blocks_and_vector_ink() {
        let page = page();
        let source = generated_typst_source(&page, 0);
        let ink = ink_svg(&page);

        assert!(source.contains("#include \"block-0-0.typ\""));
        assert!(source.contains("#scale(x: 125%"));
        assert!(source.contains("#image(\"ink-0.svg\""));
        // Ink exports as a filled silhouette, not a stroked centreline. The centre
        // line runs (22,43)→(42,63) after the transform, at width 4, so the outline sits half a
        // width either side of it along the normal.
        assert!(
            ink.contains(r#"M 20.586 44.414 L 40.586 64.414 L 43.414 61.586 L 23.414 41.586 Z"#),
            "{ink}"
        );
        assert!(ink.contains(r##"fill="#111111""##), "{ink}");
        // A constant stroke width is exactly what cannot express pressure; it must be gone.
        assert!(!ink.contains("stroke-width"), "{ink}");
    }

    /// A template is paper, so it has to be under the ink and reach the page edge — and it has
    /// to be the *same* paper the screen drew, which is what the shared fixture guarantees.
    #[test]
    fn a_template_exports_as_paper_under_the_ink() {
        use goodtype_core::template::{Area, PageTemplate, TemplateElement};

        let mut page = page();
        page.background = PageBackground::Template {
            template: PageTemplate {
                id: "ruled".to_owned(),
                name: "Ruled".to_owned(),
                background_color: "#FCFCFA".to_owned(),
                elements: vec![TemplateElement::HorizontalLines {
                    area: Area {
                        top_pt: 36.0,
                        right_pt: 36.0,
                        bottom_pt: 36.0,
                        left_pt: 36.0,
                    },
                    spacing_pt: 24.0,
                    color: "#D4DAE0".to_owned(),
                    weight_pt: 0.5,
                }],
            },
        };

        let source = generated_typst_source(&page, 0);
        // The paper colour is the page fill, so it reaches the edge without a shape to size.
        assert!(source.contains(r##"fill: rgb("#FCFCFA")"##), "{source}");
        let paper = source.find("paper-0.svg").expect("paper placed");
        let ink = source.find("ink-0.svg").expect("ink placed");
        assert!(paper < ink, "paper must be placed before the ink: {source}");

        // 842pt tall, 36pt margins, 24pt ruling: 32 whole steps span 768pt of the 770pt
        // available, so the leftover 2pt is split — 1pt above the first line and 1pt below the
        // last, rather than the whole remainder piling up against the bottom margin.
        let svg = template_svg(&page).expect("a template renders");
        assert!(
            svg.contains(r##"<line x1="36" y1="37" x2="559" y2="37" stroke="#D4DAE0""##),
            "{svg}"
        );
        assert!(svg.contains(r#"y1="805""#), "{svg}");
        // Ruling stops inside the margin rather than running off the page.
        assert!(!svg.contains(r#"y1="829""#), "{svg}");

        // Plain paper does not pay for an SVG it would not draw.
        let mut plain = super::tests::page();
        plain.background = PageBackground::Plain {
            color: "#ffffff".to_owned(),
        };
        assert!(template_svg(&plain).is_none());
    }

    /// Pressure has to survive into the exported geometry — it used to be visible on screen and
    /// flat in the PDF, because a stroked centreline cannot vary its width.
    #[test]
    fn export_geometry_varies_with_pressure() {
        let mut page = page();
        page.strokes[0].pressure = true;
        page.strokes[0].transform = ExportTransform {
            translate_x: 0.0,
            translate_y: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            rotation_degrees: 0.0,
        };
        page.strokes[0].points = vec![
            ExportPoint {
                x: 0.0,
                y: 0.0,
                pressure: 0.0,
            },
            ExportPoint {
                x: 10.0,
                y: 0.0,
                pressure: 1.0,
            },
        ];
        page.strokes[0].width_pt = 4.0;

        let ink = ink_svg(&page);
        // Light end is a quarter width (±0.5), full end is the whole width (±2).
        assert!(ink.contains("M 0 0.5 L 10 2 L 10 -2 L 0 -0.5 Z"), "{ink}");
    }

    /// A highlighter is translucent on screen; without this it came out solid in the PDF, hiding
    /// whatever it was drawn over. One filled shape per stroke is what makes an even alpha safe.
    #[test]
    fn export_keeps_translucent_ink_translucent() {
        let mut page = page();
        page.strokes[0].opacity = 0.6;
        assert!(ink_svg(&page).contains(r#"fill-opacity="0.6""#), "{page:?}");

        page.strokes[0].opacity = 1.0;
        assert!(ink_svg(&page).contains(r#"fill-opacity="1""#));
    }

    #[test]
    fn rejects_unsafe_names_paths_and_numbers() {
        for name in ["../page.pdf", "folder/page.pdf", "page.PDF", ".pdf"] {
            assert!(matches!(
                validate_output_name(name),
                Err(ExportError::InvalidOutputName(_))
            ));
        }

        let root = tempfile::tempdir().unwrap();
        for path in ["../image.png", "/image.png", r"assets\image.png"] {
            assert!(matches!(
                validate_relative_image(root.path(), path),
                Err(ExportError::InvalidImagePath(_))
            ));
        }

        let mut invalid = page();
        invalid.width_pt = f64::NAN;
        assert!(matches!(
            validate_page(root.path(), &invalid),
            Err(ExportError::InvalidPage(_))
        ));
    }

    #[test]
    fn compiler_smoke_creates_pdf() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("assets")).unwrap();
        fs::write(
            root.path().join("assets/diagram.svg"),
            include_bytes!("../../../fixtures/pdf/phase0b/image.svg"),
        )
        .unwrap();
        let mut page = page();
        page.images.push(ExportImage {
            relative_path: "assets/diagram.svg".to_owned(),
            x: 360.0,
            y: 96.0,
            width_pt: 72.0,
            height_pt: 72.0,
            scale: 1.0,
        });

        let result = export_page(root.path(), "phase0b.pdf", &page, false).unwrap();
        let bytes = fs::read(&result.output_path).unwrap();
        assert!(result.output_path.ends_with("exports/phase0b.pdf"));
        assert!(bytes.starts_with(b"%PDF-"));
        export_page(root.path(), "phase0b.pdf", &page, false).unwrap();
    }

    #[test]
    fn multi_page_export_keeps_order_and_geometry() {
        let root = tempfile::tempdir().unwrap();
        let mut second = page();
        second.blocks[0].source = "= Second page marker".to_owned();
        // A distinct geometry on page 2 must survive into the PDF.
        second.width_pt = 400.0;
        second.height_pt = 300.0;
        let third = ExportPage {
            width_pt: 595.0,
            height_pt: 842.0,
            background: PageBackground::Plain {
                color: "#ffffff".to_owned(),
            },
            blocks: vec![],
            strokes: vec![],
            images: vec![],
        };

        // The combined document keeps manifest order, per-page geometry, explicit breaks, and
        // page-scoped block/ink file names. (PDF-level page-count evidence stays with the
        // count asserted at the source level rather than in the PDF, whose streams are compressed.)
        let pages = [page(), second, third];
        let source = combined_typst_source(&pages);
        assert_eq!(source.matches("#pagebreak()").count(), 2);
        assert_eq!(source.matches("#set page(").count(), 3);
        assert!(source.contains("width: 400pt, height: 300pt"));
        assert!(source.contains("#include \"block-1-0.typ\""));
        assert!(source.contains("ink-2.svg"));
        let break_at = source.find("#pagebreak()").unwrap();
        assert!(source.find("width: 400pt").unwrap() > break_at);

        let result = export_pages(root.path(), "notebook.pdf", &pages, false).unwrap();
        let bytes = fs::read(&result.output_path).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        assert!(matches!(
            export_pages(root.path(), "empty.pdf", &[], false),
            Err(ExportError::InvalidPage(_))
        ));
    }
}
