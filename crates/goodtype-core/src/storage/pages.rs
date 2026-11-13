//! Opening a notebook, and the operations that change which pages it has.
//!
//! [`ManifestGuard`] is what makes those operations safe to interleave with another writer: it
//! records the manifest bytes a decision was made from, and refuses to commit if they moved.

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    IdRemap, InkLayer, InkLayerReference, NotebookManifest, Page, PageBackground, PageDefaults,
    PageGeometry, PageReference, SourceRole, layout,
};

use super::{
    HISTORY_LIMIT, MAX_IMAGE_BYTES, MAX_JSON_BYTES, NotebookSnapshot, StorageError, StoredFile,
    files::*, invalid, invalid_error, paths::*, recovery::*, validate::*, write::*,
};

pub fn create_notebook(
    selected_root: &Path,
    snapshot: &NotebookSnapshot,
) -> Result<(), StorageError> {
    fs::create_dir_all(selected_root)?;
    let root = canonical_root(selected_root)?;
    if root.join(layout::MANIFEST).exists() {
        return Err(StorageError::AlreadyExists(layout::MANIFEST.into()));
    }
    save_to_root(&root, snapshot).map(|_| ())
}

pub fn save_notebook(
    selected_root: &Path,
    snapshot: &NotebookSnapshot,
) -> Result<(), StorageError> {
    let root = canonical_root(selected_root)?;
    if resolve_existing(&root, layout::MANIFEST).is_err() {
        return Err(StorageError::InvalidNotebook(
            "goodtype.json does not exist".into(),
        ));
    }
    save_to_root(&root, snapshot).map(|_| ())
}

pub fn open_notebook(selected_root: &Path) -> Result<NotebookSnapshot, StorageError> {
    let root = canonical_root(selected_root)?;
    recover_interrupted_transaction(&root)?;
    let manifest: NotebookManifest = read_json(&root, layout::MANIFEST)?;
    validate_manifest(&manifest)?;
    validate_manifest_files(&root, &manifest)?;
    let page_id = manifest
        .pages
        .first()
        .expect("validated non-empty page list")
        .id
        .clone();
    load_page(&root, manifest, &page_id)
}

pub fn open_page(selected_root: &Path, page_id: &str) -> Result<NotebookSnapshot, StorageError> {
    let root = canonical_root(selected_root)?;
    recover_interrupted_transaction(&root)?;
    let manifest: NotebookManifest = read_json(&root, layout::MANIFEST)?;
    validate_manifest(&manifest)?;
    validate_manifest_files(&root, &manifest)?;
    load_page(&root, manifest, page_id)
}

/// A calculation owns cell order. Only parts still referenced by the manifest participate.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalculationStep {
    pub page_id: String,
    pub cell_id: String,
    pub part_id: String,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalculationExecutionPlan {
    pub revision: u64,
    pub steps: Vec<CalculationStep>,
}

pub fn calculation_execution_plan(
    selected_root: &Path,
    page_id: &str,
) -> Result<CalculationExecutionPlan, StorageError> {
    use crate::calculation::Calculation;
    let snapshot = open_page(selected_root, page_id)?;
    let reference = snapshot
        .page
        .calculation
        .as_ref()
        .ok_or_else(|| invalid_error("page is not computational"))?;
    let root = canonical_root(selected_root)?;
    let file = snapshot
        .calculations
        .iter()
        .find(|file| file.path == reference.source_path)
        .ok_or_else(|| invalid_error("calculation source is missing"))?;
    let document = Calculation::parse(&file.bytes)?;
    let mut parts = HashMap::new();
    for entry in &snapshot.manifest.pages {
        let page: Page = read_json(&root, &entry.path)?;
        if page.id != entry.id {
            return Err(invalid_error("manifest and page identities disagree"));
        }
        if let Some(part) = page.calculation.filter(|part| part.id == reference.id)
            && (part.source_path != reference.source_path
                || parts.insert(part.part_id, page.id).is_some())
        {
            return Err(invalid_error(
                "calculation parts must have one source and one page each",
            ));
        }
    }
    let steps = document
        .cells
        .iter()
        .filter(|cell| cell.cell_type == "code")
        .filter_map(|cell| {
            let part = cell.part_id()?;
            parts.get(part).map(|page| CalculationStep {
                page_id: page.clone(),
                cell_id: cell.id.clone(),
                part_id: part.to_owned(),
            })
        })
        .collect();
    Ok(CalculationExecutionPlan {
        revision: document.revision(),
        steps,
    })
}

fn load_page(
    root: &Path,
    manifest: NotebookManifest,
    page_id: &str,
) -> Result<NotebookSnapshot, StorageError> {
    let reference = page_reference(&manifest, page_id)?;
    let page: Page = read_json(root, &reference.path)?;
    let ink_layers = page
        .ink_layers
        .iter()
        .map(|reference| read_ink(root, &reference.path))
        .collect::<Result<Vec<_>, _>>()?;

    let block_paths = referenced_paths(&manifest, &page, SourceRole::Block);
    let asset_paths = referenced_paths(&manifest, &page, SourceRole::Asset);
    let blocks = read_stored_files(root, &block_paths, SourceRole::Block.max_bytes())?;
    let assets = read_stored_files(root, &asset_paths, MAX_IMAGE_BYTES)?;
    let calculations = read_stored_files(
        root,
        &referenced_paths(&manifest, &page, SourceRole::Calculation),
        SourceRole::Calculation.max_bytes(),
    )?;
    let snapshot = NotebookSnapshot {
        calculations,
        manifest,
        page,
        blocks,
        assets,
        ink_layers,
    };
    validate_snapshot(root, &snapshot)?;
    Ok(snapshot)
}

/// Where a new page lands in the manifest.
///
/// Not part of the on-disk format — it is an argument to the operation. Appending was never
/// enough: inserting before the current page is how a cover or a correction gets added, and
/// inserting after it is how you keep writing without walking back to the end.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum PagePosition {
    Before { page_id: String },
    After { page_id: String },
    Last,
}

/// A read-modify-write of the manifest that will not land on top of someone else's change.
///
/// Opening records the exact bytes the decision is being made from; committing re-reads them and
/// refuses if they moved. Every operation that edits the page list goes through this, so one
/// added later cannot quietly skip the check — which, with the sequence written out by hand at
/// four call sites, was only a matter of time.
struct ManifestGuard {
    root: PathBuf,
    bytes: Vec<u8>,
    manifest: NotebookManifest,
}

impl ManifestGuard {
    fn read(selected_root: &Path) -> Result<Self, StorageError> {
        let root = canonical_root(selected_root)?;
        recover_interrupted_transaction(&root)?;
        let bytes = read_limited(resolve_existing(&root, layout::MANIFEST)?, MAX_JSON_BYTES)?;
        let manifest: NotebookManifest = serde_json::from_slice(&bytes)?;
        validate_manifest(&manifest)?;
        Ok(Self {
            root,
            bytes,
            manifest,
        })
    }

    /// `modified_at` is checked here because every operation that reaches the manifest stamps it,
    /// and every one of them used to validate it separately.
    fn open(selected_root: &Path, modified_at: &str) -> Result<Self, StorageError> {
        if modified_at.is_empty() || modified_at.len() > 64 {
            return invalid("page timestamp must be present and bounded");
        }
        Self::read(selected_root)
    }

    /// As [`Self::open`], and additionally resolve every page file the manifest names. Operations
    /// that build on the existing pages need this; ones that only reorder references do not.
    fn open_resolved(selected_root: &Path, modified_at: &str) -> Result<Self, StorageError> {
        let guard = Self::open(selected_root, modified_at)?;
        validate_manifest_files(&guard.root, &guard.manifest)?;
        Ok(guard)
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn ensure_unchanged(&self, operation: &str) -> Result<(), StorageError> {
        let current = read_limited(
            resolve_existing(&self.root, layout::MANIFEST)?,
            MAX_JSON_BYTES,
        )?;
        if current != self.bytes {
            return Err(StorageError::ExternalChange(format!(
                "reopen the notebook before {operation}"
            )));
        }
        Ok(())
    }

    /// Persist the manifest alone, for operations that only move references around.
    fn commit(self, operation: &str) -> Result<NotebookManifest, StorageError> {
        self.ensure_unchanged(operation)?;
        write_json(&self.root, layout::MANIFEST, &self.manifest)?;
        Ok(self.manifest)
    }

    /// Persist a whole snapshot, for operations that also write new page content.
    fn commit_snapshot(
        self,
        snapshot: &NotebookSnapshot,
        operation: &str,
    ) -> Result<(), StorageError> {
        self.ensure_unchanged(operation)?;
        save_to_root(&self.root, snapshot)?;
        Ok(())
    }
}

pub fn create_page(
    selected_root: &Path,
    modified_at: &str,
    position: &PagePosition,
    background: Option<&PageBackground>,
    geometry: Option<&PageGeometry>,
) -> Result<NotebookSnapshot, StorageError> {
    create_page_inner(
        selected_root,
        modified_at,
        position,
        background,
        geometry,
        None,
    )
}

/// Publish a fresh page and its initial Page text in the same transaction.
pub fn create_page_with_text(
    selected_root: &Path,
    modified_at: &str,
    position: &PagePosition,
    background: Option<&PageBackground>,
    geometry: Option<&PageGeometry>,
    source: &str,
) -> Result<NotebookSnapshot, StorageError> {
    create_page_inner(
        selected_root,
        modified_at,
        position,
        background,
        geometry,
        Some(source),
    )
}

fn create_page_inner(
    selected_root: &Path,
    modified_at: &str,
    position: &PagePosition,
    background: Option<&PageBackground>,
    geometry: Option<&PageGeometry>,
    source: Option<&str>,
) -> Result<NotebookSnapshot, StorageError> {
    let mut guard = ManifestGuard::open_resolved(selected_root, modified_at)?;
    let root = guard.root().to_path_buf();
    // A page added to a version-1 notebook is a version-1 page. One notebook, one version.
    let schema_version = guard.manifest.schema_version;
    let manifest = &mut guard.manifest;

    let FreshPage {
        id,
        page_path,
        ink_id,
        ink_path,
    } = fresh_page_identifiers(&root, manifest, schema_version);

    let insert_at = match position {
        PagePosition::Last => manifest.pages.len(),
        PagePosition::Before { page_id } => neighbour_index(manifest, page_id)?,
        PagePosition::After { page_id } => neighbour_index(manifest, page_id)? + 1,
    };
    let geometry = geometry
        .cloned()
        .unwrap_or_else(|| manifest.default_page.geometry.clone());
    let background = background
        .cloned()
        .unwrap_or_else(|| manifest.default_page.background.clone());
    // The paper chosen for this page becomes the notebook's paper for new pages: the writer asked
    // for whatever they added last to be what the next add offers, starting from the paper the
    // notebook was created with. A PDF page is a document, not paper, so it never becomes it.
    if !matches!(background, PageBackground::Pdf { .. }) {
        manifest.default_page = PageDefaults {
            geometry: geometry.clone(),
            background: background.clone(),
        };
    }
    manifest.pages.insert(
        insert_at,
        PageReference {
            id: id.clone(),
            path: page_path,
            geometry: geometry.clone(),
        },
    );
    manifest.modified_at = modified_at.to_owned();
    let page = Page {
        calculation: None,
        schema_version,
        id: id.clone(),
        revision: 1,
        geometry,
        background,
        objects: Vec::new(),
        reading_order: Vec::new(),
        ink_layers: vec![InkLayerReference {
            id: ink_id.clone(),
            path: ink_path,
        }],
    };
    let mut snapshot = NotebookSnapshot {
        calculations: Vec::new(),
        manifest: guard.manifest.clone(),
        page,
        blocks: Vec::new(),
        assets: Vec::new(),
        ink_layers: vec![InkLayer {
            schema_version,
            id: ink_id,
            page_id: id,
            strokes: Vec::new(),
        }],
    };
    if let Some(source) = source {
        let id = format!("text-{}", snapshot.page.id);
        let path = (0..1000)
            .map(|n| format!("blocks/{id}-{n}.typ"))
            .find(|path| root.join(path).symlink_metadata().is_err())
            .ok_or_else(|| invalid_error("no unused Page text path"))?;
        snapshot.page.objects.push(crate::PageObject::PageTypst {
            fields: crate::ObjectFields {
                id: id.clone(),
                x: 0.,
                y: 0.,
                rotation: 0.,
                scale: 1.,
                z_index: 0,
                reading_order: 0,
                group_id: None,
                created_at: modified_at.into(),
                modified_at: modified_at.into(),
            },
            source_path: path.clone(),
        });
        snapshot.page.reading_order.push(id);
        snapshot.blocks.push(StoredFile {
            path,
            bytes: source.as_bytes().to_vec(),
        });
    }
    validate_snapshot(&root, &snapshot)?;
    guard.commit_snapshot(&snapshot, "adding a page")?;
    open_page(&root, &snapshot.page.id)
}

/// Create an independent calculation, or add an explicit part to an existing calculation.
pub fn create_calculation_page(
    selected_root: &Path,
    modified_at: &str,
    position: &PagePosition,
    title: &str,
    continue_page_id: Option<&str>,
) -> Result<NotebookSnapshot, StorageError> {
    create_calculation_page_inner(
        selected_root,
        modified_at,
        position,
        title,
        continue_page_id,
        None,
    )
}

pub fn import_calculation_page(
    selected_root: &Path,
    modified_at: &str,
    position: &PagePosition,
    title: &str,
    bytes: &[u8],
) -> Result<NotebookSnapshot, StorageError> {
    let document = super::calculation_import::prepare(bytes, title)?;
    create_calculation_page_inner(
        selected_root,
        modified_at,
        position,
        title,
        None,
        Some(document),
    )
}

fn create_calculation_page_inner(
    selected_root: &Path,
    modified_at: &str,
    position: &PagePosition,
    title: &str,
    continue_page_id: Option<&str>,
    imported: Option<crate::calculation::Calculation>,
) -> Result<NotebookSnapshot, StorageError> {
    use crate::calculation::{
        A4_WIDTH_PT, Calculation, CalculationPart, CalculationReference, Cell,
    };
    let mut guard = ManifestGuard::open_resolved(selected_root, modified_at)?;
    let root = guard.root().to_path_buf();
    let fresh = fresh_page_identifiers(&root, &guard.manifest, crate::CALCULATION_SCHEMA_VERSION);
    let part_id = format!("part-{}", fresh.id);
    let previous = continue_page_id
        .map(|page_id| open_page(&root, page_id))
        .transpose()?;
    let (source_path, document) = if let Some(previous) = &previous {
        let reference = previous
            .page
            .calculation
            .as_ref()
            .ok_or_else(|| invalid_error("continuation requires a computational page"))?;
        let file = previous
            .calculations
            .iter()
            .find(|file| file.path == reference.source_path)
            .ok_or_else(|| invalid_error("calculation source is missing"))?;
        let mut document = Calculation::parse(&file.bytes)?;
        let mut parts = document.parts()?;
        parts.push(CalculationPart {
            id: part_id.clone(),
            title: title.to_owned(),
        });
        document
            .goodtype_mut()
            .insert("parts".into(), serde_json::to_value(parts)?);
        document
            .cells
            .push(Cell::new(format!("cell-{part_id}"), &part_id, "code"));
        document.advance_revision()?;
        (reference.source_path.clone(), document)
    } else {
        let id = (0..1000)
            .map(|suffix| format!("calc-{}-{suffix}", fresh.id))
            .find(|id| !root.join(format!("calculations/{id}.ipynb")).exists())
            .ok_or_else(|| invalid_error("no unused calculation path is available"))?;
        let document = if let Some(mut imported) = imported {
            imported
                .goodtype_mut()
                .insert("id".into(), serde_json::json!(id));
            imported.goodtype_mut().insert(
                "parts".into(),
                serde_json::json!([{"id":part_id,"title":title}]),
            );
            for (index, cell) in imported.cells.iter_mut().enumerate() {
                cell.id = format!("cell-{}-{index}", fresh.id);
                cell.metadata["goodtype"]["partId"] = serde_json::json!(part_id);
            }
            imported
        } else {
            Calculation::new(&id, &part_id, title)
        };
        (format!("calculations/{id}.ipynb"), document)
    };
    let insert_at = match position {
        PagePosition::Last => guard.manifest.pages.len(),
        PagePosition::Before { page_id } => neighbour_index(&guard.manifest, page_id)?,
        PagePosition::After { page_id } => neighbour_index(&guard.manifest, page_id)? + 1,
    };
    let geometry = PageGeometry {
        width_pt: A4_WIDTH_PT,
        height_pt: 84.0,
    };
    guard.manifest.schema_version = crate::CALCULATION_SCHEMA_VERSION;
    guard.manifest.modified_at = modified_at.to_owned();
    guard.manifest.pages.insert(
        insert_at,
        PageReference {
            id: fresh.id.clone(),
            path: fresh.page_path.clone(),
            geometry: geometry.clone(),
        },
    );
    let snapshot = NotebookSnapshot {
        manifest: guard.manifest.clone(),
        page: Page {
            calculation: Some(CalculationReference {
                id: document.id().to_owned(),
                source_path: source_path.clone(),
                part_id,
            }),
            schema_version: crate::CALCULATION_SCHEMA_VERSION,
            id: fresh.id.clone(),
            revision: 1,
            geometry,
            background: PageBackground::Plain {
                color: "#ffffff".into(),
            },
            objects: Vec::new(),
            reading_order: Vec::new(),
            ink_layers: Vec::new(),
        },
        calculations: vec![StoredFile {
            path: source_path,
            bytes: document.bytes()?,
        }],
        blocks: Vec::new(),
        assets: Vec::new(),
        ink_layers: Vec::new(),
    };
    validate_snapshot(&root, &snapshot)?;
    guard.ensure_unchanged("adding a computational page")?;
    if let Some(previous) = previous {
        // The new view is unreferenced until the existing recovery transaction publishes the manifest.
        write_json(&root, &fresh.page_path, &snapshot.page)?;
        let mut candidate = previous.clone();
        candidate.manifest = snapshot.manifest.clone();
        candidate.calculations = snapshot.calculations.clone();
        candidate.page.revision = candidate
            .page
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid_error("page revision overflow"))?;
        super::write::save_transactional(&root, &previous, &candidate)?;
    } else {
        guard.commit_snapshot(&snapshot, "adding a computational page")?;
    }
    open_page(&root, &fresh.id)
}

/// Add source pages from one preserved PDF as consecutive notebook pages.
///
/// `pdf_pages` names the one-based PDF page each entry of `geometries` shows; without it the
/// geometries are the PDF's pages from the first on. Page and ink files land first and the manifest lands last. A failed import can therefore
/// leave recoverable orphan files, but it cannot make the notebook reference half an import.
pub fn import_pdf_pages(
    selected_root: &Path,
    modified_at: &str,
    position: &PagePosition,
    source_path: &str,
    geometries: &[PageGeometry],
    pdf_pages: Option<&[u32]>,
    replacing: Option<&str>,
) -> Result<NotebookSnapshot, StorageError> {
    if geometries.is_empty() || geometries.len() > 1_000 {
        return invalid("a PDF import must contain between 1 and 1000 pages");
    }
    if let Some(numbers) = pdf_pages {
        if numbers.len() != geometries.len() {
            return invalid("a PDF import must name one source page per imported page");
        }
        let mut seen = HashSet::with_capacity(numbers.len());
        if numbers
            .iter()
            .any(|&number| number == 0 || !seen.insert(number))
        {
            return invalid("PDF page numbers must start at 1 and must not repeat");
        }
    }

    let mut guard = ManifestGuard::open_resolved(selected_root, modified_at)?;
    let root = guard.root().to_path_buf();
    let schema_version = guard.manifest.schema_version;
    let insert_at = match position {
        PagePosition::Last => guard.manifest.pages.len(),
        PagePosition::Before { page_id } => neighbour_index(&guard.manifest, page_id)?,
        PagePosition::After { page_id } => neighbour_index(&guard.manifest, page_id)? + 1,
    };
    let mut pages = Vec::with_capacity(geometries.len());

    for (index, geometry) in geometries.iter().enumerate() {
        let FreshPage {
            id,
            page_path,
            ink_id,
            ink_path,
        } = fresh_page_identifiers(&root, &guard.manifest, schema_version);
        guard.manifest.pages.insert(
            insert_at + index,
            PageReference {
                id: id.clone(),
                path: page_path.clone(),
                geometry: geometry.clone(),
            },
        );
        let page = Page {
            calculation: None,
            schema_version,
            id: id.clone(),
            revision: 1,
            geometry: geometry.clone(),
            background: PageBackground::Pdf {
                source_path: source_path.to_owned(),
                page: match pdf_pages {
                    Some(numbers) => numbers[index],
                    None => u32::try_from(index + 1).expect("PDF import capped at 1000 pages"),
                },
            },
            objects: Vec::new(),
            reading_order: Vec::new(),
            ink_layers: vec![InkLayerReference {
                id: ink_id.clone(),
                path: ink_path.clone(),
            }],
        };
        pages.push((
            page,
            page_path,
            InkLayer {
                schema_version,
                id: ink_id,
                page_id: id,
                strokes: Vec::new(),
            },
            ink_path,
        ));
    }
    // A notebook started from a PDF swaps its blank starter page for the document in the same
    // manifest write, so it is one step to undo. As with deletion, only the reference goes.
    if let Some(page_id) = replacing {
        let index = guard
            .manifest
            .pages
            .iter()
            .position(|page| page.id == page_id)
            .ok_or_else(|| invalid_error("the page to replace is not in this notebook"))?;
        guard.manifest.pages.remove(index);
    }
    guard.manifest.modified_at = modified_at.to_owned();

    for (page, _, ink, _) in &pages {
        validate_snapshot(
            &root,
            &NotebookSnapshot {
                calculations: Vec::new(),
                manifest: guard.manifest.clone(),
                page: page.clone(),
                blocks: Vec::new(),
                assets: Vec::new(),
                ink_layers: vec![ink.clone()],
            },
        )?;
    }
    guard.ensure_unchanged("importing PDF pages")?;
    for (page, page_path, ink, ink_path) in &pages {
        write_ink(&root, ink_path, ink)?;
        write_json(&root, page_path, page)?;
    }
    write_json(&root, layout::MANIFEST, &guard.manifest)?;

    open_page(&root, &pages[0].0.id)
}

/// Allocate a fresh identity for everything a duplicated page owns.
///
/// Assets and the shared style are deliberately absent from the source map: anything left
/// unmapped passes through [`IdRemap::source`] unchanged, which is exactly what "original
/// assets stay shared" means. Stroke IDs are keyed by their old ID rather than by position,
/// which the uniqueness rule in `validate_ink` already guarantees.
fn duplication_remap(source: &NotebookSnapshot, new_id: &str, modified_at: &str) -> IdRemap {
    let mut remap = IdRemap::new(modified_at);
    for (index, object) in source.page.objects.iter().enumerate() {
        remap.map_object(&object.fields().id, layout::object_id(new_id, index + 1));
    }
    for (index, reference) in source.page.ink_layers.iter().enumerate() {
        remap.map_ink_layer(
            &reference.id,
            layout::ink_layer_id(new_id, index + 1),
            &reference.path,
            layout::ink_layer_path_for_schema(new_id, index + 1, source.page.schema_version),
        );
    }
    let mut blocks = 0usize;
    for object in &source.page.objects {
        if let Some(file) = object.source()
            && file.role == SourceRole::Block
            && !remap.has_source(file.path)
        {
            blocks += 1;
            remap.map_source(file.path, layout::block_path(new_id, blocks));
        }
    }
    let mut strokes = 0usize;
    for layer in &source.ink_layers {
        for stroke in &layer.strokes {
            strokes += 1;
            remap.map_stroke(&stroke.id, layout::stroke_id(new_id, strokes));
        }
    }
    remap
}

/// Position of the page a new one is being placed next to. A stale id means the caller's view of
/// the notebook no longer matches the file, which is a refusal rather than a silent append.
fn neighbour_index(manifest: &NotebookManifest, page_id: &str) -> Result<usize, StorageError> {
    manifest
        .pages
        .iter()
        .position(|page| page.id == page_id)
        .ok_or_else(|| invalid_error("cannot place a page next to one this notebook does not have"))
}

/// A page identity nothing in the notebook is using yet.
struct FreshPage {
    id: String,
    page_path: String,
    ink_id: String,
    ink_path: String,
}

fn fresh_page_identifiers(
    root: &Path,
    manifest: &NotebookManifest,
    schema_version: u32,
) -> FreshPage {
    let mut number = manifest.pages.len() + 1;
    loop {
        let id = layout::page_id(number);
        let fresh = FreshPage {
            page_path: layout::page_path(&id),
            ink_id: layout::ink_layer_id(&id, 1),
            ink_path: layout::ink_layer_path_for_schema(&id, 1, schema_version),
            id,
        };
        if !manifest
            .pages
            .iter()
            .any(|page| page.id == fresh.id || page.path == fresh.page_path)
            && !root.join(&fresh.page_path).exists()
            && !root.join(&fresh.ink_path).exists()
        {
            return fresh;
        }
        number += 1;
    }
}

/// Duplicate one page with fresh page, object, group, ink-layer, stroke, and block IDs.
/// Original assets stay shared by reference; the shared style stays shared. The new page is
/// inserted directly after its source in the manifest.
pub fn duplicate_page(
    selected_root: &Path,
    page_id: &str,
    modified_at: &str,
) -> Result<NotebookSnapshot, StorageError> {
    let mut guard = ManifestGuard::open(selected_root, modified_at)?;
    let root = guard.root().to_path_buf();
    let source = open_page(&root, page_id)?;
    let manifest = &mut guard.manifest;

    let fresh = fresh_page_identifiers(&root, manifest, source.page.schema_version);
    let (new_id, new_page_path) = (fresh.id, fresh.page_path);

    let remap = duplication_remap(&source, &new_id, modified_at);

    let ink_layers = source
        .ink_layers
        .iter()
        .map(|layer| {
            let strokes = layer
                .strokes
                .iter()
                .map(|stroke| {
                    Ok(crate::Stroke {
                        id: remap.stroke(&stroke.id)?,
                        group_id: remap.optional_object(stroke.group_id.as_deref())?,
                        ..stroke.clone()
                    })
                })
                .collect::<Result<Vec<_>, &'static str>>()?;
            Ok(InkLayer {
                schema_version: layer.schema_version,
                id: remap.ink_layer(&layer.id)?,
                page_id: new_id.clone(),
                strokes,
            })
        })
        .collect::<Result<Vec<_>, &'static str>>()
        .map_err(invalid_error)?;

    // A copied part is independent. Preserve its saved results with explicit provenance.
    let (calculation, calculations) = if let Some(reference) = &source.page.calculation {
        use crate::calculation::{Calculation, CalculationReference};
        let original = Calculation::parse(&source.calculations[0].bytes)?;
        let id = format!("calc-{new_id}");
        let part = format!("part-{new_id}");
        let path = format!("calculations/{id}.ipynb");
        if root.join(&path).exists() {
            return Err(invalid_error("copied calculation path already exists"));
        }
        let title = original.metadata["goodtype"]["title"]
            .as_str()
            .unwrap_or("");
        let mut copy = Calculation::new(&id, &part, title);
        copy.cells = original
            .cells
            .into_iter()
            .filter(|cell| cell.part_id() == Some(&reference.part_id))
            .collect();
        for (index, cell) in copy.cells.iter_mut().enumerate() {
            cell.id = format!("cell-{new_id}-{index}");
            cell.metadata["goodtype"]["partId"] = serde_json::json!(part);
            cell.metadata["goodtype"]["copiedFrom"] =
                serde_json::json!({"calculationId":reference.id,"partId":reference.part_id});
        }
        (
            Some(CalculationReference {
                id,
                part_id: part,
                source_path: path.clone(),
            }),
            vec![StoredFile {
                path,
                bytes: copy.bytes()?,
            }],
        )
    } else {
        (None, Vec::new())
    };
    let page = Page {
        calculation,
        schema_version: source.page.schema_version,
        id: new_id.clone(),
        revision: 1,
        geometry: source.page.geometry.clone(),
        background: source.page.background.clone(),
        objects: source
            .page
            .objects
            .iter()
            .map(|object| object.remapped(&remap))
            .collect::<Result<Vec<_>, _>>()
            .map_err(invalid_error)?,
        reading_order: source
            .page
            .reading_order
            .iter()
            .map(|id| remap.object(id))
            .collect::<Result<Vec<_>, _>>()
            .map_err(invalid_error)?,
        ink_layers: source
            .page
            .ink_layers
            .iter()
            .map(|reference| {
                Ok(InkLayerReference {
                    id: remap.ink_layer(&reference.id)?,
                    path: remap.ink_layer_path(&reference.path)?,
                })
            })
            .collect::<Result<Vec<_>, &'static str>>()
            .map_err(invalid_error)?,
    };

    let source_index = manifest
        .pages
        .iter()
        .position(|reference| reference.id == page_id)
        .ok_or_else(|| invalid_error("page is not referenced by the notebook manifest"))?;
    manifest.pages.insert(
        source_index + 1,
        PageReference {
            id: new_id.clone(),
            path: new_page_path,
            geometry: source.page.geometry.clone(),
        },
    );
    manifest.modified_at = modified_at.to_owned();

    let blocks = source
        .blocks
        .iter()
        .map(|file| StoredFile {
            path: remap.source(&file.path),
            bytes: file.bytes.clone(),
        })
        .collect();

    let snapshot = NotebookSnapshot {
        calculations,
        manifest: guard.manifest.clone(),
        page,
        blocks,
        assets: Vec::new(),
        ink_layers,
    };
    validate_snapshot(&root, &snapshot)?;
    guard.commit_snapshot(&snapshot, "duplicating a page")?;
    open_page(&root, &new_id)
}

/// Remove one page's manifest reference transactionally. Canonical page, ink, block, and asset
/// files are retained for recovery; only the reference disappears. Returns the bundle of the
/// nearest remaining page.
pub fn delete_page(
    selected_root: &Path,
    page_id: &str,
    modified_at: &str,
) -> Result<NotebookSnapshot, StorageError> {
    let mut guard = ManifestGuard::open(selected_root, modified_at)?;
    let root = guard.root().to_path_buf();
    let manifest = &mut guard.manifest;
    if manifest.pages.len() < 2 {
        return invalid("the final remaining page cannot be deleted");
    }
    let index = manifest
        .pages
        .iter()
        .position(|reference| reference.id == page_id)
        .ok_or_else(|| invalid_error("page is not referenced by the notebook manifest"))?;
    manifest.pages.remove(index);
    manifest.modified_at = modified_at.to_owned();

    let manifest = guard.commit("deleting a page")?;
    let neighbor = manifest.pages[index.min(manifest.pages.len() - 1)]
        .id
        .clone();
    open_page(&root, &neighbor)
}

/// Remove several page references in one manifest write, so they are one step to undo. As with
/// [`delete_page`], only the references go; the page and ink files stay on disk.
///
/// The returned snapshot opens `active_page_id` when it survives, and otherwise the page that now
/// stands where the first removed page was.
pub fn delete_pages(
    selected_root: &Path,
    page_ids: &[String],
    modified_at: &str,
    active_page_id: &str,
) -> Result<NotebookSnapshot, StorageError> {
    if page_ids.is_empty() {
        return invalid("a page deletion must name at least one page");
    }
    let mut guard = ManifestGuard::open(selected_root, modified_at)?;
    let root = guard.root().to_path_buf();
    let manifest = &mut guard.manifest;
    let doomed: HashSet<&str> = page_ids.iter().map(String::as_str).collect();
    if doomed
        .iter()
        .any(|id| !manifest.pages.iter().any(|page| page.id == *id))
    {
        return invalid("page is not referenced by the notebook manifest");
    }
    if doomed.len() >= manifest.pages.len() {
        return invalid("the final remaining page cannot be deleted");
    }
    let first = manifest
        .pages
        .iter()
        .position(|page| doomed.contains(page.id.as_str()))
        .expect("validated page ids");
    manifest
        .pages
        .retain(|page| !doomed.contains(page.id.as_str()));
    manifest.modified_at = modified_at.to_owned();

    let manifest = guard.commit("deleting pages")?;
    let open = if manifest.pages.iter().any(|page| page.id == active_page_id) {
        active_page_id.to_owned()
    } else {
        manifest.pages[first.min(manifest.pages.len() - 1)]
            .id
            .clone()
    };
    open_page(&root, &open)
}

/// Reorder the manifest page list. `ordered_ids` must be a permutation of the current page IDs;
/// page files, IDs, and paths are untouched.
pub fn reorder_pages(
    selected_root: &Path,
    ordered_ids: &[String],
    modified_at: &str,
    active_page_id: &str,
) -> Result<NotebookSnapshot, StorageError> {
    let mut guard = ManifestGuard::open(selected_root, modified_at)?;
    let root = guard.root().to_path_buf();
    let manifest = &mut guard.manifest;

    let current: HashSet<&str> = manifest.pages.iter().map(|page| page.id.as_str()).collect();
    let requested: HashSet<&str> = ordered_ids.iter().map(String::as_str).collect();
    if ordered_ids.len() != manifest.pages.len() || current != requested {
        return invalid("reorder must list every current page exactly once");
    }

    let mut by_id: HashMap<&str, PageReference> = manifest
        .pages
        .iter()
        .map(|page| (page.id.as_str(), page.clone()))
        .collect();
    manifest.pages = ordered_ids
        .iter()
        .map(|id| by_id.remove(id.as_str()).expect("validated permutation"))
        .collect();
    manifest.modified_at = modified_at.to_owned();

    guard.commit("reordering pages")?;
    open_page(&root, active_page_id)
}

#[derive(Clone, Debug)]
struct StructureSnapshot {
    manifest: NotebookManifest,
    active_page_id: String,
}

/// Session-only history for changes to the manifest page list.
///
/// Page files are deliberately absent: create and duplicate write fresh immutable identities,
/// while delete only removes a manifest reference. Restoring the manifest is therefore enough
/// to undo every page-list operation without copying page payloads into memory.
#[derive(Debug, Default)]
pub struct NotebookStructureHistory {
    undo: Vec<StructureSnapshot>,
    redo: Vec<StructureSnapshot>,
    current: Option<StructureSnapshot>,
    current_manifest_bytes: Option<Vec<u8>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureHistoryResult {
    pub snapshot: NotebookSnapshot,
    pub can_undo: bool,
    pub can_redo: bool,
}

/// Observe the page list before a structure operation. An outside manifest change invalidates
/// the session history rather than becoming something undo may overwrite.
pub fn observe_structure(
    selected_root: &Path,
    active_page_id: &str,
    history: &mut NotebookStructureHistory,
) -> Result<(), StorageError> {
    let guard = ManifestGuard::read(selected_root)?;
    page_reference(&guard.manifest, active_page_id)?;
    let fingerprint = manifest_fingerprint_bytes(&guard.manifest)?;
    if history
        .current_manifest_bytes
        .as_ref()
        .is_some_and(|bytes| bytes != &fingerprint)
    {
        history.undo.clear();
        history.redo.clear();
    }
    history.current = Some(StructureSnapshot {
        manifest: guard.manifest,
        active_page_id: active_page_id.to_owned(),
    });
    history.current_manifest_bytes = Some(fingerprint);
    Ok(())
}

/// Record the manifest produced by a successful page-list operation.
pub fn advance_structure(
    selected_root: &Path,
    snapshot: &NotebookSnapshot,
    history: &mut NotebookStructureHistory,
) -> Result<StructureHistoryResult, StorageError> {
    let guard = ManifestGuard::read(selected_root)?;
    let fingerprint = manifest_fingerprint_bytes(&guard.manifest)?;
    let previous = history
        .current
        .take()
        .ok_or_else(|| invalid_error("page structure must be observed before changing it"))?;
    push_structure(&mut history.undo, previous);
    history.redo.clear();
    history.current = Some(StructureSnapshot {
        manifest: guard.manifest,
        active_page_id: snapshot.page.id.clone(),
    });
    history.current_manifest_bytes = Some(fingerprint);
    Ok(structure_result(snapshot.clone(), history))
}

pub fn undo_structure(
    selected_root: &Path,
    modified_at: &str,
    history: &mut NotebookStructureHistory,
) -> Result<StructureHistoryResult, StorageError> {
    restore_structure(selected_root, modified_at, history, true)
}

pub fn redo_structure(
    selected_root: &Path,
    modified_at: &str,
    history: &mut NotebookStructureHistory,
) -> Result<StructureHistoryResult, StorageError> {
    restore_structure(selected_root, modified_at, history, false)
}

fn restore_structure(
    selected_root: &Path,
    modified_at: &str,
    history: &mut NotebookStructureHistory,
    undo: bool,
) -> Result<StructureHistoryResult, StorageError> {
    let mut guard = ManifestGuard::open(selected_root, modified_at)?;
    if history.current_manifest_bytes.as_ref()
        != Some(&manifest_fingerprint_bytes(&guard.manifest)?)
    {
        history.undo.clear();
        history.redo.clear();
        return Err(StorageError::ExternalChange(
            "reopen the notebook before changing page history".into(),
        ));
    }
    let source = if undo { &history.undo } else { &history.redo };
    let target = source
        .last()
        .ok_or_else(|| invalid_error("nothing to undo or redo in page structure"))?;
    let current = history
        .current
        .as_ref()
        .cloned()
        .ok_or_else(|| invalid_error("page structure must be observed before undo or redo"))?;
    // Restore only page references; newer schema and other notebook settings stay current.
    guard.manifest.pages = target.manifest.pages.clone();
    guard.manifest.modified_at = modified_at.to_owned();
    validate_manifest_files(guard.root(), &guard.manifest)?;
    let root = guard.root().to_path_buf();
    let snapshot = load_page(&root, guard.manifest.clone(), &target.active_page_id)?;
    let fingerprint = manifest_fingerprint_bytes(&guard.manifest)?;
    let active_page_id = target.active_page_id.clone();
    guard.commit(if undo {
        "undoing a page-list change"
    } else {
        "redoing a page-list change"
    })?;
    if undo {
        history.undo.pop();
        push_structure(&mut history.redo, current);
    } else {
        history.redo.pop();
        push_structure(&mut history.undo, current);
    }
    history.current = Some(StructureSnapshot {
        manifest: snapshot.manifest.clone(),
        active_page_id,
    });
    history.current_manifest_bytes = Some(fingerprint);
    Ok(structure_result(snapshot, history))
}

fn push_structure(stack: &mut Vec<StructureSnapshot>, snapshot: StructureSnapshot) {
    if stack.len() == HISTORY_LIMIT {
        stack.remove(0);
    }
    stack.push(snapshot);
}

fn structure_result(
    snapshot: NotebookSnapshot,
    history: &NotebookStructureHistory,
) -> StructureHistoryResult {
    StructureHistoryResult {
        snapshot,
        can_undo: !history.undo.is_empty(),
        can_redo: !history.redo.is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        BINARY_INK_SCHEMA_VERSION as SCHEMA_VERSION, MIN_SUPPORTED_SCHEMA_VERSION, PageBackground,
        PageGeometry, PageObject, ShapeGeometry, ShapePoint, ShapeStyle, StrokePattern,
        storage::{fixtures::*, *},
    };
    use std::path::Path;

    #[test]
    fn blank_pages_and_duplicates_preserve_the_shared_style() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("notebook");
        let mut initial = snapshot();
        initial.manifest.shared_style_path = Some("style.typ".into());
        let style = b"#set text(size: 12pt)\n";
        initial.blocks.push(StoredFile {
            path: "style.typ".into(),
            bytes: style.to_vec(),
        });
        create_notebook(&root, &initial).unwrap();
        let added = create_page(
            &root,
            "2026-10-12T10:00:00Z",
            &PagePosition::Last,
            None,
            None,
        )
        .unwrap();
        assert_eq!(added.manifest.pages.len(), 2);
        assert!(
            added
                .blocks
                .iter()
                .any(|file| file.path == "style.typ" && file.bytes == style)
        );
        duplicate_page(&root, &added.page.id, "2026-10-12T10:01:00Z").unwrap();
        let mut history = NotebookHistory::default();
        let mut candidate = observe_page(&root, &added.page.id, &mut history).unwrap();
        candidate.blocks.clear();
        let saved = commit_notebook(&root, &mut history, candidate).unwrap();
        assert_eq!(std::fs::read(root.join("style.typ")).unwrap(), style);
        assert_eq!(saved.snapshot.page.revision, 2);
        assert!(
            saved
                .snapshot
                .blocks
                .iter()
                .any(|file| file.path == "style.typ")
        );
        observe_page(&root, &added.page.id, &mut history).unwrap();
    }

    #[test]
    fn structure_history_survives_page_saves_and_does_not_downgrade_schema() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("notebook");
        create_notebook(&root, &legacy_snapshot()).unwrap();
        let mut structure = NotebookStructureHistory::default();
        observe_structure(&root, "page-001", &mut structure).unwrap();
        let added = create_page(
            &root,
            "2026-10-12T10:00:00Z",
            &PagePosition::Last,
            None,
            None,
        )
        .unwrap();
        advance_structure(&root, &added, &mut structure).unwrap();
        let mut history = NotebookHistory::default();
        let mut page = observe_page(&root, "page-001", &mut history).unwrap();
        page.manifest.modified_at = "2026-10-12T10:01:00Z".into();
        commit_notebook(&root, &mut history, page).unwrap();
        let undone = undo_structure(&root, "2026-10-12T10:02:00Z", &mut structure).unwrap();
        assert_eq!(undone.snapshot.manifest.pages.len(), 1);
        assert_eq!(undone.snapshot.manifest.schema_version, SCHEMA_VERSION);
        assert!(undone.snapshot.page.ink_layers[0].path.ends_with(".ink"));
        let redone = redo_structure(&root, "2026-10-12T10:03:00Z", &mut structure).unwrap();
        assert_eq!(redone.snapshot.manifest.pages.len(), 2);
        assert_eq!(redone.snapshot.manifest.schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn the_paper_last_added_becomes_the_paper_for_new_pages() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("notebook");
        create_notebook(&root, &snapshot()).unwrap();
        let dotted = PageBackground::Plain {
            color: "#f4efe3".into(),
        };
        let a5 = PageGeometry {
            width_pt: 419.528,
            height_pt: 595.276,
        };
        let added = create_page(
            &root,
            "2026-10-14T10:00:00Z",
            &PagePosition::Last,
            Some(&dotted),
            Some(&a5),
        )
        .unwrap();
        assert_eq!(added.manifest.default_page.background, dotted);
        assert_eq!(added.manifest.default_page.geometry.width_pt, a5.width_pt);

        // A page added without a choice comes out as that paper.
        let next = create_page(
            &root,
            "2026-10-14T10:01:00Z",
            &PagePosition::Last,
            None,
            None,
        )
        .unwrap();
        assert_eq!(next.page.background, dotted);
        assert_eq!(next.page.geometry.height_pt, a5.height_pt);
    }

    #[test]
    fn failed_structure_undo_can_be_retried_after_repair() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("notebook");
        create_notebook(&root, &snapshot()).unwrap();
        let added = create_page(
            &root,
            "2026-10-12T10:00:00Z",
            &PagePosition::Last,
            None,
            None,
        )
        .unwrap();
        let mut structure = NotebookStructureHistory::default();
        observe_structure(&root, &added.page.id, &mut structure).unwrap();
        let deleted = delete_page(&root, &added.page.id, "2026-10-12T10:01:00Z").unwrap();
        advance_structure(&root, &deleted, &mut structure).unwrap();
        let path = root.join(crate::layout::page_path(&added.page.id));
        let original = std::fs::read(&path).unwrap();
        let manifest = std::fs::read(root.join(crate::layout::MANIFEST)).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(undo_structure(&root, "2026-10-12T10:02:00Z", &mut structure).is_err());
        assert_eq!(
            std::fs::read(root.join(crate::layout::MANIFEST)).unwrap(),
            manifest
        );
        std::fs::write(path, original).unwrap();
        let restored = undo_structure(&root, "2026-10-12T10:02:00Z", &mut structure).unwrap();
        assert_eq!(restored.snapshot.page.id, added.page.id);
        assert_eq!(restored.snapshot.manifest.pages.len(), 2);
    }

    #[test]
    fn structure_undo_still_refuses_external_manifest_content_changes() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("notebook");
        create_notebook(&root, &snapshot()).unwrap();
        let mut structure = NotebookStructureHistory::default();
        observe_structure(&root, "page-001", &mut structure).unwrap();
        let added = create_page(
            &root,
            "2026-10-12T10:00:00Z",
            &PagePosition::Last,
            None,
            None,
        )
        .unwrap();
        advance_structure(&root, &added, &mut structure).unwrap();
        let path = root.join(crate::layout::MANIFEST);
        let mut manifest = added.manifest;
        manifest.title = "External title".into();
        let bytes = serde_json::to_vec_pretty(&manifest).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        assert!(undo_structure(&root, "2026-10-12T10:01:00Z", &mut structure).is_err());
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn creates_saves_and_reopens_one_page() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();

        let mut reopened = open_notebook(&notebook_root).unwrap();
        assert_eq!(reopened.page.revision, 1);
        assert_eq!(reopened.ink_layers[0].strokes.len(), 1);
        reopened.page.revision = 2;
        if let PageObject::Image { fields, .. } = &mut reopened.page.objects[1] {
            fields.x = 144.0;
            fields.scale = 1.5;
        }
        save_notebook(&notebook_root, &reopened).unwrap();

        let saved = open_notebook(&notebook_root).unwrap();
        assert_eq!(saved.page.revision, 2);
        let PageObject::Image { fields, .. } = &saved.page.objects[1] else {
            panic!("image object missing");
        };
        assert_eq!((fields.x, fields.scale), (144.0, 1.5));
    }

    #[test]
    fn creates_and_commits_pages_independently() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();

        let second = create_page(
            &notebook_root,
            "2026-07-23T18:00:00Z",
            &PagePosition::Last,
            None,
            None,
        )
        .unwrap();
        assert_eq!(second.manifest.pages.len(), 2);
        assert_eq!(second.page.id, "page-002");
        assert!(second.page.objects.is_empty());

        let mut history = NotebookHistory::default();
        let mut changed = observe_page(&notebook_root, "page-002", &mut history).unwrap();
        let mut stroke = snapshot().ink_layers[0].strokes[0].clone();
        stroke.id = "page-002-stroke-001".into();
        changed.ink_layers[0].strokes.push(stroke);
        commit_notebook(&notebook_root, &mut history, changed).unwrap();

        let first = open_page(&notebook_root, "page-001").unwrap();
        let second = open_page(&notebook_root, "page-002").unwrap();
        assert_eq!(first.page.revision, 1);
        assert_eq!(first.ink_layers[0].strokes.len(), 1);
        assert_eq!(second.page.revision, 2);
        assert_eq!(second.ink_layers[0].strokes.len(), 1);
        assert_eq!(
            second
                .manifest
                .pages
                .iter()
                .map(|page| page.id.as_str())
                .collect::<Vec<_>>(),
            ["page-001", "page-002"]
        );
    }

    #[test]
    fn multipage_fixture_opens_every_page_without_loading_all() {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/notebooks/multipage");

        let first = open_notebook(&fixture).unwrap();
        // Opening does not upgrade: a version-1 notebook is still version 1 in hand.
        assert_eq!(first.manifest.schema_version, MIN_SUPPORTED_SCHEMA_VERSION);
        assert_eq!(first.page.schema_version, MIN_SUPPORTED_SCHEMA_VERSION);
        assert_eq!(
            first.ink_layers[0].schema_version,
            MIN_SUPPORTED_SCHEMA_VERSION
        );
        assert_eq!(first.manifest.pages.len(), 3);
        assert_eq!(first.page.id, "page-001");
        assert_eq!(first.ink_layers[0].strokes.len(), 1);

        let second = open_page(&fixture, "page-002").unwrap();
        assert!(matches!(second.page.objects[0], PageObject::Image { .. }));
        assert_eq!(second.ink_layers[0].strokes[0].id, "page-002-stroke-0001");

        let third = open_page(&fixture, "page-003").unwrap();
        assert!(third.ink_layers[0].strokes.is_empty());

        // Search reaches every page's Typst source in manifest order.
        let hits = search_notebook(&fixture, "momentum").unwrap();
        assert_eq!(
            hits.iter().map(|hit| hit.page_number).collect::<Vec<_>>(),
            [1, 3]
        );
    }

    #[test]
    fn opens_version_two_shape_fixture() {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/notebooks/shape-v2");
        let snapshot = open_notebook(&fixture).unwrap();
        assert_eq!(
            snapshot.manifest.schema_version,
            crate::SHAPE_SCHEMA_VERSION
        );
        assert_eq!(snapshot.page.objects.len(), 4);
        assert!(
            snapshot
                .page
                .objects
                .iter()
                .all(|object| matches!(object, PageObject::Shape { .. }))
        );
    }

    /// Opening leaves a legacy bundle alone. Its first confirmed write migrates only that page;
    /// an untouched sibling stays readable at version 1 until its own confirmed write.
    #[test]
    fn a_legacy_notebook_migrates_one_confirmed_page_at_a_time() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("notebook");
        let mut snapshot = snapshot();
        snapshot.manifest.schema_version = MIN_SUPPORTED_SCHEMA_VERSION;
        snapshot.page.schema_version = MIN_SUPPORTED_SCHEMA_VERSION;
        for layer in &mut snapshot.ink_layers {
            layer.schema_version = MIN_SUPPORTED_SCHEMA_VERSION;
        }
        snapshot.page.ink_layers[0].path = crate::layout::legacy_ink_layer_path("page-001", 1);
        create_notebook(&root, &snapshot).unwrap();
        create_page(
            &root,
            "2026-09-03T08:59:00Z",
            &PagePosition::Last,
            None,
            None,
        )
        .unwrap();

        let mut history = NotebookHistory::default();
        let mut edited = observe_notebook(&root, &mut history).unwrap();
        edited.page.background = PageBackground::Plain {
            color: "#fffdf7".into(),
        };
        let saved = commit_notebook(&root, &mut history, edited)
            .unwrap()
            .snapshot;
        assert_eq!(saved.manifest.schema_version, SCHEMA_VERSION);
        assert_eq!(saved.page.schema_version, SCHEMA_VERSION);
        assert_eq!(
            open_notebook(&root).unwrap().manifest.schema_version,
            SCHEMA_VERSION
        );

        let mut with_shape = observe_notebook(&root, &mut history).unwrap();
        let reading_order = u32::try_from(with_shape.page.reading_order.len()).unwrap();
        with_shape.page.objects.push(PageObject::Shape {
            fields: fields("shape-1", reading_order),
            geometry: ShapeGeometry::Line {
                start: ShapePoint { x: 0.0, y: 0.0 },
                end: ShapePoint { x: 60.0, y: 20.0 },
            },
            style: ShapeStyle {
                stroke_color: "#16212b".into(),
                stroke_width_pt: 1.6,
                fill_color: None,
                opacity: 1.0,
                pattern: StrokePattern::Solid,
                pattern_spacing: 1.0,
                pattern_offset_pt: 0.0,
            },
        });
        with_shape.page.reading_order.push("shape-1".into());
        let upgraded = commit_notebook(&root, &mut history, with_shape)
            .unwrap()
            .snapshot;
        assert_eq!(upgraded.manifest.schema_version, SCHEMA_VERSION);
        assert_eq!(upgraded.page.schema_version, SCHEMA_VERSION);
        assert_eq!(upgraded.ink_layers[0].schema_version, SCHEMA_VERSION);

        let reopened = open_notebook(&root).unwrap();
        assert_eq!(reopened.manifest.schema_version, SCHEMA_VERSION);

        // The untouched sibling remains a valid version-1 bundle below the version-3 manifest,
        // then upgrades normally on its next confirmed write.
        let sibling = open_page(&root, "page-002").unwrap();
        assert_eq!(sibling.page.schema_version, MIN_SUPPORTED_SCHEMA_VERSION);
        assert_eq!(
            sibling.ink_layers[0].schema_version,
            MIN_SUPPORTED_SCHEMA_VERSION
        );
        let mut sibling_history = NotebookHistory::default();
        let sibling = observe_page(&root, "page-002", &mut sibling_history).unwrap();
        let sibling = commit_notebook(&root, &mut sibling_history, sibling)
            .unwrap()
            .snapshot;
        assert_eq!(sibling.page.schema_version, SCHEMA_VERSION);
        assert_eq!(sibling.ink_layers[0].schema_version, SCHEMA_VERSION);
    }

    /// A page added to a version-1 notebook starts at the manifest's current version.
    #[test]
    fn a_page_added_to_a_version_one_notebook_stays_version_one() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("notebook");
        let mut snapshot = snapshot();
        snapshot.manifest.schema_version = MIN_SUPPORTED_SCHEMA_VERSION;
        snapshot.page.schema_version = MIN_SUPPORTED_SCHEMA_VERSION;
        for layer in &mut snapshot.ink_layers {
            layer.schema_version = MIN_SUPPORTED_SCHEMA_VERSION;
        }
        snapshot.page.ink_layers[0].path = crate::layout::legacy_ink_layer_path("page-001", 1);
        create_notebook(&root, &snapshot).unwrap();

        let added = create_page(
            &root,
            "2026-09-03T09:00:00Z",
            &PagePosition::Last,
            None,
            None,
        )
        .unwrap();
        assert_eq!(added.manifest.schema_version, MIN_SUPPORTED_SCHEMA_VERSION);
        assert_eq!(added.page.schema_version, MIN_SUPPORTED_SCHEMA_VERSION);
        assert_eq!(
            added.ink_layers[0].schema_version,
            MIN_SUPPORTED_SCHEMA_VERSION
        );
    }

    #[test]
    fn opens_edges_of_a_hundred_page_notebook() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();
        for minute in 1..100 {
            create_page(
                &notebook_root,
                &format!("2026-07-29T10:{:02}:00Z", minute % 60),
                &PagePosition::Last,
                None,
                None,
            )
            .unwrap();
        }

        let opened = open_notebook(&notebook_root).unwrap();
        assert_eq!(opened.manifest.pages.len(), 100);
        assert_eq!(
            open_page(&notebook_root, "page-050").unwrap().page.id,
            "page-050"
        );
        assert_eq!(
            open_page(&notebook_root, "page-100").unwrap().page.id,
            "page-100"
        );
    }

    #[test]
    fn duplicates_a_page_with_fresh_ids_and_shared_originals() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        let mut original_snapshot = snapshot();
        let mut page_fields = fields("page-text", 2);
        page_fields.x = 0.0;
        page_fields.y = 0.0;
        original_snapshot.page.objects.push(PageObject::PageTypst {
            fields: page_fields,
            source_path: "blocks/page.typ".into(),
        });
        original_snapshot
            .page
            .reading_order
            .push("page-text".into());
        original_snapshot.blocks.push(StoredFile {
            path: "blocks/page.typ".into(),
            bytes: b"Full page prose".to_vec(),
        });
        create_notebook(&notebook_root, &original_snapshot).unwrap();

        let copy = duplicate_page(&notebook_root, "page-001", "2026-07-23T19:00:00Z").unwrap();
        assert_eq!(copy.page.id, "page-002");
        assert_eq!(copy.page.revision, 1);
        assert_eq!(copy.page.objects.len(), 3);
        assert_eq!(copy.ink_layers[0].strokes.len(), 1);
        // Fresh identity everywhere the page owns one.
        assert!(
            copy.page
                .objects
                .iter()
                .all(|object| { object.fields().id.starts_with("page-002-obj-") })
        );
        assert!(copy.ink_layers[0].id.starts_with("page-002-ink-"));
        assert!(
            copy.ink_layers[0].strokes[0]
                .id
                .starts_with("page-002-stroke-")
        );
        let PageObject::Typst { source_path, .. } = &copy.page.objects[0] else {
            panic!("typst object missing");
        };
        assert!(source_path.starts_with("blocks/page-002-block-"));
        // Shared originals stay shared.
        let PageObject::Image { source_path, .. } = &copy.page.objects[1] else {
            panic!("image object missing");
        };
        assert_eq!(source_path, "assets/diagram.png");
        let PageObject::PageTypst { source_path, .. } = &copy.page.objects[2] else {
            panic!("page Typst object missing");
        };
        assert!(source_path.starts_with("blocks/page-002-block-"));
        assert_eq!(
            search_notebook(&notebook_root, "Full page").unwrap().len(),
            2
        );

        // The source page is untouched and both pages reopen.
        let original = open_page(&notebook_root, "page-001").unwrap();
        assert_eq!(original.page.revision, 1);
        assert_eq!(original.ink_layers[0].strokes[0].id, "stroke-001");
        assert_eq!(
            copy.manifest
                .pages
                .iter()
                .map(|page| page.id.as_str())
                .collect::<Vec<_>>(),
            ["page-001", "page-002"]
        );
    }

    #[test]
    fn new_pages_land_where_the_writer_put_them() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();

        let order = |snapshot: &NotebookSnapshot| -> Vec<String> {
            snapshot
                .manifest
                .pages
                .iter()
                .map(|page| page.id.clone())
                .collect()
        };

        let appended = create_page(
            &notebook_root,
            "2026-07-25T09:00:00Z",
            &PagePosition::Last,
            None,
            None,
        )
        .unwrap();
        assert_eq!(order(&appended), ["page-001", "page-002"]);

        let before = create_page(
            &notebook_root,
            "2026-07-25T09:01:00Z",
            &PagePosition::Before {
                page_id: "page-001".into(),
            },
            None,
            None,
        )
        .unwrap();
        assert_eq!(order(&before), ["page-003", "page-001", "page-002"]);
        assert_eq!(before.page.id, "page-003");

        let after = create_page(
            &notebook_root,
            "2026-07-25T09:02:00Z",
            &PagePosition::After {
                page_id: "page-001".into(),
            },
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            order(&after),
            ["page-003", "page-001", "page-004", "page-002"]
        );

        // A neighbour the notebook does not have is a stale view, not a reason to append.
        assert!(
            create_page(
                &notebook_root,
                "2026-07-25T09:03:00Z",
                &PagePosition::After {
                    page_id: "page-404".into(),
                },
                None,
                None,
            )
            .is_err()
        );
    }

    #[test]
    fn imports_pdf_pages_consecutively_with_their_own_geometry() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();
        let source =
            store_pdf_reference(&notebook_root, "lecture.pdf", b"%PDF-1.7\nfixture").unwrap();

        let imported = import_pdf_pages(
            &notebook_root,
            "2026-07-29T12:00:00Z",
            &PagePosition::After {
                page_id: "page-001".into(),
            },
            &source,
            &[
                PageGeometry {
                    width_pt: 595.0,
                    height_pt: 842.0,
                },
                PageGeometry {
                    width_pt: 842.0,
                    height_pt: 595.0,
                },
            ],
            None,
            None,
        )
        .unwrap();

        assert_eq!(imported.page.id, "page-002");
        assert_eq!(
            imported
                .manifest
                .pages
                .iter()
                .map(|page| page.id.as_str())
                .collect::<Vec<_>>(),
            ["page-001", "page-002", "page-003"]
        );
        assert!(matches!(
            imported.page.background,
            PageBackground::Pdf { page: 1, .. }
        ));
        let second = open_page(&notebook_root, "page-003").unwrap();
        assert_eq!(second.page.geometry.width_pt, 842.0);
        assert!(matches!(
            second.page.background,
            PageBackground::Pdf { page: 2, .. }
        ));
    }

    #[test]
    fn a_pdf_import_can_replace_the_starter_page() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();
        let source = store_pdf_reference(
            &notebook_root,
            "lecture.pdf",
            b"%PDF-1.7
fixture",
        )
        .unwrap();
        let geometry = PageGeometry {
            width_pt: 595.0,
            height_pt: 842.0,
        };
        let starter = PagePosition::Before {
            page_id: "page-001".into(),
        };

        let imported = import_pdf_pages(
            &notebook_root,
            "2026-11-08T12:00:00Z",
            &starter,
            &source,
            &[geometry.clone(), geometry.clone()],
            None,
            Some("page-001"),
        )
        .unwrap();

        assert_eq!(
            imported
                .manifest
                .pages
                .iter()
                .map(|page| page.id.as_str())
                .collect::<Vec<_>>(),
            ["page-002", "page-003"]
        );
        assert!(matches!(
            imported.page.background,
            PageBackground::Pdf { page: 1, .. }
        ));
        // Naming a page the notebook does not have changes nothing.
        assert!(
            import_pdf_pages(
                &notebook_root,
                "2026-11-08T12:01:00Z",
                &PagePosition::Last,
                &source,
                &[geometry],
                None,
                Some("page-404"),
            )
            .is_err()
        );
        assert_eq!(
            open_notebook(&notebook_root).unwrap().manifest.pages.len(),
            2
        );
    }

    #[test]
    fn a_pdf_import_can_take_chosen_pages() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();
        let source = store_pdf_reference(
            &notebook_root,
            "lecture.pdf",
            b"%PDF-1.7
fixture",
        )
        .unwrap();
        let geometry = PageGeometry {
            width_pt: 595.0,
            height_pt: 842.0,
        };
        let two = [geometry.clone(), geometry.clone()];

        let imported = import_pdf_pages(
            &notebook_root,
            "2026-11-09T12:00:00Z",
            &PagePosition::Last,
            &source,
            &two,
            Some(&[2, 4]),
            None,
        )
        .unwrap();

        assert!(matches!(
            imported.page.background,
            PageBackground::Pdf { page: 2, .. }
        ));
        assert!(matches!(
            open_page(&notebook_root, "page-003")
                .unwrap()
                .page
                .background,
            PageBackground::Pdf { page: 4, .. }
        ));
        // A list that does not match the geometries, a zero or a repeat imports nothing.
        for numbers in [&[2][..], &[0, 1], &[3, 3]] {
            assert!(
                import_pdf_pages(
                    &notebook_root,
                    "2026-11-09T12:01:00Z",
                    &PagePosition::Last,
                    &source,
                    &two,
                    Some(numbers),
                    None,
                )
                .is_err()
            );
        }
        assert_eq!(
            open_notebook(&notebook_root).unwrap().manifest.pages.len(),
            3
        );
    }

    #[test]
    fn deletes_and_reorders_pages_with_guards() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();
        create_page(
            &notebook_root,
            "2026-07-23T19:00:00Z",
            &PagePosition::Last,
            None,
            None,
        )
        .unwrap();
        create_page(
            &notebook_root,
            "2026-07-23T19:01:00Z",
            &PagePosition::Last,
            None,
            None,
        )
        .unwrap();

        let reordered = reorder_pages(
            &notebook_root,
            &["page-003".into(), "page-001".into(), "page-002".into()],
            "2026-07-23T19:02:00Z",
            "page-001",
        )
        .unwrap();
        assert_eq!(
            reordered
                .manifest
                .pages
                .iter()
                .map(|page| page.id.as_str())
                .collect::<Vec<_>>(),
            ["page-003", "page-001", "page-002"]
        );

        let error = reorder_pages(
            &notebook_root,
            &["page-001".into(), "page-002".into()],
            "2026-07-23T19:03:00Z",
            "page-001",
        )
        .unwrap_err();
        assert!(matches!(error, StorageError::InvalidNotebook(_)));

        let after_delete = delete_page(&notebook_root, "page-001", "2026-07-23T19:04:00Z").unwrap();
        assert_eq!(after_delete.manifest.pages.len(), 2);
        // Deletion removes the reference, never the canonical files.
        assert!(notebook_root.join("pages/page-001.json").is_file());
        assert!(notebook_root.join("ink/page-001-layer-001.ink").is_file());

        delete_page(&notebook_root, "page-002", "2026-07-23T19:05:00Z").unwrap();
        let error = delete_page(&notebook_root, "page-003", "2026-07-23T19:06:00Z").unwrap_err();
        assert!(matches!(error, StorageError::InvalidNotebook(_)));
    }

    #[test]
    fn deletes_several_pages_in_one_write_with_guards() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();
        for minute in 0..3 {
            create_page(
                &notebook_root,
                &format!("2026-11-09T10:0{minute}:00Z"),
                &PagePosition::Last,
                None,
                None,
            )
            .unwrap();
        }
        let ids = |names: &[&str]| {
            names
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>()
        };

        // Nothing, an unknown page, or every page is refused and changes nothing.
        for refused in [
            ids(&[]),
            ids(&["page-002", "page-404"]),
            ids(&["page-001", "page-002", "page-003", "page-004"]),
        ] {
            let error = delete_pages(&notebook_root, &refused, "2026-11-09T10:05:00Z", "page-001")
                .unwrap_err();
            assert!(matches!(error, StorageError::InvalidNotebook(_)));
        }
        assert_eq!(
            open_notebook(&notebook_root).unwrap().manifest.pages.len(),
            4
        );

        let kept = delete_pages(
            &notebook_root,
            &ids(&["page-002", "page-004"]),
            "2026-11-09T10:06:00Z",
            "page-001",
        )
        .unwrap();
        assert_eq!(kept.page.id, "page-001");
        assert_eq!(
            kept.manifest
                .pages
                .iter()
                .map(|page| page.id.as_str())
                .collect::<Vec<_>>(),
            ["page-001", "page-003"]
        );
        assert!(notebook_root.join("pages/page-002.json").is_file());

        // Removing the open page opens the one now standing in its place.
        let moved = delete_pages(
            &notebook_root,
            &ids(&["page-001"]),
            "2026-11-09T10:07:00Z",
            "page-001",
        )
        .unwrap();
        assert_eq!(moved.page.id, "page-003");
    }

    #[test]
    fn page_structure_changes_undo_and_redo_without_deleting_page_files() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();
        let mut history = NotebookStructureHistory::default();

        observe_structure(&notebook_root, "page-001", &mut history).unwrap();
        let added = create_page(
            &notebook_root,
            "2026-07-29T10:00:00Z",
            &PagePosition::Last,
            None,
            None,
        )
        .unwrap();
        let added = advance_structure(&notebook_root, &added, &mut history).unwrap();
        assert_eq!(added.snapshot.manifest.pages.len(), 2);
        assert!(added.can_undo && !added.can_redo);

        let undone = undo_structure(&notebook_root, "2026-07-29T10:01:00Z", &mut history).unwrap();
        assert_eq!(undone.snapshot.page.id, "page-001");
        assert_eq!(undone.snapshot.manifest.pages.len(), 1);
        assert!(notebook_root.join("pages/page-002.json").is_file());
        assert!(!undone.can_undo && undone.can_redo);

        let redone = redo_structure(&notebook_root, "2026-07-29T10:02:00Z", &mut history).unwrap();
        assert_eq!(redone.snapshot.page.id, "page-002");
        assert_eq!(redone.snapshot.manifest.pages.len(), 2);
        assert!(redone.can_undo && !redone.can_redo);
    }
    #[test]
    fn calculation_plan_keeps_cell_order_and_excludes_detached_parts() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        create_notebook(root, &snapshot()).unwrap();
        let first = create_calculation_page(
            root,
            "2026-11-02T12:00:00Z",
            &PagePosition::Last,
            "Task A",
            None,
        )
        .unwrap();
        let second = create_calculation_page(
            root,
            "2026-11-02T12:00:01Z",
            &PagePosition::Last,
            "Part 2",
            Some(&first.page.id),
        )
        .unwrap();
        let order = vec![
            "page-001".to_owned(),
            second.page.id.clone(),
            first.page.id.clone(),
        ];
        reorder_pages(root, &order, "2026-11-02T12:00:02Z", &second.page.id).unwrap();
        let plan = calculation_execution_plan(root, &second.page.id).unwrap();
        assert_eq!(
            plan.steps
                .iter()
                .map(|step| &step.page_id)
                .collect::<Vec<_>>(),
            vec![&first.page.id, &second.page.id]
        );
        delete_page(root, &first.page.id, "2026-11-02T12:00:03Z").unwrap();
        let plan = calculation_execution_plan(root, &second.page.id).unwrap();
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].page_id, second.page.id);
    }
}
