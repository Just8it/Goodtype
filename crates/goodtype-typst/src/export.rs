use std::{
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

const MAX_SOURCE_BYTES: usize = 1024 * 1024;
const MAX_PAGE_ITEMS: usize = 10_000;
const MAX_STROKE_POINTS: usize = 1_000_000;
const MAX_DIMENSION_PT: f64 = 100_000.0;

#[derive(Debug, Clone, PartialEq)]
pub struct ExportPage {
    pub width_pt: f64,
    pub height_pt: f64,
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
    pub points: Vec<ExportPoint>,
    pub transform: ExportTransform,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExportPoint {
    pub x: f64,
    pub y: f64,
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

pub fn export_page(
    notebook_root: &Path,
    output_name: &str,
    page: &ExportPage,
) -> Result<ExportResult, ExportError> {
    let root = notebook_root
        .canonicalize()
        .map_err(|_| ExportError::InvalidRoot(notebook_root.to_path_buf()))?;
    if !root.is_dir() {
        return Err(ExportError::InvalidRoot(root));
    }
    validate_output_name(output_name)?;
    validate_page(&root, page)?;

    let workspace = tempfile::Builder::new()
        .prefix(".goodtype-export-")
        .tempdir_in(&root)?;
    for (index, block) in page.blocks.iter().enumerate() {
        fs::write(
            workspace.path().join(format!("block-{index}.typ")),
            &block.source,
        )?;
    }
    fs::write(workspace.path().join("ink.svg"), ink_svg(page))?;
    fs::write(
        workspace.path().join("page.typ"),
        generated_typst_source(page),
    )?;

    let compiled_pdf = workspace.path().join("page.pdf");
    let output = Command::new(crate::typst_compiler())
        .arg("compile")
        .arg("--root")
        .arg(&root)
        .arg("--diagnostic-format")
        .arg("short")
        .arg(workspace.path().join("page.typ"))
        .arg(&compiled_pdf)
        .output()?;
    if !output.status.success() {
        return Err(ExportError::CompilerFailed(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }

    let exports = root.join("exports");
    fs::create_dir_all(&exports)?;
    let canonical_exports = exports.canonicalize()?;
    if !canonical_exports.starts_with(&root) {
        return Err(ExportError::InvalidRoot(canonical_exports));
    }

    let destination = canonical_exports.join(output_name);
    let mut temporary = tempfile::NamedTempFile::new_in(&canonical_exports)?;
    std::io::copy(&mut fs::File::open(compiled_pdf)?, &mut temporary)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(&destination)
        .map_err(|error| ExportError::Io(error.error))?;

    Ok(ExportResult {
        output_path: destination,
    })
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

fn validate_relative_image(root: &Path, relative: &str) -> Result<(), ExportError> {
    if relative.contains('\\') {
        return Err(ExportError::InvalidImagePath(relative.to_owned()));
    }
    let path = Path::new(relative);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(ExportError::InvalidImagePath(relative.to_owned()));
    }
    let canonical = root
        .join(path)
        .canonicalize()
        .map_err(|_| ExportError::InvalidImagePath(relative.to_owned()))?;
    if !canonical.starts_with(root) || !canonical.is_file() {
        return Err(ExportError::InvalidImagePath(relative.to_owned()));
    }
    Ok(())
}

fn generated_typst_source(page: &ExportPage) -> String {
    let mut source = format!(
        "#set page(width: {}pt, height: {}pt, margin: 0pt)\n",
        page.width_pt, page.height_pt
    );
    for (index, block) in page.blocks.iter().enumerate() {
        source.push_str(&format!(
            "#place(top + left, dx: {}pt, dy: {}pt)[#scale(x: {}%, y: {}%, origin: top + left)[#block(width: {}pt)[#include \"block-{index}.typ\"]]]\n",
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
        "#place(top + left)[#image(\"ink.svg\", width: {}pt, height: {}pt)]\n",
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

fn ink_svg(page: &ExportPage) -> String {
    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}pt" height="{}pt" viewBox="0 0 {} {}">"#,
        page.width_pt, page.height_pt, page.width_pt, page.height_pt
    );
    for stroke in &page.strokes {
        if stroke.points.is_empty() {
            continue;
        }
        let width = stroke.width_pt
            * stroke
                .transform
                .scale_x
                .abs()
                .max(stroke.transform.scale_y.abs());
        svg.push_str(r#"<path d=""#);
        for (index, point) in stroke.points.iter().enumerate() {
            let point = transformed_point(*point, stroke.transform);
            svg.push_str(if index == 0 { "M " } else { " L " });
            svg.push_str(&format!("{} {}", point.x, point.y));
        }
        svg.push_str(&format!(
            r#"" fill="none" stroke="{}" stroke-width="{}" stroke-linecap="round" stroke-linejoin="round"/>"#,
            stroke.color, width
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> ExportPage {
        ExportPage {
            width_pt: 595.0,
            height_pt: 842.0,
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
                points: vec![
                    ExportPoint { x: 10.0, y: 20.0 },
                    ExportPoint { x: 20.0, y: 30.0 },
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
        let source = generated_typst_source(&page);
        let ink = ink_svg(&page);

        assert!(source.contains("#include \"block-0.typ\""));
        assert!(source.contains("#scale(x: 125%"));
        assert!(source.contains("#image(\"ink.svg\""));
        assert!(ink.contains(r#"<path d="M 22 43 L 42 63""#));
        assert!(ink.contains(r#"stroke-width="4""#));
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
        let compiler = crate::typst_compiler();
        if !compiler.is_file() {
            eprintln!("Typst compiler is unavailable; skipping PDF compiler smoke test");
            return;
        }

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

        let result = export_page(root.path(), "phase0b.pdf", &page).unwrap();
        let bytes = fs::read(&result.output_path).unwrap();
        assert!(result.output_path.ends_with("exports/phase0b.pdf"));
        assert!(bytes.starts_with(b"%PDF-"));
        export_page(root.path(), "phase0b.pdf", &page).unwrap();
    }
}
