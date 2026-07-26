//! Case-insensitive search across a notebook's Typst sources.
//!
//! Read-only, and deliberately without an index: the canonical files are the index. Nothing here
//! writes, so a search can run against a notebook another window is editing.

use std::path::Path;

use serde::Serialize;

use crate::{NotebookManifest, Page, SourceRole, layout};

use super::{StorageError, files::*, invalid, paths::*, validate::*};

const MAX_SEARCH_RESULTS: usize = 200;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub page_id: String,
    pub page_number: usize,
    pub object_id: String,
    pub excerpt: String,
}

/// Case-insensitive search across the notebook's Typst block sources, in manifest page order.
/// The index is the canonical files themselves; nothing is cached or persisted.
pub fn search_notebook(selected_root: &Path, query: &str) -> Result<Vec<SearchHit>, StorageError> {
    let trimmed = query.trim();
    if trimmed.is_empty() || trimmed.len() > 200 {
        return invalid("search text must be between 1 and 200 characters");
    }
    let root = canonical_root(selected_root)?;
    let manifest: NotebookManifest = read_json(&root, layout::MANIFEST)?;
    validate_manifest(&manifest)?;
    let needle = trimmed.to_lowercase();

    let mut hits = Vec::new();
    for (page_index, reference) in manifest.pages.iter().enumerate() {
        let Ok(page) = read_json::<Page>(&root, &reference.path) else {
            continue;
        };
        for object in &page.objects {
            let Some(source_path) = object.searchable_source() else {
                continue;
            };
            let Ok(bytes) = read_limited(
                match resolve_existing(&root, source_path) {
                    Ok(path) => path,
                    Err(_) => continue,
                },
                SourceRole::Block.max_bytes(),
            ) else {
                continue;
            };
            let source = String::from_utf8_lossy(&bytes);
            let lowered = source.to_lowercase();
            let Some(position) = lowered.find(&needle) else {
                continue;
            };
            hits.push(SearchHit {
                page_id: page.id.clone(),
                page_number: page_index + 1,
                object_id: object.fields().id.clone(),
                excerpt: excerpt_around(&source, position, needle.len()),
            });
            if hits.len() >= MAX_SEARCH_RESULTS {
                return Ok(hits);
            }
        }
    }
    Ok(hits)
}

fn excerpt_around(source: &str, position: usize, length: usize) -> String {
    const CONTEXT: usize = 40;
    // The match position was found in a lowercased copy whose byte offsets can differ from the
    // original for non-ASCII text; clamp and slice only at original char boundaries.
    let position = position.min(source.len());
    let start = source
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= position.saturating_sub(CONTEXT))
        .last()
        .unwrap_or(0);
    let end = source
        .char_indices()
        .map(|(index, _)| index)
        .find(|index| *index >= (position + length + CONTEXT).min(source.len()))
        .unwrap_or(source.len());
    source[start..end]
        .replace(['\n', '\r'], " ")
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::storage::{fixtures::*, *};

    #[test]
    fn searches_typst_sources_across_pages() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();
        duplicate_page(&notebook_root, "page-001", "2026-07-23T19:00:00Z").unwrap();

        let hits = search_notebook(&notebook_root, "F = m a").unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(
            (hits[0].page_number, hits[0].page_id.as_str()),
            (1, "page-001")
        );
        assert_eq!(
            (hits[1].page_number, hits[1].page_id.as_str()),
            (2, "page-002")
        );
        assert!(hits[0].excerpt.contains("F = m a"));
        // Case-insensitive; no hit for absent text.
        assert_eq!(search_notebook(&notebook_root, "f = M A").unwrap().len(), 2);
        assert!(
            search_notebook(&notebook_root, "entropy")
                .unwrap()
                .is_empty()
        );
        assert!(search_notebook(&notebook_root, "   ").is_err());
    }
}
