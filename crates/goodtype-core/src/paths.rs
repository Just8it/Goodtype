//! Keeping a path inside the directory it was supposed to stay in.
//!
//! This is the trust boundary. Every part of Goodtype that turns a string from outside — notebook
//! JSON, a frontend request, an export target — into a real path goes through here, because the
//! rule is subtle enough that a second copy of it is a second thing to get right.
//!
//! Rejecting `..`, absolute paths and odd characters is not sufficient on its own: a symlink
//! inside the directory can still point anywhere on the disk. So a candidate is always
//! *canonicalised*, which resolves every link, and only then required to sit under the
//! canonicalised root. [`contained`] is that check, and it is the part every caller shares.
//!
//! What differs between callers is which *names* are acceptable before containment is even
//! attempted, and that genuinely is policy rather than security:
//!
//! - [`validate_relative`] is the notebook-format rule. These paths are written into canonical
//!   JSON and have to mean the same thing on every machine the notebook is opened on, so it is
//!   strict: no backslash, no colon, nothing but ordinary components.
//! - [`validate_library_relative`] is for names the writer created in their own file manager.
//!   Goodtype did not choose them and refusing to open a folder is worse than the alternative, so
//!   it only rejects what cannot be interpreted at all.
//!
//! Both end at [`contained`]. The looser rule is not a weaker boundary — it is the same boundary
//! with a wider set of acceptable names in front of it.

use std::{
    fmt, fs, io,
    path::{Component, Path, PathBuf},
};

/// Why a path was refused.
///
/// Deliberately not carrying the resolved path in the escape case: the caller asked about a
/// relative name and telling them where it landed on this machine says more than they should
/// learn from a rejection.
#[derive(Debug)]
pub enum PathError {
    /// The name could not be interpreted as a contained relative path.
    Invalid(String),
    /// The name resolved outside the root, or to the wrong kind of entry.
    Escapes(String),
    Io(io::Error),
}

impl fmt::Display for PathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(value) => write!(formatter, "invalid path: {value}"),
            Self::Escapes(value) => write!(formatter, "path leaves its root: {value}"),
            Self::Io(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for PathError {}

impl From<io::Error> for PathError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Canonicalise `root` and confirm it is a directory.
pub fn canonical_root(root: &Path) -> Result<PathBuf, PathError> {
    let canonical = fs::canonicalize(root)?;
    if !canonical.is_dir() {
        return Err(PathError::Invalid(root.display().to_string()));
    }
    Ok(canonical)
}

/// The security primitive: resolve `candidate` through every symlink and require the result to
/// sit under `root`.
///
/// `root` must already be canonical — pass the result of [`canonical_root`]. Comparing against a
/// non-canonical root is the mistake this signature exists to make visible, because
/// `starts_with` on an unresolved root can pass for a path that is genuinely outside it.
pub fn contained(root: &Path, candidate: &Path, label: &str) -> Result<PathBuf, PathError> {
    let resolved = fs::canonicalize(candidate)?;
    if !resolved.starts_with(root) {
        return Err(PathError::Escapes(label.to_owned()));
    }
    Ok(resolved)
}

/// The notebook-format name rule, for paths stored in canonical JSON.
///
/// Strict because these names travel: a notebook written on one machine is opened on another, and
/// a name that is legal on only one of them is a notebook that does not survive the trip.
///
/// - Backslash, so a path written on Windows cannot mean something different on Unix.
/// - Colon, which on Windows names an NTFS alternate data stream. `foo.typ:hidden` is a single
///   ordinary-looking component, so the component check below does not catch it.
/// - Anything that is not a plain component, which covers `..`, roots, and drive prefixes.
pub fn validate_relative(value: &str) -> Result<&Path, PathError> {
    let path = Path::new(value);
    if value.is_empty()
        || value.contains(['\\', ':'])
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(PathError::Invalid(value.into()));
    }
    Ok(path)
}

/// The library name rule, for folders the writer made themselves.
///
/// Looser than [`validate_relative`] on purpose. These names come from `read_dir` of real
/// directories, so refusing one means a folder that lists but cannot be opened. A colon is legal
/// in a name on macOS and Linux, and banning it here would strand those folders for no gain:
/// containment is still enforced by [`contained`], and nothing from this path is written into a
/// notebook's canonical JSON.
///
/// An empty string is the library root itself and is accepted by the caller, not here.
pub fn validate_library_relative(value: &str) -> Result<&Path, PathError> {
    let path = Path::new(value);
    if value.is_empty()
        || value.contains('\\')
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(PathError::Invalid(value.into()));
    }
    Ok(path)
}

/// Whether `stem` names a DOS device Windows reserves, and therefore a name no directory or
/// file may take.
///
/// One predicate rather than two, because there were two: the library refused `com0` while the
/// store accepted it, and the store accepted a bare `com` the library refused. A library is
/// synced and shared, so both rules land on the same disks and disagreeing about them is how the
/// same name becomes creatable through one path and not the other.
///
/// Deliberately a superset of what Windows actually reserves: `COM0` and `LPT0` are not device
/// names, but refusing them costs a name nobody wants and removes the off-by-one entirely.
pub fn is_windows_reserved(stem: &str) -> bool {
    const BARE: [&str; 6] = ["con", "prn", "aux", "nul", "com", "lpt"];
    let lowered = stem.to_ascii_lowercase();
    if BARE.contains(&lowered.as_str()) {
        return true;
    }
    matches!(
        lowered
            .strip_prefix("com")
            .or_else(|| lowered.strip_prefix("lpt")),
        Some(rest) if rest.len() == 1 && rest.as_bytes()[0].is_ascii_digit()
    )
}

/// Resolve a notebook-relative path that must already exist as a file.
pub fn resolve_file(root: &Path, relative: &str) -> Result<PathBuf, PathError> {
    let path = validate_relative(relative)?;
    let resolved = contained(root, &root.join(path), relative)?;
    if !resolved.is_file() {
        return Err(PathError::Escapes(relative.to_owned()));
    }
    Ok(resolved)
}

/// Resolve a directory that must already exist, without assuming a name rule — the caller has
/// applied whichever one fits. Used for store-owned directories built from known constants.
pub fn resolve_dir(root: &Path, candidate: &Path, label: &str) -> Result<PathBuf, PathError> {
    let resolved = contained(root, candidate, label)?;
    if !resolved.is_dir() {
        return Err(PathError::Escapes(label.to_owned()));
    }
    Ok(resolved)
}

/// Create `candidate` if absent, then confirm it is a contained directory.
///
/// Creation and the check are one operation because doing them separately invites the check to be
/// skipped on the path where the directory already existed.
pub fn ensure_dir(root: &Path, candidate: &Path, label: &str) -> Result<PathBuf, PathError> {
    if !candidate.exists() {
        fs::create_dir_all(candidate)?;
    }
    resolve_dir(root, candidate, label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_what_cannot_stay_inside() {
        for value in [
            "",
            "..",
            "../escape.typ",
            "blocks/../../escape.typ",
            "/absolute.typ",
            "blocks\\windows.typ",
        ] {
            assert!(
                validate_relative(value).is_err(),
                "{value} should be refused"
            );
        }
        assert!(validate_relative("blocks/equation.typ").is_ok());
    }

    /// The rule the two name policies deliberately disagree on, asserted from both sides so a
    /// later edit cannot quietly collapse them into one.
    #[test]
    fn only_the_notebook_rule_bans_a_colon() {
        assert!(validate_relative("blocks/stream.typ:hidden").is_err());
        assert!(validate_library_relative("Semester 3: Thermo").is_ok());
        // Everything else they agree on.
        for value in ["..", "/absolute", "a\\b", ""] {
            assert!(validate_relative(value).is_err());
            assert!(validate_library_relative(value).is_err());
        }
    }

    /// The library and the store used to disagree about these, in both directions. Pinned here
    /// so the one predicate stays the only answer.
    #[test]
    fn reserved_device_names_are_refused_the_same_way_everywhere() {
        for reserved in [
            "con", "CON", "prn", "aux", "nul", "com", "lpt", "com0", "COM1", "com9", "lpt0", "LPT9",
        ] {
            assert!(
                is_windows_reserved(reserved),
                "`{reserved}` should be reserved"
            );
        }
        for ordinary in ["console", "com10", "communication", "prnt", "nula", "a", ""] {
            assert!(
                !is_windows_reserved(ordinary),
                "`{ordinary}` should be an ordinary name"
            );
        }
    }

    #[test]
    fn containment_resolves_links_before_deciding() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("root");
        let outside = temporary.path().join("outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.txt"), b"secret").unwrap();
        let root = canonical_root(&root).unwrap();

        fs::write(root.join("inside.txt"), b"inside").unwrap();
        assert!(resolve_file(&root, "inside.txt").is_ok());

        // Always exercised: a path that resolves outside the root is refused by `contained`
        // itself, independently of whatever name rule ran in front of it.
        assert!(
            contained(&root, &outside.join("secret.txt"), "secret.txt").is_err(),
            "a path outside the root must not be contained"
        );

        // Additionally exercised where the OS allows it: a link whose name is perfectly ordinary
        // and which genuinely sits inside the root, but points out of it. This is the case that
        // name checking alone cannot catch, and it needs Developer Mode or admin on Windows.
        #[cfg(windows)]
        let linked =
            std::os::windows::fs::symlink_file(outside.join("secret.txt"), root.join("link.txt"))
                .is_ok();
        #[cfg(unix)]
        let linked =
            std::os::unix::fs::symlink(outside.join("secret.txt"), root.join("link.txt")).is_ok();

        if linked {
            assert!(
                resolve_file(&root, "link.txt").is_err(),
                "a symlink out of the root must not resolve"
            );
        }
    }
}
