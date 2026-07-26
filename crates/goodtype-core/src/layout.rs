//! The on-disk shape of a notebook: what things are called and where they live.
//!
//! Every name the store writes and every name it checks comes from here. These used to be
//! string literals spelled out separately in the code that builds a path and the code that
//! validates one — so a page's ink layer was named in one function and verified in another,
//! and the two could disagree without anything failing to compile.

use crate::SourceRole;

/// The notebook manifest, at the root of the notebook directory.
pub const MANIFEST: &str = "goodtype.json";

/// Typst preamble shared by every page. The one file that belongs to the notebook rather than
/// to a page, which is why it sits at the root instead of in a role directory.
pub const SHARED_STYLE: &str = "style.typ";

pub const PAGES_DIR: &str = "pages";
pub const INK_DIR: &str = "ink";

/// Store-owned state that is not part of the document.
pub const INTERNAL_DIR: &str = ".goodtype";
pub const PENDING_TRANSACTION: &str = ".goodtype/pending-transaction.json";
pub const RECOVERY_DIR: &str = ".goodtype/recovery";

pub fn page_id(number: usize) -> String {
    format!("page-{number:03}")
}

pub fn page_path(page_id: &str) -> String {
    format!("{PAGES_DIR}/{page_id}.json")
}

pub fn ink_layer_id(page_id: &str, number: usize) -> String {
    format!("{page_id}-ink-{number:03}")
}

pub fn ink_layer_path(page_id: &str, number: usize) -> String {
    format!("{INK_DIR}/{page_id}-layer-{number:03}.json")
}

pub fn object_id(page_id: &str, number: usize) -> String {
    format!("{page_id}-obj-{number:03}")
}

/// Four digits: a page holds up to `MAX_INK_STROKES_PER_LAYER` strokes, which is well past what
/// three would order correctly.
pub fn stroke_id(page_id: &str, number: usize) -> String {
    format!("{page_id}-stroke-{number:04}")
}

pub fn block_path(page_id: &str, number: usize) -> String {
    format!(
        "{}/{page_id}-block-{number:03}.typ",
        SourceRole::Block.directory()
    )
}

pub fn asset_path(filename: &str) -> String {
    format!("{}/{filename}", SourceRole::Asset.directory())
}

pub fn recovery_path(file_name: &str) -> String {
    format!("{RECOVERY_DIR}/{file_name}")
}

/// An archived candidate from a transaction that was interrupted before it could finish. The
/// suffix breaks ties when two archives land inside one timestamp tick.
pub fn interrupted_candidate(revision: u64, timestamp: u128, suffix: usize) -> String {
    format!("interrupted-r{revision}-{timestamp}-{suffix}.json")
}

/// Whether a file name in the recovery directory is one the store wrote.
///
/// The inverse of [`interrupted_candidate`], kept beside it so the two cannot drift: a name
/// this rejects is a file the store will refuse to read back.
pub fn is_candidate_name(file_name: &str) -> bool {
    file_name.strip_prefix("interrupted-r").is_some_and(|rest| {
        rest.strip_suffix(".json").is_some_and(|stem| {
            let mut parts = stem.split('-');
            parts.clone().count() == 3 && parts.all(|part| part.bytes().all(|b| b.is_ascii_digit()))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The naming scheme and the check that reads it back are one pair, not two independent
    /// rules — this is the assertion that keeps them that way.
    #[test]
    fn archived_candidate_names_are_accepted_by_their_own_validator() {
        assert!(is_candidate_name(&interrupted_candidate(
            2,
            1_700_000_000,
            0
        )));
        assert!(is_candidate_name(&interrupted_candidate(
            u64::MAX,
            u128::MAX,
            999
        )));
        assert!(!is_candidate_name("interrupted-r1-2.json"));
        assert!(!is_candidate_name("../../goodtype.json"));
        assert!(!is_candidate_name("interrupted-rx-1-2.json"));
    }

    #[test]
    fn ink_layer_names_agree_between_id_and_path() {
        assert_eq!(ink_layer_id("page-002", 1), "page-002-ink-001");
        assert_eq!(ink_layer_path("page-002", 1), "ink/page-002-layer-001.json");
        assert_eq!(page_path(&page_id(2)), "pages/page-002.json");
    }
}
