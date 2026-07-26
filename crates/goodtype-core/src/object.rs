//! Page objects and the questions the store asks about them.
//!
//! Every variant-specific decision lives here as an exhaustive `match`, so adding a kind of
//! object is a compile error at each place that has to know about it. The store asks; it never
//! decides on an object's behalf.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{nonnegative, positive};

/// Where a file an object owns lives, and how the store may treat it.
///
/// This is the answer to four questions the store used to ask by matching on the variant:
/// which directory the file belongs in, whether the store writes it, whether it may be
/// rewritten, and whether it takes part in the change fingerprint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceRole {
    /// Typst source. The store rewrites it on every commit, so it is fingerprinted.
    Block,
    /// Pasted bytes. Written once and shared by reference from then on — rewriting one would
    /// silently change every page that points at it.
    Asset,
    /// Imported material the store only ever reads.
    Reference,
}

/// Typst source is hand-written structure; a megabyte is already far past anything a person
/// types into one block.
pub const MAX_BLOCK_BYTES: usize = 1024 * 1024;
/// Pasted images. Large enough for a photograph, small enough that one paste cannot make a
/// notebook unopenable.
pub const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;

impl SourceRole {
    /// The one directory a file in this role may live in.
    pub fn directory(self) -> &'static str {
        match self {
            Self::Block => "blocks",
            Self::Asset => "assets",
            Self::Reference => "references",
        }
    }

    /// The most the store will read or write for a file in this role.
    ///
    /// A ceiling belongs to the kind of content, not to the call site: every place that touches
    /// a block wants the block ceiling, and getting that wrong once is how a file gets written
    /// that can never be read back.
    pub const fn max_bytes(self) -> usize {
        match self {
            Self::Block => MAX_BLOCK_BYTES,
            // Imported material is resolved rather than loaded, so this ceiling is what would
            // apply if it ever were — deliberately the conservative one.
            Self::Asset | Self::Reference => MAX_IMAGE_BYTES,
        }
    }

    /// Whether the store writes files in this role at all, or only reads what is already there.
    pub fn is_written(self) -> bool {
        match self {
            Self::Block | Self::Asset => true,
            Self::Reference => false,
        }
    }

    /// Whether an existing file in this role may be replaced.
    pub fn is_rewritable(self) -> bool {
        match self {
            Self::Block => true,
            Self::Asset | Self::Reference => false,
        }
    }

    /// Whether a change to this file has to invalidate a snapshot a caller is holding. Only the
    /// files the store itself rewrites qualify: write-once content cannot change under a reader.
    pub fn is_fingerprinted(self) -> bool {
        match self {
            Self::Block => true,
            Self::Asset | Self::Reference => false,
        }
    }
}

/// One file an object owns, and the role that decides how it is handled.
#[derive(Clone, Copy, Debug)]
pub struct SourceRef<'a> {
    pub role: SourceRole,
    pub path: &'a str,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectFields {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub rotation: f64,
    pub scale: f64,
    pub z_index: i32,
    pub reading_order: u32,
    pub group_id: Option<String>,
    pub created_at: String,
    pub modified_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum PageObject {
    Typst {
        #[serde(flatten)]
        fields: ObjectFields,
        source_path: String,
        layout_width_pt: f64,
        measured_width_pt: f64,
        measured_height_pt: f64,
    },
    Image {
        #[serde(flatten)]
        fields: ObjectFields,
        source_path: String,
        width_pt: f64,
        height_pt: f64,
        alt_text: String,
    },
    PdfMaterial {
        #[serde(flatten)]
        fields: ObjectFields,
        source_path: String,
        page: u32,
        source_width_pt: f64,
        source_height_pt: f64,
    },
    InkGroup {
        #[serde(flatten)]
        fields: ObjectFields,
        ink_layer_id: String,
        stroke_ids: Vec<String>,
    },
    Group {
        #[serde(flatten)]
        fields: ObjectFields,
        child_ids: Vec<String>,
    },
}

impl PageObject {
    pub fn fields(&self) -> &ObjectFields {
        match self {
            Self::Typst { fields, .. }
            | Self::Image { fields, .. }
            | Self::PdfMaterial { fields, .. }
            | Self::InkGroup { fields, .. }
            | Self::Group { fields, .. } => fields,
        }
    }

    pub fn fields_mut(&mut self) -> &mut ObjectFields {
        match self {
            Self::Typst { fields, .. }
            | Self::Image { fields, .. }
            | Self::PdfMaterial { fields, .. }
            | Self::InkGroup { fields, .. }
            | Self::Group { fields, .. } => fields,
        }
    }

    /// The file this object owns, if it owns one.
    ///
    /// Collecting blocks, collecting assets, checking that imported material resolves, choosing
    /// a size ceiling, and deciding what the fingerprint covers all read this one answer.
    pub fn source(&self) -> Option<SourceRef<'_>> {
        match self {
            Self::Typst { source_path, .. } => Some(SourceRef {
                role: SourceRole::Block,
                path: source_path,
            }),
            Self::Image { source_path, .. } => Some(SourceRef {
                role: SourceRole::Asset,
                path: source_path,
            }),
            Self::PdfMaterial { source_path, .. } => Some(SourceRef {
                role: SourceRole::Reference,
                path: source_path,
            }),
            Self::InkGroup { .. } | Self::Group { .. } => None,
        }
    }

    /// The file whose text notebook search should read, if this object has one.
    ///
    /// Deliberately not derived from [`Self::source`]: owning a block file and holding prose a
    /// reader would search for are different claims, and a new variant has to make both.
    pub fn searchable_source(&self) -> Option<&str> {
        match self {
            Self::Typst { source_path, .. } => Some(source_path),
            Self::Image { .. }
            | Self::PdfMaterial { .. }
            | Self::InkGroup { .. }
            | Self::Group { .. } => None,
        }
    }

    /// Variant-specific numeric invariants. The shared transform is checked by the validator;
    /// this covers only what one kind of object means by its own measurements.
    pub fn validate_dimensions(&self) -> Result<(), &'static str> {
        match self {
            Self::Typst {
                layout_width_pt,
                measured_width_pt,
                measured_height_pt,
                ..
            } => {
                if !positive(*layout_width_pt)
                    || !nonnegative(*measured_width_pt)
                    || !nonnegative(*measured_height_pt)
                {
                    return Err("Typst dimensions must be finite and non-negative");
                }
            }
            Self::Image {
                width_pt,
                height_pt,
                ..
            } => {
                if !positive(*width_pt) || !positive(*height_pt) {
                    return Err("image dimensions must be finite and positive");
                }
            }
            Self::PdfMaterial {
                source_width_pt,
                source_height_pt,
                ..
            } => {
                if !positive(*source_width_pt) || !positive(*source_height_pt) {
                    return Err("PDF dimensions must be finite and positive");
                }
            }
            Self::InkGroup { .. } | Self::Group { .. } => {}
        }
        Ok(())
    }

    /// Rebuild this object with every identity it owns replaced, for page duplication.
    ///
    /// Shared originals keep their path: [`IdRemap::source`] passes through anything that was
    /// never given a replacement.
    pub fn remapped(&self, remap: &IdRemap) -> Result<Self, &'static str> {
        let mut fields = self.fields().clone();
        fields.id = remap.object(&fields.id)?;
        fields.group_id = remap.optional_object(fields.group_id.as_deref())?;
        fields.created_at = remap.timestamp().to_owned();
        fields.modified_at = remap.timestamp().to_owned();

        Ok(match self {
            Self::Typst {
                source_path,
                layout_width_pt,
                measured_width_pt,
                measured_height_pt,
                ..
            } => Self::Typst {
                fields,
                source_path: remap.source(source_path),
                layout_width_pt: *layout_width_pt,
                measured_width_pt: *measured_width_pt,
                measured_height_pt: *measured_height_pt,
            },
            Self::Image {
                source_path,
                width_pt,
                height_pt,
                alt_text,
                ..
            } => Self::Image {
                fields,
                source_path: remap.source(source_path),
                width_pt: *width_pt,
                height_pt: *height_pt,
                alt_text: alt_text.clone(),
            },
            Self::PdfMaterial {
                source_path,
                page,
                source_width_pt,
                source_height_pt,
                ..
            } => Self::PdfMaterial {
                fields,
                source_path: remap.source(source_path),
                page: *page,
                source_width_pt: *source_width_pt,
                source_height_pt: *source_height_pt,
            },
            Self::InkGroup {
                ink_layer_id,
                stroke_ids,
                ..
            } => Self::InkGroup {
                fields,
                ink_layer_id: remap.ink_layer(ink_layer_id)?,
                stroke_ids: stroke_ids
                    .iter()
                    .map(|id| remap.stroke(id))
                    .collect::<Result<Vec<_>, _>>()?,
            },
            Self::Group { child_ids, .. } => Self::Group {
                fields,
                child_ids: child_ids
                    .iter()
                    .map(|id| remap.object(id))
                    .collect::<Result<Vec<_>, _>>()?,
            },
        })
    }
}

/// Every identity a duplicated page has to replace, in one lookup table.
///
/// Duplication used to build five separate maps and consult them inline; keeping them together
/// means a remap that was never registered fails in one place with one message, instead of
/// resolving to a stale ID that only shows up as a broken page later.
#[derive(Debug, Default)]
pub struct IdRemap {
    objects: HashMap<String, String>,
    ink_layers: HashMap<String, String>,
    ink_layer_paths: HashMap<String, String>,
    sources: HashMap<String, String>,
    strokes: HashMap<String, String>,
    timestamp: String,
}

impl IdRemap {
    pub fn new(timestamp: impl Into<String>) -> Self {
        Self {
            timestamp: timestamp.into(),
            ..Self::default()
        }
    }

    pub fn timestamp(&self) -> &str {
        &self.timestamp
    }

    pub fn map_object(&mut self, old: impl Into<String>, new: impl Into<String>) {
        self.objects.insert(old.into(), new.into());
    }

    pub fn map_ink_layer(
        &mut self,
        old_id: impl Into<String>,
        new_id: impl Into<String>,
        old_path: impl Into<String>,
        new_path: impl Into<String>,
    ) {
        self.ink_layers.insert(old_id.into(), new_id.into());
        self.ink_layer_paths
            .insert(old_path.into(), new_path.into());
    }

    pub fn map_source(&mut self, old: impl Into<String>, new: impl Into<String>) {
        self.sources.insert(old.into(), new.into());
    }

    pub fn map_stroke(&mut self, old: impl Into<String>, new: impl Into<String>) {
        self.strokes.insert(old.into(), new.into());
    }

    pub fn has_source(&self, path: &str) -> bool {
        self.sources.contains_key(path)
    }

    pub fn object(&self, id: &str) -> Result<String, &'static str> {
        self.objects
            .get(id)
            .cloned()
            .ok_or("page references an unknown object")
    }

    pub fn optional_object(&self, id: Option<&str>) -> Result<Option<String>, &'static str> {
        id.map(|id| self.object(id)).transpose()
    }

    pub fn ink_layer(&self, id: &str) -> Result<String, &'static str> {
        self.ink_layers
            .get(id)
            .cloned()
            .ok_or("page references an unknown ink layer")
    }

    pub fn ink_layer_path(&self, path: &str) -> Result<String, &'static str> {
        self.ink_layer_paths
            .get(path)
            .cloned()
            .ok_or("page references an unknown ink layer path")
    }

    pub fn stroke(&self, id: &str) -> Result<String, &'static str> {
        self.strokes
            .get(id)
            .cloned()
            .ok_or("ink group references an unknown stroke")
    }

    /// A source with no replacement is a shared original and keeps its canonical path.
    pub fn source(&self, path: &str) -> String {
        self.sources
            .get(path)
            .cloned()
            .unwrap_or_else(|| path.to_owned())
    }
}
