//! What makes a snapshot a notebook rather than a pile of JSON.
//!
//! Nearly pure: these functions reach the filesystem only to confirm that a referenced file
//! resolves. Every rule that can be checked in memory is checked before anything is written, so a
//! refused commit leaves the previous revision exactly as it was.

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};

use crate::{
    MIN_SUPPORTED_SCHEMA_VERSION, NotebookManifest, Page, PageBackground, PageObject,
    PageReference, SCHEMA_VERSION, SHAPE_SCHEMA_VERSION, SourceRef, SourceRole, layout,
    nonnegative, positive, valid_page_dimension,
};

use super::{
    MAX_INK_BYTES, MAX_INK_POINTS_PER_LAYER, MAX_INK_STROKES_PER_LAYER, MAX_JSON_BYTES,
    NotebookSnapshot, StorageError, StoredFile, files::json_bytes, invalid, invalid_error,
    paths::*,
};

/// The schema version a page's content obliges its notebook to be written at.
///
/// A notebook is not upgraded for being opened, nor for being saved after an unrelated edit. It
/// rises to a version only when it actually stores something that version introduced, so a
/// version-1 notebook stays readable by an older build right up until it gains its first shape.
pub(crate) fn required_schema_version(page: &Page) -> u32 {
    if page
        .objects
        .iter()
        .any(|object| matches!(object, PageObject::Shape { .. }))
    {
        SHAPE_SCHEMA_VERSION
    } else {
        MIN_SUPPORTED_SCHEMA_VERSION
    }
}

pub(crate) fn validate_manifest(manifest: &NotebookManifest) -> Result<(), StorageError> {
    supported_version(manifest.schema_version, "schema")?;
    if manifest.pages.is_empty() {
        return invalid("notebook must contain at least one page");
    }
    let mut ids = HashSet::new();
    let mut paths = HashSet::new();
    for page in &manifest.pages {
        if page.id.is_empty() || !ids.insert(page.id.as_str()) || !paths.insert(page.path.as_str())
        {
            return invalid("manifest page IDs and paths must be non-empty and unique");
        }
        validate_relative(&page.path)?;
        if !is_in_directory(&page.path, layout::PAGES_DIR) {
            return Err(StorageError::InvalidPath(page.path.clone()));
        }
    }
    if let Some(style) = &manifest.shared_style_path {
        validate_relative(style)?;
    }
    Ok(())
}

/// Raise a snapshot to the version its own content requires, and stamp that version through the
/// page and its ink layers so the notebook stays internally consistent.
///
/// Only ever upwards: a page that loses its last shape does not drag the notebook back down,
/// because a sibling page this snapshot cannot see may still hold one.
pub(crate) fn raise_schema_version(snapshot: &mut NotebookSnapshot) {
    let version = snapshot
        .manifest
        .schema_version
        .max(required_schema_version(&snapshot.page));
    snapshot.manifest.schema_version = version;
    snapshot.page.schema_version = version;
    for layer in &mut snapshot.ink_layers {
        layer.schema_version = version;
    }
}

/// One notebook, one version: the manifest states it, and the page and its ink layers must agree.
///
/// Checked on the way in as well as the way out, because a snapshot that arrives with mismatched
/// versions is corrupt whether it came from disk, from a recovery intent, or from the app.
pub(crate) fn validate_snapshot_versions(snapshot: &NotebookSnapshot) -> Result<(), StorageError> {
    let version = snapshot.manifest.schema_version;
    supported_version(version, "schema")?;
    if snapshot.page.schema_version != version {
        return Err(StorageError::InvalidNotebook(format!(
            "page schema version {} does not match notebook version {version}",
            snapshot.page.schema_version
        )));
    }
    for layer in &snapshot.ink_layers {
        if layer.schema_version != version {
            return Err(StorageError::InvalidNotebook(format!(
                "ink schema version {} does not match notebook version {version}",
                layer.schema_version
            )));
        }
    }
    let required = required_schema_version(&snapshot.page);
    if version < required {
        return Err(StorageError::InvalidNotebook(format!(
            "this page needs schema version {required}, but the notebook is version {version}"
        )));
    }
    Ok(())
}

fn supported_version(version: u32, subject: &str) -> Result<(), StorageError> {
    if (MIN_SUPPORTED_SCHEMA_VERSION..=SCHEMA_VERSION).contains(&version) {
        Ok(())
    } else {
        Err(StorageError::InvalidNotebook(format!(
            "unsupported {subject} version {version}"
        )))
    }
}

pub(crate) fn validate_manifest_files(
    root: &Path,
    manifest: &NotebookManifest,
) -> Result<(), StorageError> {
    for page in &manifest.pages {
        resolve_existing(root, &page.path)?;
    }
    Ok(())
}

pub(crate) fn page_reference<'a>(
    manifest: &'a NotebookManifest,
    page_id: &str,
) -> Result<&'a PageReference, StorageError> {
    manifest
        .pages
        .iter()
        .find(|page| page.id == page_id)
        .ok_or_else(|| invalid_error("page is not referenced by the notebook manifest"))
}

pub(crate) fn validate_snapshot(
    root: &Path,
    snapshot: &NotebookSnapshot,
) -> Result<(), StorageError> {
    validate_manifest(&snapshot.manifest)?;
    json_bytes(&snapshot.manifest, true, MAX_JSON_BYTES)?;
    prepare_target(root, layout::MANIFEST)?;
    let page_reference = page_reference(&snapshot.manifest, &snapshot.page.id)?;
    prepare_target(root, &page_reference.path)?;
    validate_snapshot_versions(snapshot)?;
    validate_page_content(snapshot)?;
    json_bytes(&snapshot.page, true, MAX_JSON_BYTES)?;
    for layer in &snapshot.ink_layers {
        json_bytes(layer, false, MAX_INK_BYTES)?;
    }

    let block_paths = referenced_paths(&snapshot.manifest, &snapshot.page, SourceRole::Block);
    let asset_paths = referenced_paths(&snapshot.manifest, &snapshot.page, SourceRole::Asset);
    validate_files(root, &snapshot.blocks, &block_paths, SourceRole::Block)?;
    validate_files(root, &snapshot.assets, &asset_paths, SourceRole::Asset)?;
    validate_background(root, &snapshot.manifest.default_page.background)?;
    validate_background(root, &snapshot.page.background)?;

    // Material the store only reads still has to resolve, or the page renders a hole.
    for object in &snapshot.page.objects {
        if let Some(source) = object.source()
            && !source.role.is_written()
        {
            validate_source_path(root, source)?;
        }
    }

    if snapshot.ink_layers.len() != snapshot.page.ink_layers.len() {
        return Err(StorageError::InvalidNotebook(
            "ink layer references and payloads differ".into(),
        ));
    }
    for reference in &snapshot.page.ink_layers {
        validate_relative(&reference.path)?;
        if !is_in_directory(&reference.path, layout::INK_DIR) {
            return Err(StorageError::InvalidPath(reference.path.clone()));
        }
        prepare_target(root, &reference.path)?;
        let matches = snapshot
            .ink_layers
            .iter()
            .filter(|layer| layer.id == reference.id && layer.page_id == snapshot.page.id)
            .count();
        if matches != 1 {
            return Err(StorageError::InvalidNotebook(format!(
                "ink layer {} does not match its reference",
                reference.id
            )));
        }
    }
    Ok(())
}

fn validate_page_content(snapshot: &NotebookSnapshot) -> Result<(), StorageError> {
    let page = &snapshot.page;
    // Bounded above as well as below: page size is what decides how many shapes a template
    // resolves into, so an unbounded page is an unbounded amount of drawing work.
    if page.id.is_empty()
        || !valid_page_dimension(page.geometry.width_pt)
        || !valid_page_dimension(page.geometry.height_pt)
    {
        return invalid("page ID and geometry must be valid");
    }

    let mut objects = HashMap::new();
    for object in &page.objects {
        let fields = object.fields();
        if fields.id.is_empty()
            || objects.insert(fields.id.as_str(), object).is_some()
            || !fields.x.is_finite()
            || !fields.y.is_finite()
            || !fields.rotation.is_finite()
            || !positive(fields.scale)
        {
            return invalid("object IDs must be unique and transforms must be finite");
        }
        object.validate_dimensions().map_err(invalid_error)?;
    }
    if page
        .objects
        .iter()
        .filter(|object| matches!(object, PageObject::PageTypst { .. }))
        .count()
        > 1
    {
        return invalid("a page may contain only one page Typst surface");
    }

    let top_level = page
        .objects
        .iter()
        .filter(|object| object.fields().group_id.is_none())
        .map(|object| object.fields().id.as_str())
        .collect::<HashSet<_>>();
    let reading_order = page
        .reading_order
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    if reading_order.len() != page.reading_order.len() || reading_order != top_level {
        return invalid("reading order must contain every top-level object exactly once");
    }
    for (index, id) in page.reading_order.iter().enumerate() {
        if objects[id.as_str()].fields().reading_order != index as u32 {
            return invalid("object reading-order values must match the page reading order");
        }
    }

    validate_groups(&objects)?;
    validate_ink(snapshot, &objects)
}

fn validate_groups(objects: &HashMap<&str, &PageObject>) -> Result<(), StorageError> {
    for object in objects.values() {
        if let PageObject::Group { fields, child_ids } = object {
            let children = child_ids.iter().map(String::as_str).collect::<HashSet<_>>();
            if children.is_empty() || children.len() != child_ids.len() {
                return invalid("groups must contain unique child IDs");
            }
            for child_id in child_ids {
                let child = objects
                    .get(child_id.as_str())
                    .ok_or_else(|| invalid_error("group child does not exist"))?;
                if child.fields().group_id.as_deref() != Some(fields.id.as_str()) {
                    return invalid("group membership must agree in parent and child");
                }
            }
        }

        let mut seen = HashSet::new();
        let mut current = object.fields();
        while let Some(parent_id) = current.group_id.as_deref() {
            if !seen.insert(parent_id) {
                return invalid("groups cannot contain a cycle");
            }
            let parent = objects
                .get(parent_id)
                .ok_or_else(|| invalid_error("group parent does not exist"))?;
            if !matches!(parent, PageObject::Group { .. }) {
                return invalid("group parent must reference a group object");
            }
            current = parent.fields();
        }
    }
    Ok(())
}

pub(crate) fn validate_ink(
    snapshot: &NotebookSnapshot,
    objects: &HashMap<&str, &PageObject>,
) -> Result<(), StorageError> {
    let reference_ids = snapshot
        .page
        .ink_layers
        .iter()
        .map(|reference| reference.id.as_str())
        .collect::<HashSet<_>>();
    let reference_paths = snapshot
        .page
        .ink_layers
        .iter()
        .map(|reference| reference.path.as_str())
        .collect::<HashSet<_>>();
    if reference_ids.len() != snapshot.page.ink_layers.len()
        || reference_paths.len() != snapshot.page.ink_layers.len()
    {
        return invalid("ink-layer references must be unique");
    }

    let mut layers = HashMap::new();
    let mut strokes = HashMap::new();
    for layer in &snapshot.ink_layers {
        if layer.id.is_empty() || layers.insert(layer.id.as_str(), layer).is_some() {
            return invalid("ink-layer IDs must be non-empty and unique");
        }
        // Refuse an oversized layer here, before `save_to_root` writes anything. Relying on the
        // read ceiling alone would let a commit write a layer it can never read back, which
        // leaves the notebook unopenable.
        if layer.strokes.len() > MAX_INK_STROKES_PER_LAYER {
            return Err(StorageError::InvalidNotebook(format!(
                "ink layer {} holds {} strokes; maximum is {MAX_INK_STROKES_PER_LAYER}",
                layer.id,
                layer.strokes.len(),
            )));
        }
        let points = layer
            .strokes
            .iter()
            .map(|stroke| stroke.points.len())
            .sum::<usize>();
        if points > MAX_INK_POINTS_PER_LAYER {
            return Err(StorageError::InvalidNotebook(format!(
                "ink layer {} holds {points} samples; maximum is {MAX_INK_POINTS_PER_LAYER}",
                layer.id,
            )));
        }
        for stroke in &layer.strokes {
            if stroke.id.is_empty()
                || strokes
                    .insert(stroke.id.as_str(), (layer.id.as_str(), stroke))
                    .is_some()
                || !positive(stroke.width_pt)
                || !(0.0..=1.0).contains(&stroke.taper)
                || !(0.0..=1.0).contains(&stroke.opacity)
                || stroke.points.is_empty()
                || !positive(stroke.transform.scale_x)
                || !positive(stroke.transform.scale_y)
                || !stroke.transform.translate_x.is_finite()
                || !stroke.transform.translate_y.is_finite()
                || !stroke.transform.rotation.is_finite()
            {
                return invalid("stroke IDs, points, widths, tapers, and transforms must be valid");
            }
            if stroke.points.iter().any(|point| {
                !point.x.is_finite()
                    || !point.y.is_finite()
                    || !(0.0..=1.0).contains(&point.pressure)
                    || !nonnegative(point.time_ms)
                    || !(-90.0..=90.0).contains(&point.tilt_x)
                    || !(-90.0..=90.0).contains(&point.tilt_y)
            }) {
                return invalid("stroke samples must be finite and calibrated");
            }
        }
    }

    for object in objects.values() {
        if let PageObject::InkGroup {
            fields,
            ink_layer_id,
            stroke_ids,
        } = object
        {
            if !layers.contains_key(ink_layer_id.as_str()) {
                return invalid("ink group references an unknown layer");
            }
            let unique = stroke_ids
                .iter()
                .map(String::as_str)
                .collect::<HashSet<_>>();
            if unique.is_empty() || unique.len() != stroke_ids.len() {
                return invalid("ink groups must contain unique stroke IDs");
            }
            for stroke_id in stroke_ids {
                let Some((layer_id, stroke)) = strokes.get(stroke_id.as_str()) else {
                    return invalid("ink group references an unknown stroke");
                };
                if *layer_id != ink_layer_id || stroke.group_id.as_deref() != Some(&fields.id) {
                    return invalid("ink group membership must agree in object and stroke");
                }
            }
        }
    }

    for (layer_id, stroke) in strokes.values() {
        if let Some(group_id) = stroke.group_id.as_deref() {
            let Some(PageObject::InkGroup {
                ink_layer_id,
                stroke_ids,
                ..
            }) = objects.get(group_id)
            else {
                return invalid("stroke group does not exist");
            };
            if ink_layer_id != layer_id || !stroke_ids.iter().any(|id| id == &stroke.id) {
                return invalid("stroke group membership is incomplete");
            }
        }
    }
    Ok(())
}

fn validate_background(root: &Path, background: &PageBackground) -> Result<(), StorageError> {
    if let Some(source) = background.source() {
        validate_source_path(root, source)?;
    }
    // Checked on the way in so that anything already on disk can be resolved and drawn
    // without a renderer having to second-guess it.
    if let PageBackground::Template { template } = background {
        crate::template::validate(template).map_err(invalid_error)?;
    }
    Ok(())
}

fn validate_files(
    root: &Path,
    files: &[StoredFile],
    expected: &[&str],
    role: SourceRole,
) -> Result<(), StorageError> {
    for (index, file) in files.iter().enumerate() {
        validate_stored_path(&file.path, role)?;
        if !expected.contains(&file.path.as_str())
            || files[index + 1..]
                .iter()
                .any(|other| other.path == file.path)
        {
            return Err(StorageError::InvalidNotebook(format!(
                "unexpected or duplicate file {}",
                file.path
            )));
        }
        let target = prepare_target(root, &file.path)?;
        if role.is_written() && file.bytes.len() > role.max_bytes() {
            if role == SourceRole::Asset {
                return Err(StorageError::ImageTooLarge {
                    size: file.bytes.len(),
                    maximum: role.max_bytes(),
                });
            }
            return Err(StorageError::InvalidNotebook(format!(
                "{} is {} bytes; maximum is {}",
                file.path,
                file.bytes.len(),
                role.max_bytes()
            )));
        }
        if role == SourceRole::Block && std::str::from_utf8(&file.bytes).is_err() {
            return invalid("Typst source must be valid UTF-8");
        }
        // Content that cannot be rewritten has to be identical to whatever is already there,
        // because every page sharing the path would otherwise change with it.
        if !role.is_rewritable() {
            if file.bytes.is_empty() {
                return Err(StorageError::ImageTooLarge {
                    size: file.bytes.len(),
                    maximum: role.max_bytes(),
                });
            }
            if target.exists() && fs::read(target)? != file.bytes {
                return Err(StorageError::AlreadyExists(file.path.clone()));
            }
        }
    }
    for path in expected {
        validate_stored_path(path, role)?;
        if !files.iter().any(|file| file.path == *path) {
            resolve_existing(root, path)?;
        }
    }
    Ok(())
}

/// Where a file in this role is allowed to sit. The shared style is the one exception: it
/// belongs to the notebook rather than to any page, so it lives at the root.
pub(crate) fn validate_stored_path(path: &str, role: SourceRole) -> Result<(), StorageError> {
    validate_relative(path)?;
    match role {
        SourceRole::Asset => validate_asset_path(path),
        SourceRole::Block if path == layout::SHARED_STYLE => Ok(()),
        SourceRole::Block | SourceRole::Reference => {
            if is_in_directory(path, role.directory()) {
                Ok(())
            } else {
                Err(StorageError::InvalidPath(path.into()))
            }
        }
    }
}

/// A file the store reads but never writes: it has to sit in its role's directory and already
/// exist. One check for imported page material and for a PDF background.
fn validate_source_path(root: &Path, source: SourceRef<'_>) -> Result<(), StorageError> {
    validate_stored_path(source.path, source.role)?;
    resolve_existing(root, source.path)?;
    Ok(())
}

/// Every file the page owns in one role, in reading order and deduplicated.
///
/// The shared style belongs to the manifest rather than to any one object, so it leads the
/// block list; everything else comes from the objects themselves via [`PageObject::source`].
pub(crate) fn referenced_paths<'a>(
    manifest: &'a NotebookManifest,
    page: &'a Page,
    role: SourceRole,
) -> Vec<&'a str> {
    let mut paths = Vec::new();
    if role == SourceRole::Block
        && let Some(path) = manifest.shared_style_path.as_deref()
    {
        paths.push(path);
    }
    for object in &page.objects {
        if let Some(source) = object.source()
            && source.role == role
            && !paths.contains(&source.path)
        {
            paths.push(source.path);
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use crate::{
        PageObject, layout,
        storage::{fixtures::*, *},
    };

    #[test]
    fn rejects_inconsistent_canonical_page_content() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();

        let mut duplicate_id = open_notebook(&notebook_root).unwrap();
        let PageObject::Image { fields, .. } = &mut duplicate_id.page.objects[1] else {
            panic!("image object missing");
        };
        fields.id = "typst-001".into();
        assert!(matches!(
            save_notebook(&notebook_root, &duplicate_id),
            Err(StorageError::InvalidNotebook(_))
        ));

        let mut bad_order = open_notebook(&notebook_root).unwrap();
        bad_order.page.reading_order.pop();
        assert!(matches!(
            save_notebook(&notebook_root, &bad_order),
            Err(StorageError::InvalidNotebook(_))
        ));

        let mut bad_ink = open_notebook(&notebook_root).unwrap();
        bad_ink.ink_layers[0].strokes[0].points[0].pressure = 1.5;
        assert!(matches!(
            save_notebook(&notebook_root, &bad_ink),
            Err(StorageError::InvalidNotebook(_))
        ));
    }

    #[test]
    fn accepts_one_fixed_page_typst_surface_and_rejects_two() {
        let mut snapshot = snapshot();
        let mut page_fields = fields("page-text-1", 2);
        page_fields.x = 0.0;
        page_fields.y = 0.0;
        snapshot.page.objects.push(PageObject::PageTypst {
            fields: page_fields.clone(),
            source_path: "blocks/page.typ".into(),
        });
        snapshot.page.reading_order.push("page-text-1".into());
        assert!(super::validate_page_content(&snapshot).is_ok());

        page_fields.id = "page-text-2".into();
        page_fields.reading_order = 3;
        snapshot.page.objects.push(PageObject::PageTypst {
            fields: page_fields,
            source_path: "blocks/page-2.typ".into(),
        });
        snapshot.page.reading_order.push("page-text-2".into());
        assert!(matches!(
            super::validate_page_content(&snapshot),
            Err(StorageError::InvalidNotebook(_))
        ));
    }

    /// An over-budget layer must be refused before anything reaches disk, so the previous
    /// confirmed revision stays openable.
    #[test]
    fn refuses_an_oversized_ink_layer_without_writing_it() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();

        let mut history = NotebookHistory::default();
        let mut oversized = observe_notebook(&notebook_root, &mut history).unwrap();
        oversized.ink_layers = vec![handwriting_layer(MAX_INK_STROKES_PER_LAYER + 1, 1)];

        let error = commit_notebook(&notebook_root, &mut history, oversized).unwrap_err();
        assert!(matches!(error, StorageError::InvalidNotebook(_)));

        let confirmed = open_notebook(&notebook_root).unwrap();
        assert_eq!(confirmed.page.revision, 1);
        assert_eq!(confirmed.ink_layers[0].strokes.len(), 1);
        assert!(!notebook_root.join(layout::PENDING_TRANSACTION).exists());
    }

    #[test]
    fn refuses_unreadable_or_oversized_typst_before_writing() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();

        let mut oversized = open_notebook(&notebook_root).unwrap();
        oversized.blocks[0].bytes = vec![b'x'; crate::object::MAX_BLOCK_BYTES + 1];
        assert!(matches!(
            save_notebook(&notebook_root, &oversized),
            Err(StorageError::InvalidNotebook(_))
        ));

        let mut invalid_utf8 = open_notebook(&notebook_root).unwrap();
        invalid_utf8.blocks[0].bytes = vec![0xff];
        assert!(matches!(
            save_notebook(&notebook_root, &invalid_utf8),
            Err(StorageError::InvalidNotebook(_))
        ));
        assert_eq!(open_notebook(&notebook_root).unwrap().page.revision, 1);
    }
}
