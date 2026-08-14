use std::path::{Path, PathBuf};

use goodtype_core::{
    PageBackground, PageGeometry,
    outline::{OutlineOptions, OutlinePoint, outline_points},
    template::{Area, Edge, TemplateElement, TemplateShape, resolve},
};

/// The crate's one Typst source ceiling. This module had its own copy of the same number, which
/// shadowed the public one at every call site below and agreed with it only by coincidence.
use crate::MAX_SOURCE_BYTES;

const MAX_PAGE_ITEMS: usize = 10_000;
const MAX_STROKE_POINTS: usize = 1_000_000;
const MAX_DIMENSION_PT: f64 = 100_000.0;

#[derive(Debug, Clone, PartialEq)]
pub struct ExportPage {
    pub width_pt: f64,
    pub height_pt: f64,
    pub shared_style: Option<String>,
    /// The paper. Carried through rather than assumed white, so a page written on ruled paper
    /// exports as ruled paper — the template is part of the page, not a screen decoration.
    pub background: PageBackground,
    /// App preference used for this export; Page text remains canonical ordinary Typst source.
    pub page_text_baseline_grid: bool,
    pub page_typst: Option<String>,
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
    pub rotation_degrees: f64,
    pub z_index: i32,
    pub order: usize,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExportStroke {
    pub z_index: i32,
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
    pub rotation_degrees: f64,
    pub z_index: i32,
    pub order: usize,
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
        if let Some(page_text) = &page.page_typst {
            let source = match page.shared_style.as_deref() {
                _ if has_managed_preset(page_text) => page_text_source(page, page_text),
                Some(style) if !style.is_empty() => {
                    format!("{style}\n{}", page_text_source(page, page_text))
                }
                _ => page_text_source(page, page_text),
            };
            overlay.push((format!("page-text-{page_index}.typ"), source.into_bytes()));
        }
        for (index, block) in page.blocks.iter().enumerate() {
            let source = match page.shared_style.as_deref() {
                Some(style) if !style.is_empty() => format!("{style}\n{}", block.source),
                _ => block.source.clone(),
            };
            overlay.push((
                format!("block-{page_index}-{index}.typ"),
                source.into_bytes(),
            ));
        }
        if let Some(paper) = template_svg(page) {
            overlay.push((format!("paper-{page_index}.svg"), paper.into_bytes()));
        }
        for (stratum_index, stratum) in ink_strata(page).iter().enumerate() {
            overlay.push((
                format!("ink-{page_index}-{stratum_index}.svg"),
                ink_svg_for(page, stratum.strokes.iter().copied()).into_bytes(),
            ));
        }
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
    if page.blocks.len()
        + page.strokes.len()
        + page.images.len()
        + usize::from(page.page_typst.is_some())
        > MAX_PAGE_ITEMS
    {
        return Err(ExportError::InvalidPage("too many page items".to_owned()));
    }
    if page
        .shared_style
        .as_ref()
        .is_some_and(|style| style.len() > MAX_SOURCE_BYTES)
    {
        return Err(ExportError::InvalidPage(
            "shared Typst style is too large".to_owned(),
        ));
    }
    if page.page_typst.as_ref().is_some_and(|source| {
        source.len() > MAX_SOURCE_BYTES
            || page
                .shared_style
                .as_ref()
                .is_some_and(|style| style.len() + source.len() > MAX_SOURCE_BYTES)
    }) {
        return Err(ExportError::InvalidPage(
            "page Typst source is too large".to_owned(),
        ));
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
        PageBackground::Pdf { source_path, page } => {
            if *page == 0 {
                return Err(ExportError::InvalidPage(
                    "PDF page numbers start at one".to_owned(),
                ));
            }
            if Path::new(source_path)
                .components()
                .next()
                .is_none_or(|component| {
                    component.as_os_str() != goodtype_core::SourceRole::Reference.directory()
                })
            {
                return Err(ExportError::InvalidImagePath(source_path.clone()));
            }
            validate_relative_image(root, source_path)?;
        }
    }

    for block in &page.blocks {
        valid_position(block.x, block.y)?;
        valid_positive_dimension(block.layout_width_pt, "block width")?;
        valid_positive_dimension(block.scale, "block scale")?;
        if !block.rotation_degrees.is_finite() {
            return Err(ExportError::InvalidPage(
                "non-finite block rotation".to_owned(),
            ));
        }
        if block.source.len() > MAX_SOURCE_BYTES {
            return Err(ExportError::InvalidPage(
                "Typst block source is too large".to_owned(),
            ));
        }
        if page
            .shared_style
            .as_ref()
            .is_some_and(|style| style.len() + block.source.len() > MAX_SOURCE_BYTES)
        {
            return Err(ExportError::InvalidPage(
                "combined Typst style and block source is too large".to_owned(),
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
        if !image.rotation_degrees.is_finite() {
            return Err(ExportError::InvalidPage(
                "non-finite image rotation".to_owned(),
            ));
        }
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

use goodtype_core::valid_hex_color as valid_color;

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
    if let PageBackground::Pdf {
        source_path,
        page: pdf_page,
    } = &page.background
    {
        source.push_str(&format!(
            "#place(top + left)[#image(\"/{}\", page: {}, width: {}pt, height: {}pt, fit: \"stretch\")]\n",
            typst_string(source_path),
            pdf_page,
            page.width_pt,
            page.height_pt
        ));
    }
    if page.page_typst.is_some() {
        let layout = page_text_layout(page);
        source.push_str(&format!(
            "#place(top + left, dx: {}pt, dy: {}pt)[#block(width: {}pt)[#include \"page-text-{page_index}.typ\"]]\n",
            layout.x, layout.y, layout.width,
        ));
    }
    let strata = ink_strata(page);
    let mut objects = Vec::with_capacity(page.blocks.len() + page.images.len() + strata.len());
    for (index, block) in page.blocks.iter().enumerate() {
        objects.push((
            block.z_index,
            block.order,
            format!(
                "#place(top + left, dx: {}pt, dy: {}pt)[#rotate({}deg, origin: top + left)[#scale(x: {}%, y: {}%, origin: top + left)[#block(width: {}pt)[#include \"block-{page_index}-{index}.typ\"]]]]\n",
                block.x,
                block.y,
                block.rotation_degrees,
                block.scale * 100.0,
                block.scale * 100.0,
                block.layout_width_pt,
            ),
        ));
    }
    for image in &page.images {
        objects.push((
            image.z_index,
            image.order,
            format!(
                "#place(top + left, dx: {}pt, dy: {}pt)[#rotate({}deg, origin: top + left)[#scale(x: {}%, y: {}%, origin: top + left)[#image(\"/{}\", width: {}pt, height: {}pt)]]]\n",
                image.x,
                image.y,
                image.rotation_degrees,
                image.scale * 100.0,
                image.scale * 100.0,
                typst_string(&image.relative_path),
                image.width_pt,
                image.height_pt,
            ),
        ));
    }
    for (index, stratum) in strata.iter().enumerate() {
        objects.push((
            stratum.z_index,
            stratum.order,
            format!(
                "#place(top + left)[#image(\"ink-{page_index}-{index}.svg\", width: {}pt, height: {}pt)]\n",
                page.width_pt, page.height_pt
            ),
        ));
    }
    objects.sort_by_key(|(z_index, order, _)| (*z_index, *order));
    for (_, _, object) in objects {
        source.push_str(&object);
    }
    source
}

struct InkStratum<'a> {
    z_index: i32,
    order: usize,
    strokes: Vec<&'a ExportStroke>,
}

fn ink_strata(page: &ExportPage) -> Vec<InkStratum<'_>> {
    let mut boundaries: Vec<i32> = page
        .blocks
        .iter()
        .map(|block| block.z_index)
        .chain(page.images.iter().map(|image| image.z_index))
        .collect();
    boundaries.sort_unstable();
    let mut strokes: Vec<(usize, &ExportStroke)> = page.strokes.iter().enumerate().collect();
    strokes.sort_by_key(|(order, stroke)| (stroke.z_index, *order));

    let mut groups: Vec<(usize, InkStratum<'_>)> = Vec::new();
    for (stroke_order, stroke) in strokes {
        let band = boundaries.partition_point(|boundary| *boundary <= stroke.z_index);
        if groups
            .last()
            .is_some_and(|(last_band, _)| *last_band == band)
        {
            groups.last_mut().unwrap().1.strokes.push(stroke);
        } else {
            groups.push((
                band,
                InkStratum {
                    z_index: stroke.z_index,
                    // Object order is always below this offset, so an equal z-index keeps ink
                    // after the object just as the screen DOM does.
                    order: MAX_PAGE_ITEMS + stroke_order,
                    strokes: vec![stroke],
                },
            ));
        }
    }
    groups.into_iter().map(|(_, stratum)| stratum).collect()
}

/// Escape a string being embedded in generated Typst source.
///
/// The backslash goes first and must stay first: escaping it after the quote would turn the `\"`
/// this produces back into a literal backslash followed by an unescaped quote. Nothing reaching
/// here can currently contain one — colours are hex-validated and paths went through
/// `validate_relative`, which refuses `\` — but this is the one function standing between
/// canonical content and generated code, and it should not depend on its callers to be safe.
fn typst_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

#[derive(Clone, Copy)]
struct PageTextLayout {
    x: f64,
    y: f64,
    width: f64,
    line_spacing_pt: f64,
    columns: u8,
}

struct PageTextBounds {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

fn page_text_layout(page: &ExportPage) -> PageTextLayout {
    const MARGIN: f64 = 36.0;
    let fallback = PageTextLayout {
        x: MARGIN,
        y: MARGIN,
        width: (page.width_pt - 2.0 * MARGIN).max(72.0),
        line_spacing_pt: 16.0,
        columns: 1,
    };
    let PageBackground::Template { template } = &page.background else {
        return fallback;
    };
    let Some((area, guide_spacing, horizontal_only)) =
        template.elements.iter().find_map(|element| match element {
            TemplateElement::HorizontalLines {
                area, spacing_pt, ..
            } => Some((area, *spacing_pt, true)),
            TemplateElement::Grid {
                area, spacing_pt, ..
            }
            | TemplateElement::Dots {
                area, spacing_pt, ..
            } => Some((area, *spacing_pt, false)),
            _ => None,
        })
    else {
        return fallback;
    };
    let Some(bounds) = page_text_bounds(area, page.width_pt, page.height_pt) else {
        return fallback;
    };
    let spacing = if guide_spacing < 18.0 {
        guide_spacing * 2.0
    } else {
        guide_spacing
    };
    let geometry = PageGeometry {
        width_pt: page.width_pt,
        height_pt: page.height_pt,
    };
    let shapes = resolve(template, &geometry);
    let mut rows = shapes
        .iter()
        .filter_map(|shape| match shape {
            TemplateShape::Dot { cy, .. } => Some(*cy),
            TemplateShape::Line { y1, y2, .. } if y1 == y2 => Some(*y1),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut columns = shapes
        .iter()
        .filter_map(|shape| match shape {
            TemplateShape::Dot { cx, .. } => Some(*cx),
            TemplateShape::Line { x1, x2, .. } if x1 == x2 => Some(*x1),
            _ => None,
        })
        .collect::<Vec<_>>();
    rows.sort_by(f64::total_cmp);
    rows.dedup();
    columns.sort_by(f64::total_cmp);
    columns.dedup();
    let baseline = rows
        .into_iter()
        .find(|row| *row >= bounds.top + if horizontal_only { 0.0 } else { guide_spacing })
        .unwrap_or(bounds.top + spacing);
    let column = columns
        .into_iter()
        .find(|column| *column >= bounds.left)
        .unwrap_or(bounds.left);
    let x = if horizontal_only { bounds.left } else { column } + 2.0;
    PageTextLayout {
        x,
        y: (baseline - 13.0).max(0.0),
        width: (bounds.right - guide_spacing - x).max(72.0),
        line_spacing_pt: spacing,
        columns: u8::from(template.elements.iter().any(|element| {
            matches!(
                element,
                TemplateElement::Rule {
                    edge: Edge::CenterX,
                    ..
                }
            )
        })) + 1,
    }
}

fn page_text_source(page: &ExportPage, source: &str) -> String {
    let layout = page_text_layout(page);
    let color = match &page.background {
        PageBackground::Plain { color } => color,
        PageBackground::Template { template } => &template.background_color,
        PageBackground::Pdf { .. } => "#ffffff",
    };
    let text_color = readable_text(color);
    let leading = (layout.line_spacing_pt - 11.0).max(0.0);
    let rhythm = if page.page_text_baseline_grid {
        format!("{}pt", layout.line_spacing_pt)
    } else {
        "1em".into()
    };
    let mut prelude = format!(
        "#set text(size: 11pt, fill: rgb(\"{text_color}\"), top-edge: 1em, bottom-edge: 0em)\n#set par(leading: {leading}pt, spacing: {leading}pt)\n#let goodtype_rhythm = {rhythm}"
    );
    if page.page_text_baseline_grid {
        prelude.push_str(&format!(
            "\n#let goodtype_gap = {leading}pt\n#let goodtype_snap_block(it) = block(\n  above: goodtype_gap,\n  below: goodtype_gap,\n  layout(size => {{\n    let measured = measure(width: size.width, it)\n    let rows = calc.max(1, calc.ceil((measured.height + goodtype_gap) / goodtype_rhythm))\n    block(width: size.width, height: rows * goodtype_rhythm - goodtype_gap, it)\n  }}),\n)\n#show heading: set block(above: 0pt, below: 0pt)\n#show math.equation.where(block: true): set block(above: 0pt, below: 0pt)\n#show heading: goodtype_snap_block\n#show math.equation.where(block: true): goodtype_snap_block",
        ));
    }
    if layout.columns == 2 {
        format!(
            "{prelude}\n#columns(2, gutter: {}pt)[\n{source}\n]",
            layout.line_spacing_pt
        )
    } else {
        format!("{prelude}\n{source}")
    }
}

use crate::has_managed_preset;

fn page_text_bounds(area: &Area, width_pt: f64, height_pt: f64) -> Option<PageTextBounds> {
    let bounds = PageTextBounds {
        left: area.left_pt,
        top: area.top_pt,
        right: width_pt - area.right_pt,
        bottom: height_pt - area.bottom_pt,
    };
    (bounds.right > bounds.left && bounds.bottom > bounds.top).then_some(bounds)
}

fn readable_text(color: &str) -> &'static str {
    let Some(hex) = color.strip_prefix('#').filter(|hex| hex.len() == 6) else {
        return "#16212b";
    };
    let Ok(value) = u32::from_str_radix(hex, 16) else {
        return "#16212b";
    };
    let luminance = ((value >> 16) * 299 + ((value >> 8) & 255) * 587 + (value & 255) * 114) / 1000;
    if luminance < 110 {
        "#eef1f4"
    } else {
        "#16212b"
    }
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

#[cfg(test)]
fn ink_svg(page: &ExportPage) -> String {
    ink_svg_for(page, &page.strokes)
}

fn ink_svg_for<'a>(
    page: &ExportPage,
    strokes: impl IntoIterator<Item = &'a ExportStroke>,
) -> String {
    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}pt" height="{}pt" viewBox="0 0 {} {}">"#,
        page.width_pt, page.height_pt, page.width_pt, page.height_pt
    );
    // Ink is exported as its silhouette, filled — not as a stroked centreline.
    // A constant `stroke-width` cannot vary along a path, which is how pressure used to be
    // visible on screen and flat in the PDF. Filling the same polygon the canvas draws makes the
    // export match what was written.
    for stroke in strokes {
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
            shared_style: None,
            background: PageBackground::Plain {
                color: "#ffffff".to_owned(),
            },
            page_text_baseline_grid: true,
            page_typst: None,
            blocks: vec![ExportTypstBlock {
                x: 72.0,
                y: 96.0,
                layout_width_pt: 240.0,
                scale: 1.25,
                rotation_degrees: 0.0,
                z_index: 2,
                order: 0,
                source: include_str!("../../../fixtures/pdf/phase0b/block.typ").to_owned(),
            }],
            strokes: vec![ExportStroke {
                z_index: 3,
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
        let mut page = page();
        page.blocks[0].rotation_degrees = 15.0;
        page.images.push(ExportImage {
            relative_path: "assets/underlay.svg".to_owned(),
            x: 10.0,
            y: 10.0,
            width_pt: 20.0,
            height_pt: 20.0,
            scale: 1.0,
            rotation_degrees: -5.0,
            z_index: 1,
            order: 1,
        });
        let source = generated_typst_source(&page, 0);
        let ink = ink_svg(&page);

        assert!(source.contains("#include \"block-0-0.typ\""));
        assert!(source.contains("#scale(x: 125%"));
        assert!(source.contains("#rotate(15deg, origin: top + left)"));
        assert!(
            source.find("underlay.svg").unwrap() < source.find("block-0-0.typ").unwrap(),
            "lower z-index must be emitted first: {source}"
        );
        assert!(source.contains("#image(\"ink-0-0.svg\""));
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

    #[test]
    fn interleaves_ink_and_objects_by_one_visual_order() {
        let mut page = page();
        page.strokes[0].z_index = 1;
        let below = generated_typst_source(&page, 0);
        assert!(
            below.find("ink-0-0.svg").unwrap() < below.find("block-0-0.typ").unwrap(),
            "lower ink must be emitted before the block: {below}"
        );

        page.strokes[0].z_index = 3;
        let above = generated_typst_source(&page, 0);
        assert!(
            above.find("block-0-0.typ").unwrap() < above.find("ink-0-0.svg").unwrap(),
            "higher ink must be emitted after the block: {above}"
        );
    }

    #[test]
    fn page_text_is_fixed_below_objects_and_uses_paper_rhythm() {
        use goodtype_core::template::{Area, PageTemplate, TemplateElement};

        let mut page = page();
        page.page_typst = Some("= Page heading\n\nBody".to_owned());
        page.background = PageBackground::Template {
            template: PageTemplate {
                id: "ruled".to_owned(),
                name: "Ruled".to_owned(),
                background_color: "#ffffff".to_owned(),
                elements: vec![TemplateElement::HorizontalLines {
                    area: Area {
                        top_pt: 36.0,
                        right_pt: 36.0,
                        bottom_pt: 36.0,
                        left_pt: 36.0,
                    },
                    spacing_pt: 24.0,
                    color: "#dddddd".to_owned(),
                    weight_pt: 0.5,
                }],
            },
        };

        let source = generated_typst_source(&page, 0);
        assert!(source.contains("#include \"page-text-0.typ\""), "{source}");
        assert!(
            source.find("page-text-0.typ").unwrap() < source.find("block-0-0.typ").unwrap(),
            "page text must stay below movable objects: {source}"
        );
        let layout = page_text_layout(&page);
        assert_eq!((layout.x, layout.y, layout.width), (38.0, 24.0, 497.0));
        let snapped = page_text_source(&page, "Body");
        assert!(snapped.contains("leading: 13pt"));
        assert!(snapped.contains("#show heading: goodtype_snap_block"));
        page.page_text_baseline_grid = false;
        assert!(!page_text_source(&page, "Body").contains("goodtype_snap_block"));
    }

    #[test]
    fn page_text_heading_and_math_leave_the_flow_on_a_whole_row() {
        let root = tempfile::tempdir().unwrap();
        let page = page();
        let compile_height = |source| {
            let result = crate::compile_block(
                root.path(),
                &crate::CompileRequest {
                    source: page_text_source(&page, source),
                    width_pt: 523.0,
                    generation: 1,
                    allow_remote_packages: false,
                },
            )
            .unwrap();
            assert!(result.svg.is_some(), "{:?}", result.diagnostics);
            result.height_pt.unwrap()
        };
        let prose = compile_height("Body\n\nAfter\n\nEnd");
        let blocks = compile_height("Body\n\n= Heading\n\nAfter\n\n$ x^2 + y^2 = z^2 $\n\nEnd");
        let added_rows = (blocks - prose) / 16.0;
        assert!(
            (added_rows - added_rows.round()).abs() < 0.001,
            "blocks added {added_rows} rows"
        );
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
        let ink = source.find("ink-0-0.svg").expect("ink placed");
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

    #[test]
    fn a_pdf_background_exports_before_the_ink() {
        let mut page = page();
        page.background = PageBackground::Pdf {
            source_path: "references/lecture.pdf".to_owned(),
            page: 3,
        };

        let source = generated_typst_source(&page, 0);
        let pdf = source
            .find(r#"#image("/references/lecture.pdf", page: 3"#)
            .expect("PDF background placed");
        let ink = source.find("ink-0-0.svg").expect("ink placed");
        assert!(pdf < ink, "PDF background must be under the ink: {source}");
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
        page.page_typst = Some("= Full-page notes\n\nSelectable prose".to_owned());
        page.images.push(ExportImage {
            relative_path: "assets/diagram.svg".to_owned(),
            x: 360.0,
            y: 96.0,
            width_pt: 72.0,
            height_pt: 72.0,
            scale: 1.0,
            rotation_degrees: 0.0,
            z_index: 2,
            order: 1,
        });

        let result = export_pages(
            root.path(),
            "phase0b.pdf",
            std::slice::from_ref(&page),
            false,
        )
        .unwrap();
        let bytes = fs::read(&result.output_path).unwrap();
        assert!(result.output_path.ends_with("exports/phase0b.pdf"));
        assert!(bytes.starts_with(b"%PDF-"));
        export_pages(
            root.path(),
            "phase0b.pdf",
            std::slice::from_ref(&page),
            false,
        )
        .unwrap();
    }

    #[test]
    fn compiler_embeds_a_pdf_page_beneath_goodtype_content() {
        let root = tempfile::tempdir().unwrap();
        let source_page = page();
        let source = export_pages(
            root.path(),
            "source.pdf",
            std::slice::from_ref(&source_page),
            false,
        )
        .unwrap();
        fs::create_dir(root.path().join("references")).unwrap();
        fs::copy(
            source.output_path,
            root.path().join("references/lecture.pdf"),
        )
        .unwrap();

        let mut annotated = page();
        annotated.background = PageBackground::Pdf {
            source_path: "references/lecture.pdf".to_owned(),
            page: 1,
        };
        let result = export_pages(
            root.path(),
            "annotated.pdf",
            std::slice::from_ref(&annotated),
            false,
        )
        .unwrap();
        assert!(fs::read(result.output_path).unwrap().starts_with(b"%PDF-"));
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
            shared_style: None,
            background: PageBackground::Plain {
                color: "#ffffff".to_owned(),
            },
            page_text_baseline_grid: true,
            page_typst: None,
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
        assert!(source.contains("ink-1-0.svg"));
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

    #[test]
    fn exports_a_hundred_page_notebook() {
        let root = tempfile::tempdir().unwrap();
        let mut blank = page();
        blank.blocks.clear();
        blank.strokes.clear();
        let pages = vec![blank; 100];

        assert_eq!(
            combined_typst_source(&pages).matches("#set page(").count(),
            100
        );
        let result = export_pages(root.path(), "hundred-pages.pdf", &pages, false).unwrap();
        assert!(fs::read(result.output_path).unwrap().starts_with(b"%PDF-"));
    }

    #[test]
    fn shared_style_is_compiled_with_each_block() {
        let root = tempfile::tempdir().unwrap();
        let mut styled = page();
        styled.shared_style = Some("#error(\"shared style reached export\")".to_owned());

        let error = export_pages(
            root.path(),
            "styled.pdf",
            std::slice::from_ref(&styled),
            false,
        )
        .unwrap_err();
        assert!(matches!(error, ExportError::CompilerFailed(_)));
    }
}
