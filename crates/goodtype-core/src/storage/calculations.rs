//! Shared calculation history is one document unit, independent of presentation pages.
//!
//! Every operation reads the calculation from disk under the caller's notebook lock, refuses
//! when the file differs from the last state this history confirmed, and writes through the
//! existing recoverable transaction. One history exists per notebook root and calculation.
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{
    NotebookSnapshot, StorageError, StoredFile, invalid_error, open_page, paths::canonical_root,
    write::save_transactional,
};
use crate::calculation::{
    Calculation, CalculationReference, Cell, MAX_CELL_SOURCE_BYTES, MAX_PROVENANCE_BYTES,
    MAX_TITLE_BYTES, source_hash, valid_id,
};

const HISTORY_BYTES: usize = 64 * 1024 * 1024;
const HISTORY_COUNT: usize = 100;

#[derive(Default, Debug)]
pub struct CalculationHistory {
    /// The bytes this history last confirmed. Any other bytes on disk are an external change.
    current: Option<Vec<u8>>,
    undo: Vec<Entry>,
    redo: Vec<Entry>,
}

impl CalculationHistory {
    /// Explicit recovery replaces the document. Retain its confirmed predecessor for one undo.
    pub fn record_restore(&mut self, previous: Vec<u8>, restored: Vec<u8>, part_id: String) {
        self.undo.clear();
        self.redo.clear();
        push(
            &mut self.undo,
            Entry {
                bytes: previous,
                part_id,
            },
        );
        self.current = Some(restored);
    }
}

#[derive(Debug)]
struct Entry {
    bytes: Vec<u8>,
    part_id: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalculationResult {
    pub reference: CalculationReference,
    pub document: Calculation,
    pub can_undo: bool,
    pub can_redo: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum CellOperation {
    Source {
        cell_id: String,
        expected_source: String,
        source: String,
    },
    Insert {
        cell_id: String,
        after: Option<String>,
        cell_type: String,
    },
    Delete {
        cell_id: String,
    },
    Move {
        cell_id: String,
        before: Option<String>,
    },
    CellType {
        cell_id: String,
        cell_type: String,
    },
    Rename {
        title: String,
    },
}

struct Loaded {
    snapshot: NotebookSnapshot,
    document: Calculation,
    bytes: Vec<u8>,
    part_id: String,
}

fn load(root: &Path, page_id: &str) -> Result<Loaded, StorageError> {
    let snapshot = open_page(root, page_id)?;
    let reference = snapshot
        .page
        .calculation
        .as_ref()
        .ok_or_else(|| invalid_error("page is not computational"))?;
    let part_id = reference.part_id.clone();
    let bytes = snapshot
        .calculations
        .iter()
        .find(|file| file.path == reference.source_path)
        .ok_or_else(|| invalid_error("calculation source is missing"))?
        .bytes
        .clone();
    let document = Calculation::parse(&bytes)?;
    if document.id() != reference.id {
        return Err(invalid_error(
            "calculation identity does not match the page",
        ));
    }
    Ok(Loaded {
        snapshot,
        document,
        bytes,
        part_id,
    })
}

fn observe(history: &mut CalculationHistory, bytes: &[u8]) -> Result<(), StorageError> {
    if history
        .current
        .as_deref()
        .is_some_and(|known| known != bytes)
    {
        return Err(StorageError::ExternalChange(
            "calculation changed outside this session; preserve the draft and reload".into(),
        ));
    }
    if history.current.is_none() {
        history.current = Some(bytes.to_vec());
    }
    Ok(())
}

fn result(
    snapshot: &NotebookSnapshot,
    document: Calculation,
    history: &CalculationHistory,
) -> CalculationResult {
    CalculationResult {
        reference: snapshot
            .page
            .calculation
            .clone()
            .expect("loaded calculation page"),
        document,
        can_undo: !history.undo.is_empty(),
        can_redo: !history.redo.is_empty(),
    }
}

pub fn read_calculation(
    root: &Path,
    page_id: &str,
    history: &mut CalculationHistory,
) -> Result<CalculationResult, StorageError> {
    let loaded = load(root, page_id)?;
    observe(history, &loaded.bytes)?;
    Ok(result(&loaded.snapshot, loaded.document, history))
}

fn push(stack: &mut Vec<Entry>, entry: Entry) {
    if entry.bytes.len() > HISTORY_BYTES {
        // An entry that cannot be retained invalidates every older one: skipping it would let a
        // later undo jump over this action.
        stack.clear();
        return;
    }
    let mut total = stack.iter().map(|entry| entry.bytes.len()).sum::<usize>();
    while !stack.is_empty()
        && (stack.len() >= HISTORY_COUNT || total + entry.bytes.len() > HISTORY_BYTES)
    {
        total -= stack.remove(0).bytes.len();
    }
    stack.push(entry);
}

fn persist(
    root: &Path,
    current: &NotebookSnapshot,
    document: &Calculation,
    modified_at: &str,
) -> Result<Vec<u8>, StorageError> {
    if modified_at.is_empty() || modified_at.len() > 64 {
        return Err(invalid_error(
            "calculation timestamp must be present and bounded",
        ));
    }
    let reference = current
        .page
        .calculation
        .as_ref()
        .ok_or_else(|| invalid_error("page is not computational"))?;
    if document.id() != reference.id {
        return Err(invalid_error("calculation identity changed"));
    }
    let bytes = document.bytes()?;
    let mut candidate = current.clone();
    candidate.calculations = vec![StoredFile {
        path: reference.source_path.clone(),
        bytes: bytes.clone(),
    }];
    candidate.page.revision = current
        .page
        .revision
        .checked_add(1)
        .ok_or_else(|| invalid_error("page revision overflow"))?;
    candidate.manifest.modified_at = modified_at.to_owned();
    save_transactional(root, current, &candidate)?;
    Ok(bytes)
}

/// Write `document` as the next revision and record `previous` as one undoable action.
fn commit(
    root: &Path,
    loaded: Loaded,
    mut document: Calculation,
    history: &mut CalculationHistory,
    modified_at: &str,
) -> Result<CalculationResult, StorageError> {
    document.set_revision_after(loaded.document.revision())?;
    let bytes = persist(root, &loaded.snapshot, &document, modified_at)?;
    push(
        &mut history.undo,
        Entry {
            bytes: loaded.bytes,
            part_id: loaded.part_id,
        },
    );
    history.redo.clear();
    history.current = Some(bytes);
    Ok(result(&loaded.snapshot, document, history))
}

fn owned_cell(document: &Calculation, part_id: &str, cell_id: &str) -> Result<usize, StorageError> {
    document
        .cells
        .iter()
        .position(|cell| cell.id == cell_id && cell.part_id() == Some(part_id))
        .ok_or_else(|| invalid_error("cell does not belong to the requested calculation part"))
}

fn valid_cell_type(cell_type: &str) -> Result<(), StorageError> {
    if matches!(cell_type, "code" | "markdown" | "raw") {
        Ok(())
    } else {
        Err(invalid_error("unsupported cell type"))
    }
}

/// Execution results describe a source and type that no longer exist after a type change.
fn clear_execution(cell: &mut Cell) {
    cell.outputs.clear();
    cell.execution_count = None;
    let namespace = cell.goodtype_mut();
    namespace.remove("execution");
    namespace.remove("lastSuccess");
}

fn mark_later_outputs(document: &mut Calculation, from: usize) {
    for cell in document.cells.iter_mut().skip(from) {
        if cell.cell_type == "code" && !cell.outputs.is_empty() {
            cell.goodtype_mut()
                .insert("earlierCellsChanged".into(), json!(true));
        }
    }
}

pub fn edit_calculation(
    root: &Path,
    page_id: &str,
    history: &mut CalculationHistory,
    operation: CellOperation,
    modified_at: &str,
) -> Result<CalculationResult, StorageError> {
    let root = canonical_root(root)?;
    let loaded = load(&root, page_id)?;
    observe(history, &loaded.bytes)?;
    let part_id = loaded.part_id.clone();
    let mut document = loaded.document.clone();
    let mut changed_after = None;
    match operation {
        CellOperation::Source {
            cell_id,
            expected_source,
            source,
        } => {
            let index = owned_cell(&document, &part_id, &cell_id)?;
            if document.cells[index].source != expected_source {
                return Err(StorageError::ExternalChange(
                    "cell source changed; preserve the draft and reload".into(),
                ));
            }
            if source.len() > MAX_CELL_SOURCE_BYTES {
                return Err(invalid_error("cell source exceeds the size limit"));
            }
            if document.cells[index].source == source {
                return Ok(result(&loaded.snapshot, document, history));
            }
            document.cells[index].source = source;
            if document.cells[index].cell_type == "code" {
                changed_after = Some(index + 1);
            }
        }
        CellOperation::Insert {
            cell_id,
            after,
            cell_type,
        } => {
            valid_cell_type(&cell_type)?;
            if !valid_id(&cell_id) || document.cells.iter().any(|cell| cell.id == cell_id) {
                return Err(invalid_error(
                    "new cell identity is invalid or already exists",
                ));
            }
            let index = match after {
                Some(after) => owned_cell(&document, &part_id, &after)? + 1,
                None => match document
                    .cells
                    .iter()
                    .position(|cell| cell.part_id() == Some(&part_id))
                {
                    Some(first) => first,
                    None => document.empty_part_position(&part_id)?,
                },
            };
            document
                .cells
                .insert(index, Cell::new(cell_id, &part_id, &cell_type));
            if cell_type == "code" {
                changed_after = Some(index + 1);
            }
        }
        CellOperation::Delete { cell_id } => {
            let index = owned_cell(&document, &part_id, &cell_id)?;
            if document.cells[index].cell_type == "code" {
                changed_after = Some(index);
            }
            document.cells.remove(index);
        }
        CellOperation::Move { cell_id, before } => {
            let index = owned_cell(&document, &part_id, &cell_id)?;
            if before.as_deref() == Some(&cell_id) {
                return Ok(result(&loaded.snapshot, document, history));
            }
            if let Some(before) = &before {
                owned_cell(&document, &part_id, before)?;
            }
            let cell = document.cells.remove(index);
            let target = match before {
                Some(before) => owned_cell(&document, &part_id, &before)?,
                None => document
                    .cells
                    .iter()
                    .rposition(|cell| cell.part_id() == Some(&part_id))
                    .map_or(index.min(document.cells.len()), |last| last + 1),
            };
            if cell.cell_type == "code" {
                changed_after = Some(index.min(target));
            }
            document.cells.insert(target, cell);
            if document.cells == loaded.document.cells {
                return Ok(result(&loaded.snapshot, document, history));
            }
        }
        CellOperation::CellType { cell_id, cell_type } => {
            valid_cell_type(&cell_type)?;
            let index = owned_cell(&document, &part_id, &cell_id)?;
            if document.cells[index].cell_type == cell_type {
                return Ok(result(&loaded.snapshot, document, history));
            }
            if document.cells[index].cell_type == "code" || cell_type == "code" {
                changed_after = Some(index + 1);
            }
            document.cells[index].cell_type = cell_type;
            clear_execution(&mut document.cells[index]);
        }
        CellOperation::Rename { title } => {
            if title.len() > MAX_TITLE_BYTES || title.trim().is_empty() {
                return Err(invalid_error("calculation title is invalid"));
            }
            document.goodtype_mut().insert("title".into(), json!(title));
        }
    }
    if let Some(from) = changed_after {
        mark_later_outputs(&mut document, from);
    }
    commit(&root, loaded, document, history, modified_at)
}

/// Undo or redo the latest calculation action. A page-scoped request refuses an action that
/// another part made. Restoring a whole earlier state is exact here: the history only ever
/// restores its newest entry, and `observe` proves that no other writer changed the file since.
pub fn calculation_history(
    root: &Path,
    page_id: &str,
    history: &mut CalculationHistory,
    undo: bool,
    page_scope: bool,
    modified_at: &str,
) -> Result<CalculationResult, StorageError> {
    let root = canonical_root(root)?;
    let loaded = load(&root, page_id)?;
    observe(history, &loaded.bytes)?;
    let entry = if undo {
        history.undo.last()
    } else {
        history.redo.last()
    }
    .ok_or_else(|| invalid_error("nothing to undo or redo in this calculation"))?;
    if page_scope && entry.part_id != loaded.part_id {
        return Err(invalid_error(
            "the latest calculation change belongs to another part; use notebook undo",
        ));
    }
    let origin = entry.part_id.clone();
    let document = Calculation::parse(&entry.bytes)?;
    let mut written = document.clone();
    // Monotonic: the restored content receives a revision newer than the current one.
    written.set_revision_after(loaded.document.revision())?;
    let bytes = persist(&root, &loaded.snapshot, &written, modified_at)?;
    let inverse = Entry {
        bytes: loaded.bytes,
        part_id: origin,
    };
    if undo {
        history.undo.pop();
        push(&mut history.redo, inverse);
    } else {
        history.redo.pop();
        push(&mut history.undo, inverse);
    }
    history.current = Some(bytes);
    Ok(result(&loaded.snapshot, written, history))
}

fn is_error(outputs: &[Value]) -> bool {
    outputs
        .iter()
        .any(|output| output.get("output_type").and_then(Value::as_str) == Some("error"))
}

/// Admit one completed execution result. The record names the exact executed source by hash,
/// so a later source edit shows the output as older instead of silently matching it.
/// A failed run keeps the last successful outputs and their provenance beside the error.
#[allow(clippy::too_many_arguments)]
pub fn commit_calculation_outputs(
    root: &Path,
    page_id: &str,
    history: &mut CalculationHistory,
    cell_id: &str,
    executed_source: &str,
    outputs: Vec<Value>,
    sequence: u64,
    provenance: Value,
    modified_at: &str,
) -> Result<CalculationResult, StorageError> {
    if executed_source.len() > MAX_CELL_SOURCE_BYTES {
        return Err(invalid_error("executed source exceeds the size limit"));
    }
    if !provenance.is_object() || serde_json::to_vec(&provenance)?.len() > MAX_PROVENANCE_BYTES {
        return Err(invalid_error("execution provenance must be a small object"));
    }
    let root = canonical_root(root)?;
    let loaded = load(&root, page_id)?;
    observe(history, &loaded.bytes)?;
    let mut document = loaded.document.clone();
    let index = owned_cell(&document, &loaded.part_id, cell_id)?;
    let cell = &mut document.cells[index];
    if cell.cell_type != "code" {
        return Err(invalid_error("only code cells can execute"));
    }
    let failed = is_error(&outputs);
    let previous_outputs = std::mem::replace(&mut cell.outputs, outputs);
    let previous_count = cell.execution_count.replace(sequence);
    let namespace = cell.goodtype_mut();
    let previous_execution = namespace.get("execution").cloned();
    namespace.remove("earlierCellsChanged");
    if let Some(origin) = namespace.remove("copiedFrom") {
        namespace.insert("sourceOrigin".into(), origin);
    }
    if failed {
        // Replace the last success only with a newer success; an earlier error keeps it.
        if !previous_outputs.is_empty() && !is_error(&previous_outputs) {
            namespace.insert(
                "lastSuccess".into(),
                json!({
                    "outputs": previous_outputs,
                    "executionCount": previous_count,
                    "execution": previous_execution,
                }),
            );
        }
    } else {
        namespace.remove("lastSuccess");
    }
    namespace.insert(
        "execution".into(),
        json!({
            "sourceHash": source_hash(executed_source),
            "provenance": provenance,
            "finishedAt": modified_at,
            "status": if failed { "error" } else { "ok" },
        }),
    );
    mark_later_outputs(&mut document, index + 1);
    commit(&root, loaded, document, history, modified_at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{
        PagePosition, create_calculation_page, create_notebook, duplicate_page, fixtures,
    };
    const TIME: &str = "2026-11-02T12:00:00Z";

    fn setup() -> (tempfile::TempDir, NotebookSnapshot) {
        let temp = tempfile::tempdir().unwrap();
        create_notebook(temp.path(), &fixtures::snapshot()).unwrap();
        let snapshot =
            create_calculation_page(temp.path(), TIME, &PagePosition::Last, "Task A", None)
                .unwrap();
        (temp, snapshot)
    }

    fn first_cell(result: &CalculationResult) -> &Cell {
        result
            .document
            .cells
            .iter()
            .find(|cell| cell.part_id() == Some(&result.reference.part_id))
            .unwrap()
    }

    fn source(
        root: &Path,
        page: &str,
        history: &mut CalculationHistory,
        value: &str,
    ) -> CalculationResult {
        let current = read_calculation(root, page, history).unwrap();
        let cell = first_cell(&current);
        let operation = CellOperation::Source {
            cell_id: cell.id.clone(),
            expected_source: cell.source.clone(),
            source: value.into(),
        };
        edit_calculation(root, page, history, operation, TIME).unwrap()
    }

    fn disk(root: &Path, reference: &CalculationReference) -> Calculation {
        Calculation::parse(&std::fs::read(root.join(&reference.source_path)).unwrap()).unwrap()
    }

    #[test]
    fn v4_creation_preserves_legacy_pages_and_shared_history_preserves_other_parts() {
        let (temp, first) = setup();
        let root = temp.path();
        assert_eq!(first.manifest.schema_version, 4);
        assert_eq!(open_page(root, "page-001").unwrap().page.schema_version, 3);
        let second = create_calculation_page(
            root,
            TIME,
            &PagePosition::Last,
            "Part 2",
            Some(&first.page.id),
        )
        .unwrap();
        assert_eq!(
            first.page.calculation.as_ref().unwrap().id,
            second.page.calculation.as_ref().unwrap().id
        );
        let mut history = CalculationHistory::default();
        source(root, &first.page.id, &mut history, "x = 2");
        source(root, &second.page.id, &mut history, "x + 3");
        assert!(calculation_history(root, &first.page.id, &mut history, true, true, TIME).is_err());
        let undo =
            calculation_history(root, &second.page.id, &mut history, true, true, TIME).unwrap();
        assert_eq!(undo.document.cells[0].source, "x = 2");
        assert_eq!(undo.document.cells[1].source, "");
        let redo =
            calculation_history(root, &second.page.id, &mut history, false, true, TIME).unwrap();
        assert_eq!(redo.document.cells[1].source, "x + 3");
        let copy = duplicate_page(root, &second.page.id, TIME).unwrap();
        let copied =
            read_calculation(root, &copy.page.id, &mut CalculationHistory::default()).unwrap();
        assert_ne!(copied.reference.id, redo.reference.id);
        assert_eq!(copied.document.cells.len(), 1);
        assert_eq!(copied.document.cells[0].source, "x + 3");
        assert!(copied.document.cells[0].metadata["goodtype"]["copiedFrom"].is_object());
    }

    #[test]
    fn interleaved_parts_undo_in_commit_order_with_monotonic_revisions() {
        let (temp, first) = setup();
        let root = temp.path();
        let second = create_calculation_page(
            root,
            TIME,
            &PagePosition::Last,
            "Part 2",
            Some(&first.page.id),
        )
        .unwrap();
        let mut history = CalculationHistory::default();
        let mut revisions = vec![
            source(root, &first.page.id, &mut history, "a = 1")
                .document
                .revision(),
        ];
        revisions.push(
            source(root, &second.page.id, &mut history, "b = a")
                .document
                .revision(),
        );
        revisions.push(
            source(root, &first.page.id, &mut history, "a = 2")
                .document
                .revision(),
        );
        // A-edit, B-edit, A-edit, then A-undo removes only A's latest edit.
        let undo =
            calculation_history(root, &first.page.id, &mut history, true, true, TIME).unwrap();
        revisions.push(undo.document.revision());
        assert_eq!(
            (
                undo.document.cells[0].source.as_str(),
                undo.document.cells[1].source.as_str()
            ),
            ("a = 1", "b = a")
        );
        // The next page-scoped A-undo would remove B's edit: refuse it, keep both sources.
        let refusal = calculation_history(root, &first.page.id, &mut history, true, true, TIME);
        assert!(refusal.is_err());
        assert_eq!(disk(root, &undo.reference).cells[1].source, "b = a");
        // Notebook-scoped undo follows the actual commit order.
        let notebook =
            calculation_history(root, &first.page.id, &mut history, true, false, TIME).unwrap();
        revisions.push(notebook.document.revision());
        assert_eq!(notebook.document.cells[1].source, "");
        assert!(
            revisions.windows(2).all(|pair| pair[0] < pair[1]),
            "{revisions:?}"
        );
        // A new action clears redo.
        source(root, &second.page.id, &mut history, "b = 3");
        assert!(
            calculation_history(root, &second.page.id, &mut history, false, false, TIME).is_err()
        );
    }

    #[test]
    fn cell_ownership_and_external_edits_refuse_without_overwriting_sources() {
        let (temp, first) = setup();
        let root = temp.path();
        let second =
            create_calculation_page(root, TIME, &PagePosition::Last, "Task B", None).unwrap();
        let mut history = CalculationHistory::default();
        let known = source(root, &first.page.id, &mut history, "x = 2");
        let other =
            read_calculation(root, &second.page.id, &mut CalculationHistory::default()).unwrap();
        let operation = CellOperation::Delete {
            cell_id: other.document.cells[0].id.clone(),
        };
        assert!(edit_calculation(root, &first.page.id, &mut history, operation, TIME).is_err());
        let mut external = known.document.clone();
        external.cells[0].source = "external edit".into();
        let path = root.join(&known.reference.source_path);
        std::fs::write(&path, external.bytes().unwrap()).unwrap();
        assert!(matches!(
            read_calculation(root, &first.page.id, &mut history),
            Err(StorageError::ExternalChange(_))
        ));
        let operation = CellOperation::Insert {
            cell_id: "late".into(),
            after: None,
            cell_type: "code".into(),
        };
        assert!(matches!(
            edit_calculation(root, &first.page.id, &mut history, operation, TIME),
            Err(StorageError::ExternalChange(_))
        ));
        assert!(matches!(
            calculation_history(root, &first.page.id, &mut history, true, false, TIME),
            Err(StorageError::ExternalChange(_))
        ));
        assert_eq!(
            disk(root, &known.reference).cells[0].source,
            "external edit"
        );
        assert_eq!(disk(root, &known.reference).cells.len(), 1);
    }

    #[test]
    fn stale_source_draft_refuses_before_overwriting_newer_source() {
        let (temp, first) = setup();
        let root = temp.path();
        let mut history = CalculationHistory::default();
        let older = read_calculation(root, &first.page.id, &mut history).unwrap();
        source(root, &first.page.id, &mut history, "newer");
        let stale = CellOperation::Source {
            cell_id: older.document.cells[0].id.clone(),
            expected_source: older.document.cells[0].source.clone(),
            source: "draft from an older view".into(),
        };
        assert!(matches!(
            edit_calculation(root, &first.page.id, &mut history, stale, TIME),
            Err(StorageError::ExternalChange(_))
        ));
        assert_eq!(disk(root, &older.reference).cells[0].source, "newer");
    }

    #[test]
    fn invalid_operations_leave_the_file_unchanged() {
        let (temp, first) = setup();
        let root = temp.path();
        let mut history = CalculationHistory::default();
        let known = read_calculation(root, &first.page.id, &mut history).unwrap();
        let path = root.join(&known.reference.source_path);
        let before = std::fs::read(&path).unwrap();
        let cell = known.document.cells[0].id.clone();
        let operations = [
            CellOperation::Insert {
                cell_id: "x".into(),
                after: None,
                cell_type: "widget".into(),
            },
            CellOperation::Insert {
                cell_id: cell.clone(),
                after: None,
                cell_type: "code".into(),
            },
            CellOperation::Insert {
                cell_id: "bad id".into(),
                after: None,
                cell_type: "code".into(),
            },
            CellOperation::CellType {
                cell_id: cell.clone(),
                cell_type: "html".into(),
            },
            CellOperation::Source {
                cell_id: cell.clone(),
                expected_source: String::new(),
                source: "x".repeat(MAX_CELL_SOURCE_BYTES + 1),
            },
            CellOperation::Rename { title: " ".into() },
            CellOperation::Move {
                cell_id: "missing".into(),
                before: None,
            },
        ];
        for operation in operations {
            assert!(edit_calculation(root, &first.page.id, &mut history, operation, TIME).is_err());
        }
        let unchanged = CellOperation::Source {
            cell_id: cell,
            expected_source: String::new(),
            source: String::new(),
        };
        assert!(
            !edit_calculation(root, &first.page.id, &mut history, unchanged, TIME)
                .unwrap()
                .can_undo
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn cells_inserted_into_an_emptied_part_keep_its_execution_position() {
        let (temp, first) = setup();
        let root = temp.path();
        let second = create_calculation_page(
            root,
            TIME,
            &PagePosition::Last,
            "Part 2",
            Some(&first.page.id),
        )
        .unwrap();
        create_calculation_page(
            root,
            TIME,
            &PagePosition::Last,
            "Part 3",
            Some(&second.page.id),
        )
        .unwrap();
        let mut history = CalculationHistory::default();
        let current = read_calculation(root, &second.page.id, &mut history).unwrap();
        let only = first_cell(&current).id.clone();
        edit_calculation(
            root,
            &second.page.id,
            &mut history,
            CellOperation::Delete { cell_id: only },
            TIME,
        )
        .unwrap();
        let operation = CellOperation::Insert {
            cell_id: "again".into(),
            after: None,
            cell_type: "markdown".into(),
        };
        let result =
            edit_calculation(root, &second.page.id, &mut history, operation, TIME).unwrap();
        assert_eq!(result.document.cells[1].id, "again");
        assert_eq!(result.document.cells.len(), 3);
    }

    #[test]
    fn saved_results_keep_executed_source_identity_and_last_success_on_error() {
        let (temp, first) = setup();
        let root = temp.path();
        let mut history = CalculationHistory::default();
        let known = source(root, &first.page.id, &mut history, "2 + 2");
        let cell = &known.document.cells[0].id;
        let output = json!({"output_type":"execute_result","data":{"text/plain":"4"},"metadata":{},"execution_count":1});
        commit_calculation_outputs(
            root,
            &first.page.id,
            &mut history,
            cell,
            "2 + 2",
            vec![output.clone()],
            1,
            json!({"sessionId":"test"}),
            TIME,
        )
        .unwrap();
        source(root, &first.page.id, &mut history, "1 / 0");
        let failure = json!({"output_type":"error","ename":"ZeroDivisionError","evalue":"division by zero","traceback":[]});
        let result = commit_calculation_outputs(
            root,
            &first.page.id,
            &mut history,
            cell,
            "1 / 0",
            vec![failure.clone()],
            2,
            json!({}),
            TIME,
        )
        .unwrap();
        let namespace = &result.document.cells[0].metadata["goodtype"];
        assert_eq!(namespace["lastSuccess"]["outputs"], json!([output]));
        assert_eq!(
            namespace["lastSuccess"]["execution"]["sourceHash"],
            source_hash("2 + 2")
        );
        assert_eq!(namespace["execution"]["sourceHash"], source_hash("1 / 0"));
        assert_eq!(namespace["execution"]["status"], "error");
        // A second error keeps the earlier success rather than replacing it with an error.
        let again = commit_calculation_outputs(
            root,
            &first.page.id,
            &mut history,
            cell,
            "1 / 0",
            vec![failure],
            3,
            json!({}),
            TIME,
        )
        .unwrap();
        assert_eq!(
            again.document.cells[0].metadata["goodtype"]["lastSuccess"]["outputs"],
            json!([output])
        );
        let reopened =
            read_calculation(root, &first.page.id, &mut CalculationHistory::default()).unwrap();
        assert_eq!(reopened.document, again.document);
        // A later success supersedes the retained record.
        let fixed = commit_calculation_outputs(
            root,
            &first.page.id,
            &mut history,
            cell,
            "1 / 0",
            vec![output],
            4,
            json!({}),
            TIME,
        )
        .unwrap();
        assert!(
            fixed.document.cells[0].metadata["goodtype"]
                .get("lastSuccess")
                .is_none()
        );
    }

    #[test]
    fn hostile_outputs_and_provenance_are_refused_before_writing() {
        let (temp, first) = setup();
        let root = temp.path();
        let mut history = CalculationHistory::default();
        let known = read_calculation(root, &first.page.id, &mut history).unwrap();
        let path = root.join(&known.reference.source_path);
        let before = std::fs::read(&path).unwrap();
        let cell = known.document.cells[0].id.clone();
        let commit = |history: &mut CalculationHistory, outputs: Vec<Value>, provenance: Value| {
            commit_calculation_outputs(
                root,
                &first.page.id,
                history,
                &cell,
                "",
                outputs,
                1,
                provenance,
                TIME,
            )
        };
        assert!(
            commit(
                &mut history,
                vec![json!({"output_type":"widget"})],
                json!({})
            )
            .is_err()
        );
        assert!(
            commit(
                &mut history,
                vec![json!({"output_type":"stream","name":"stdout","text":5})],
                json!({})
            )
            .is_err()
        );
        assert!(commit(&mut history, Vec::new(), json!("not an object")).is_err());
        assert!(
            commit(
                &mut history,
                Vec::new(),
                json!({"padding":"x".repeat(MAX_PROVENANCE_BYTES)})
            )
            .is_err()
        );
        let flood = vec![
            json!({"output_type":"stream","name":"stdout","text":"x".repeat(1024 * 1024)});
            17
        ];
        assert!(commit(&mut history, flood, json!({})).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn changing_cell_type_removes_execution_records() {
        let (temp, first) = setup();
        let root = temp.path();
        let mut history = CalculationHistory::default();
        let known = source(root, &first.page.id, &mut history, "print(1)");
        let cell = known.document.cells[0].id.clone();
        let output = json!({"output_type":"stream","name":"stdout","text":"1\n"});
        commit_calculation_outputs(
            root,
            &first.page.id,
            &mut history,
            &cell,
            "print(1)",
            vec![output],
            1,
            json!({}),
            TIME,
        )
        .unwrap();
        let operation = CellOperation::CellType {
            cell_id: cell,
            cell_type: "markdown".into(),
        };
        let result = edit_calculation(root, &first.page.id, &mut history, operation, TIME).unwrap();
        let markdown = &result.document.cells[0];
        assert!(markdown.outputs.is_empty() && markdown.execution_count.is_none());
        assert!(markdown.goodtype().unwrap().get("execution").is_none());
        let value: Value = serde_json::from_slice(
            &std::fs::read(root.join(&result.reference.source_path)).unwrap(),
        )
        .unwrap();
        assert!(value["cells"][0].get("outputs").is_none());
    }
    #[test]
    fn earlier_edits_mark_later_outputs_and_execution_clears_copy_status() {
        let (temp, first) = setup();
        let root = temp.path();
        let mut history = CalculationHistory::default();
        let initial = read_calculation(root, &first.page.id, &mut history).unwrap();
        let first_cell = initial.document.cells[0].id.clone();
        edit_calculation(
            root,
            &first.page.id,
            &mut history,
            CellOperation::Insert {
                cell_id: "later".into(),
                after: Some(first_cell.clone()),
                cell_type: "code".into(),
            },
            TIME,
        )
        .unwrap();
        let output = json!({"output_type":"stream","name":"stdout","text":"2"});
        commit_calculation_outputs(
            root,
            &first.page.id,
            &mut history,
            "later",
            "",
            vec![output.clone()],
            1,
            json!({}),
            TIME,
        )
        .unwrap();
        let edited = edit_calculation(
            root,
            &first.page.id,
            &mut history,
            CellOperation::Source {
                cell_id: first_cell,
                expected_source: "".into(),
                source: "x = 3".into(),
            },
            TIME,
        )
        .unwrap();
        assert_eq!(
            edited.document.cells[1].metadata["goodtype"]["earlierCellsChanged"],
            true
        );
        let copy = duplicate_page(root, &first.page.id, TIME).unwrap();
        let mut copied_history = CalculationHistory::default();
        let copied = read_calculation(root, &copy.page.id, &mut copied_history).unwrap();
        let executed = commit_calculation_outputs(
            root,
            &copy.page.id,
            &mut copied_history,
            &copied.document.cells[1].id,
            "",
            vec![output],
            1,
            json!({}),
            TIME,
        )
        .unwrap();
        assert!(
            executed.document.cells[1].metadata["goodtype"]
                .get("copiedFrom")
                .is_none()
        );
        assert!(executed.document.cells[1].metadata["goodtype"]["sourceOrigin"].is_object());
    }
}
