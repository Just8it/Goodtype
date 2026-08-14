use serde::{Deserialize, Serialize};

pub mod layout;
pub mod object;
pub mod outline;
pub mod paths;
pub mod storage;
pub mod template;

pub use object::{IdRemap, ObjectFields, PageObject, SourceRef, SourceRole};

pub const SCHEMA_VERSION: u32 = 1;
/// Legacy strokes painted above movable objects. Missing version-1 values keep that look.
pub const DEFAULT_INK_Z_INDEX: i32 = 1_000_000;

const fn default_ink_z_index() -> i32 {
    DEFAULT_INK_Z_INDEX
}

/// The largest a page may be, in points — a little over 35 metres on a side.
///
/// Far past any paper, and deliberately so: this is not a taste limit but the bound that makes
/// everything downstream finite. A template resolves into a shape per spacing step, so page size
/// is what decides how much work drawing one page is; without a ceiling here a notebook could
/// name a page of `f64::MAX` points and the renderer would be asked for more shapes than there
/// is memory to hold. The export already refused anything past this, but only after storage had
/// accepted it, which is the wrong end to find out.
pub const MAX_PAGE_DIMENSION_PT: f64 = 100_000.0;

/// Finite and greater than zero. Shared by every measurement the store refuses to guess at:
/// a zero or NaN width is a corrupt page, not a small one.
pub(crate) fn positive(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

/// A page extent: positive, and small enough that resolving a template against it terminates.
pub(crate) fn valid_page_dimension(value: f64) -> bool {
    positive(value) && value <= MAX_PAGE_DIMENSION_PT
}

/// A colour as it appears in canonical notebook content: `#rrggbb`, or `#rrggbbaa` where the
/// content carries its own alpha.
///
/// One rule for everything stored in a notebook and everything drawn from it — the template
/// validator and the PDF export each had their own byte-identical copy, which is one edit away
/// from an export accepting a colour the store would have refused.
///
/// The desktop app's swatch rule is deliberately *not* this one: it takes `#rrggbb` only, because
/// highlighter translucency is a separate opacity setting there and alpha riding along inside a
/// colour string is how it would get silently dropped. That difference is a policy, so it is
/// stated where it is made rather than hidden behind a shared name.
pub fn valid_hex_color(color: &str) -> bool {
    matches!(color.len(), 7 | 9)
        && color.starts_with('#')
        && color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Finite and not negative. Distinct from [`positive`] because a measured extent of zero is a
/// legitimate empty result, while a width of zero is not.
pub(crate) fn nonnegative(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotebookManifest {
    pub schema_version: u32,
    pub id: String,
    pub title: String,
    pub pages: Vec<PageReference>,
    pub default_page: PageDefaults,
    pub shared_style_path: Option<String>,
    pub created_at: String,
    pub modified_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageReference {
    pub id: String,
    pub path: String,
    /// A layout hint, not the truth — the page file is authoritative.
    ///
    /// Pages load lazily, so the scroller has to reserve the right amount of room for a page it
    /// has not read yet. Without this it would either size every frame from one assumed geometry
    /// and jump as pages arrive, or read every page file to open a notebook, which is what lazy
    /// loading exists to avoid. Denormalising into the index is what an index is for.
    pub geometry: PageGeometry,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageDefaults {
    pub geometry: PageGeometry,
    pub background: PageBackground,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageGeometry {
    pub width_pt: f64,
    pub height_pt: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum PageBackground {
    Plain {
        color: String,
    },
    Pdf {
        source_path: String,
        page: u32,
    },
    /// Ruled, dotted, or squared paper. The definition is stored on the page rather than
    /// referenced by id, for the same reason a stroke stores its resolved nib parameters instead
    /// of a pen name: a notebook opened on a machine that never had the template still has to
    /// look like itself.
    Template {
        template: template::PageTemplate,
    },
}

impl PageBackground {
    /// The file this background reads, if it reads one. Same contract as
    /// [`PageObject::source`], so one helper checks both.
    pub fn source(&self) -> Option<SourceRef<'_>> {
        match self {
            Self::Pdf { source_path, .. } => Some(SourceRef {
                role: SourceRole::Reference,
                path: source_path,
            }),
            Self::Plain { .. } | Self::Template { .. } => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub schema_version: u32,
    pub id: String,
    pub revision: u64,
    pub geometry: PageGeometry,
    pub background: PageBackground,
    pub objects: Vec<PageObject>,
    pub reading_order: Vec<String>,
    pub ink_layers: Vec<InkLayerReference>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InkLayerReference {
    pub id: String,
    pub path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InkLayer {
    pub schema_version: u32,
    pub id: String,
    pub page_id: String,
    pub strokes: Vec<Stroke>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stroke {
    pub id: String,
    #[serde(default = "default_ink_z_index")]
    pub z_index: i32,
    pub tool: StrokeTool,
    pub color: String,
    pub width_pt: f64,
    /// Whether this stroke's width followed stylus pressure. Resolved from the nib when the
    /// stroke was drawn and then stored, rather than inferred from `tool`: nibs genuinely differ —
    /// a technical pen is deliberately even where a fountain pen swells — and a notebook has to
    /// render identically on a machine that has never seen the nib it was written with.
    pub pressure: bool,
    /// Fraction of the stroke's length over which each end tapers to a point; 0 disables it.
    pub taper: f64,
    /// Ink opacity, 0–1. Stored per stroke because a highlighter sweep is translucent and a pen
    /// is not, and because the setting can change after the stroke was laid down.
    pub opacity: f64,
    pub group_id: Option<String>,
    pub points: Vec<StrokePoint>,
    pub transform: Transform,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StrokeTool {
    Pen,
    Highlighter,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StrokePoint {
    pub x: f64,
    pub y: f64,
    pub pressure: f64,
    pub time_ms: f64,
    pub tilt_x: f64,
    pub tilt_y: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transform {
    pub translate_x: f64,
    pub translate_y: f64,
    pub scale_x: f64,
    pub scale_y: f64,
    pub rotation: f64,
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use serde::de::DeserializeOwned;

    use super::*;

    fn fixture_path(relative: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/notebooks/minimal")
            .join(relative)
    }

    fn load<T: DeserializeOwned>(relative: &str) -> T {
        serde_json::from_str(&fs::read_to_string(fixture_path(relative)).unwrap()).unwrap()
    }

    #[test]
    fn minimal_open_notebook_references_point_based_content() {
        let manifest: NotebookManifest = load("goodtype.json");
        assert_eq!(manifest.schema_version, SCHEMA_VERSION);
        assert_eq!(manifest.pages[0].path, "pages/page-001.json");
        assert!(fixture_path(&manifest.pages[0].path).is_file());
        assert!(fixture_path(manifest.shared_style_path.as_deref().unwrap()).is_file());

        let page: Page = load(&manifest.pages[0].path);
        assert_eq!(page.schema_version, SCHEMA_VERSION);
        assert_eq!(
            (page.geometry.width_pt, page.geometry.height_pt),
            (595.0, 842.0)
        );
        assert_eq!(page.revision, 1);
        assert_eq!(page.reading_order, ["group-001", "image-001", "pdf-001"]);
        assert_eq!(page.ink_layers[0].id, "ink-layer-001");
        assert!(fixture_path(&page.ink_layers[0].path).is_file());

        for object in &page.objects {
            assert!(
                page.reading_order.contains(&object.fields().id)
                    || object.fields().group_id.as_deref() == Some("group-001")
            );
        }

        let typst_source = page.objects.iter().find_map(|object| match object {
            PageObject::Typst { source_path, .. } => Some(source_path),
            _ => None,
        });
        let image_source = page.objects.iter().find_map(|object| match object {
            PageObject::Image { source_path, .. } => Some(source_path),
            _ => None,
        });
        let pdf_source = page.objects.iter().find_map(|object| match object {
            PageObject::PdfMaterial { source_path, .. } => Some(source_path),
            _ => None,
        });
        for source in [typst_source, image_source, pdf_source] {
            assert!(fixture_path(source.unwrap()).is_file());
        }

        let ink: InkLayer = load(&page.ink_layers[0].path);
        assert_eq!(
            (ink.schema_version, ink.page_id.as_str()),
            (SCHEMA_VERSION, "page-001")
        );
        assert_eq!(ink.strokes[0].group_id.as_deref(), Some("ink-group-001"));
        assert_eq!(ink.strokes[0].z_index, DEFAULT_INK_Z_INDEX);
        assert_eq!(ink.strokes[0].points[1].pressure, 0.75);
        assert_eq!(ink.strokes[0].points[1].time_ms, 8.0);
        assert_eq!(
            (
                ink.strokes[0].points[1].tilt_x,
                ink.strokes[0].points[1].tilt_y
            ),
            (4.0, -2.0)
        );
        assert_eq!(
            (
                ink.strokes[0].transform.scale_x,
                ink.strokes[0].transform.scale_y
            ),
            (1.0, 1.0)
        );
    }

    /// A stroke carries the nib parameters it was drawn with, so the export never has to guess
    /// them back from the tool — that guess is how pressure drifted out of the PDF before.
    #[test]
    fn strokes_carry_the_nib_parameters_they_were_drawn_with() {
        let page: Page = load("pages/page-001.json");
        let ink: InkLayer = load(&page.ink_layers[0].path);
        assert!(ink.strokes[0].pressure);
        assert_eq!(ink.strokes[0].taper, 0.12);
    }
}
