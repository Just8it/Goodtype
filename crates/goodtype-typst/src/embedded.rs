//! In-process Typst compiler.
//!
//! Goodtype owns the Typst [`World`]: fonts come only from the embedded set (no system
//! scan), file access is resolved relative to the notebook root and rejected outside it,
//! and the clock is fixed so compilation is deterministic. Generated sources (the wrapped
//! block, the combined export document, per-page ink SVGs) live in an in-memory overlay, so
//! nothing is written inside the canonical notebook tree.
//!
//! Typst Universe packages resolve offline-first and are sandboxed to their own package root,
//! never the notebook. A download happens only on a cache miss, only for the
//! official namespace, and only when the caller allows it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use typst::diag::{FileError, FileResult, Severity, SourceDiagnostic};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::Side;
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World, WorldExt};
use typst_ide::{CompletionKind, IdeWorld, Tooltip};
use typst_kit::downloader::{Downloader, SystemDownloader};
use typst_kit::fonts::FontStore;
use typst_kit::packages::{FsPackages, SystemPackages, UniversePackages};
use typst_layout::PagedDocument;
use typst_svg::SvgOptions;

use crate::{CompileResult, Diagnostic, DiagnosticSeverity};

const MAX_WORLD_FILE_BYTES: usize = 64 * 1024 * 1024;

/// Compile a single wrapped block source in-process and render its first page to SVG.
pub(crate) fn compile_block(
    root: &Path,
    generation: u64,
    wrapper: String,
    pad_pt: f64,
    packages: &'static SystemPackages,
    files: Option<&crate::GrantedFiles>,
) -> CompileResult {
    let mut world = BlockWorld::for_main("main.typ", wrapper, root.to_path_buf(), false)
        .with_packages(packages);
    world.grant(files);
    let compiled = typst::compile::<PagedDocument>(&world);

    let mut diagnostics: Vec<Diagnostic> = compiled
        .warnings
        .iter()
        .map(|d| diagnostic_from_source(&world, d))
        .collect();

    match compiled.output {
        Ok(document) => match document.pages().first() {
            Some(page) => {
                let svg = typst_svg::svg(page, &SvgOptions::default());
                // The frame includes the slack the wrapper asked for on all four sides. Reporting
                // the content size keeps the block's footprint equal to what the export prints;
                // `pad_pt` tells the caller how far the SVG bleeds past it. A source that sets
                // its own page margin can leave less than the slack, hence the floor.
                let content = |extent: f64| (extent - 2.0 * pad_pt).max(0.0);
                CompileResult {
                    generation,
                    svg: Some(svg),
                    width_pt: Some(content(page.frame.width().to_pt())),
                    height_pt: Some(content(page.frame.height().to_pt())),
                    pad_pt,
                    diagnostics,
                }
            }
            None => CompileResult {
                generation,
                svg: None,
                width_pt: None,
                height_pt: None,
                pad_pt,
                diagnostics,
            },
        },
        Err(errors) => {
            diagnostics.extend(errors.iter().map(|d| diagnostic_from_source(&world, d)));
            CompileResult {
                generation,
                svg: None,
                width_pt: None,
                height_pt: None,
                pad_pt,
                diagnostics,
            }
        }
    }
}

/// Compile a generated multi-page document (with its in-memory helper files) to PDF bytes.
///
/// `main_name` is the virtual name of the entry document; `overlay` carries every generated
/// helper file (blocks, ink SVGs) keyed by its virtual name. Referenced assets (images) are
/// read from disk, always contained within `root`.
pub(crate) fn export_pdf(
    root: &Path,
    main_name: &str,
    main_source: String,
    overlay: Vec<(String, Vec<u8>)>,
    allow_remote_packages: bool,
) -> Result<Vec<u8>, String> {
    let document = export_document(
        root,
        main_name,
        main_source,
        overlay,
        allow_remote_packages,
        None,
    )?;
    typst_pdf::pdf(&document, &typst_pdf::PdfOptions::default())
        .map_err(|errors| diagnostic_messages(&errors))
}

pub(crate) fn export_document(
    root: &Path,
    main_name: &str,
    main_source: String,
    overlay: Vec<(String, Vec<u8>)>,
    allow_remote_packages: bool,
    files: Option<&crate::GrantedFiles>,
) -> Result<PagedDocument, String> {
    let mut world = BlockWorld::for_main(
        main_name,
        main_source,
        root.to_path_buf(),
        allow_remote_packages,
    );
    world.grant(files);
    for (name, bytes) in overlay {
        world.insert_binary(&name, bytes);
    }

    let compiled = typst::compile::<PagedDocument>(&world);
    let document = compiled
        .output
        .map_err(|errors| diagnostic_messages(&errors))?;

    Ok(document)
}

pub(crate) fn diagnostic_messages(diagnostics: &[SourceDiagnostic]) -> String {
    diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect::<Vec<_>>()
        .join("; ")
}

/// Complete at a caret inside a block's source.
///
/// Analysis runs on the same root-scoped `World` as compilation, so completions can only ever
/// mention the notebook and already-resolved package roots. `cursor` is a byte offset into the
/// unwrapped block source — the page wrapper used for rendering is deliberately not applied, so
/// frontend offsets need no translation.
pub(crate) fn complete(
    root: &Path,
    source_text: String,
    cursor: usize,
    explicit: bool,
    allow_remote_packages: bool,
) -> Vec<crate::Completion> {
    let world = BlockWorld::for_main(
        "main.typ",
        source_text,
        root.to_path_buf(),
        allow_remote_packages,
    );
    let Ok(source) = world.source(world.main()) else {
        return Vec::new();
    };
    // A caret past the end, or inside a character, would panic the analyzer.
    if cursor > source.text().len() || !source.text().is_char_boundary(cursor) {
        return Vec::new();
    }

    let Some((offset, completions)) = typst_ide::autocomplete(
        &world,
        Option::<&PagedDocument>::None,
        &source,
        cursor,
        explicit,
    ) else {
        return Vec::new();
    };

    completions
        .into_iter()
        .map(|completion| crate::Completion {
            kind: match completion.kind {
                CompletionKind::Syntax => "syntax",
                CompletionKind::Func => "function",
                CompletionKind::Type => "type",
                CompletionKind::Param => "parameter",
                CompletionKind::Constant => "constant",
                CompletionKind::Path => "path",
                CompletionKind::Package => "package",
                CompletionKind::Label => "label",
                CompletionKind::Font => "font",
                CompletionKind::Symbol(_) => "symbol",
            },
            // The rendered glyph for a math symbol, so the picker can show `∑` next to `sum`.
            symbol: match &completion.kind {
                CompletionKind::Symbol(glyph) => Some(glyph.to_string()),
                _ => None,
            },
            label: completion.label.to_string(),
            apply: completion.apply.map(|apply| apply.to_string()),
            detail: completion.detail.map(|detail| detail.to_string()),
            offset,
        })
        .collect()
}

pub(crate) fn hover(
    root: &Path,
    source_text: String,
    cursor: usize,
    allow_remote_packages: bool,
) -> Option<crate::Hover> {
    let world = BlockWorld::for_main(
        "main.typ",
        source_text,
        root.to_path_buf(),
        allow_remote_packages,
    );
    let source = world.source(world.main()).ok()?;
    if cursor > source.text().len() || !source.text().is_char_boundary(cursor) {
        return None;
    }
    typst_ide::tooltip(
        &world,
        Option::<&PagedDocument>::None,
        &source,
        cursor,
        Side::After,
    )
    .map(|tooltip| match tooltip {
        Tooltip::Text(value) => crate::Hover {
            value: value.to_string(),
            code: false,
        },
        Tooltip::Code(value) => crate::Hover {
            value: value.to_string(),
            code: true,
        },
    })
}

fn diagnostic_from_source(world: &BlockWorld, diagnostic: &SourceDiagnostic) -> Diagnostic {
    let mut d = Diagnostic {
        severity: match diagnostic.severity {
            Severity::Error => DiagnosticSeverity::Error,
            Severity::Warning => DiagnosticSeverity::Warning,
        },
        message: diagnostic.message.as_str().to_owned(),
        line: None,
        column: None,
        end_line: None,
        end_column: None,
        snippet: None,
        file: None,
        range: None,
    };
    if let Some(id) = diagnostic.span.id() {
        if id != world.main {
            d.file = Some(match id.root() {
                VirtualRoot::Package(spec) => format!("{spec}{}", id.vpath().get_with_slash()),
                VirtualRoot::Project => id.vpath().get_with_slash().into(),
            });
        }
        if let Ok(source) = world.source(id) {
            d.range = world.range(diagnostic.span);
            if id != world.main
                && let Some(r) = &d.range
            {
                let pos = |n| {
                    let b = &source.text()[..n];
                    (
                        Some(b.chars().filter(|&c| c == '\n').count() + 1),
                        Some(b.rsplit('\n').next().unwrap_or("").chars().count() + 1),
                    )
                };
                (d.line, d.column) = pos(r.start);
                (d.end_line, d.end_column) = pos(r.end);
                d.snippet = Some(source.text()[r.clone()].chars().take(200).collect());
            }
        }
    }
    d
}

/// The fonts every compile can use, shared for the life of the process.
///
/// The bundled set (New Computer Modern, Libertinus, DejaVu Sans Mono) comes first, so a document
/// that names no font renders exactly as it always has. The fonts installed on this computer
/// follow, so `#set text(font: "Arial")` works: they are only indexed here, read-only, and a
/// font file is read from disk the first time a document actually uses it. Nothing about them
/// leaves the device, and a PDF export embeds what it uses, so it looks the same everywhere.
/// A notebook opened on a computer without that font falls back to the bundled set.
pub fn font_families() -> Vec<String> {
    fonts()
        .book()
        .families()
        .map(|(family, _)| family.to_owned())
        .collect()
}

fn fonts() -> &'static FontStore {
    static FONTS: OnceLock<FontStore> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut store = FontStore::new();
        store.extend(typst_kit::fonts::embedded());
        store.extend(typst_kit::fonts::system());
        store
    })
}

/// The Typst standard library, built once for the life of the process.
///
/// A `World` is constructed for every compile, completion and hover — several a second while
/// someone is typing — and `Library::default()` builds the whole standard scope each time. The
/// fonts beside it were already shared for exactly this reason; this is the same fix for the
/// other half of the world that never changes.
fn standard_library() -> &'static LazyHash<Library> {
    static LIBRARY: OnceLock<LazyHash<Library>> = OnceLock::new();
    LIBRARY.get_or_init(|| LazyHash::new(Library::default()))
}

/// Identifies Goodtype to the package registry.
const PACKAGE_USER_AGENT: &str = concat!("goodtype/", env!("CARGO_PKG_VERSION"));

/// The loader matching Rust's package policy: downloads on a cache miss only when allowed.
pub(crate) fn package_loader(allow_remote_packages: bool) -> &'static SystemPackages {
    if allow_remote_packages {
        downloading_packages()
    } else {
        local_packages()
    }
}

/// Whether the module at `path` binds `preset` to a function, decided by evaluating it rather
/// than by reading its text, so aliases (`#let preset = style`) and re-exports
/// (`#import "base.typ": preset`) count while `#let preset = 1` does not. `None` when the module
/// does not evaluate. Uses the same root-scoped, optionally granted-only world as compilation.
pub(crate) fn binds_preset_function(
    root: &Path,
    path: &str,
    packages: &'static SystemPackages,
    files: Option<&crate::GrantedFiles>,
) -> Option<bool> {
    // The probe stops during evaluation, before any layout, and reports through its own marker.
    // `path` is a managed-header path (`/styles/` plus safe filename characters), so quoting is
    // exact.
    let probe = format!(
        "#import \"{path}\" as goodtype_probe_module\n\
         #panic(\"{PROBE_MARKER}\" + repr(type(dictionary(goodtype_probe_module).at(\"preset\", default: none)) == function))"
    );
    let mut world = BlockWorld::for_main(
        "goodtype-preset-probe.typ",
        probe,
        root.canonicalize().ok()?,
        false,
    )
    .with_packages(packages);
    world.grant(files);
    let errors = typst::compile::<PagedDocument>(&world).output.err()?;
    errors.iter().find_map(|error| {
        let message = error.message.as_str();
        let found = &message[message.find(PROBE_MARKER)? + PROBE_MARKER.len()..];
        Some(found.starts_with("true"))
    })
}

const PROBE_MARKER: &str = "goodtype-preset-form:";

/// Package loader that may download from Typst Universe on a cache miss. Built once: it holds
/// the HTTPS client and the resolved data/cache directories.
fn downloading_packages() -> &'static SystemPackages {
    static PACKAGES: OnceLock<SystemPackages> = OnceLock::new();
    PACKAGES.get_or_init(|| SystemPackages::new(SystemDownloader::new(PACKAGE_USER_AGENT)))
}

/// The local package directories alone, used when the user has turned remote packages off:
/// anything already downloaded still resolves, but no request is ever made.
fn local_packages() -> &'static SystemPackages {
    static PACKAGES: OnceLock<SystemPackages> = OnceLock::new();
    PACKAGES.get_or_init(|| {
        // Passing no universe registry is what makes this loader incapable of downloading.
        SystemPackages::from_parts(
            FsPackages::system_data(),
            FsPackages::system_cache(),
            UniversePackages::new(OfflineDownloader),
        )
    })
}

/// A downloader that always fails, so a loader built with it can never reach the network even
/// if a future typst-kit change routes through it.
#[derive(Debug)]
struct OfflineDownloader;

impl Downloader for OfflineDownloader {
    fn stream(
        &self,
        _key: &dyn std::any::Any,
        _url: &str,
    ) -> std::io::Result<(Option<usize>, Box<dyn std::io::Read>)> {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "remote Typst packages are turned off",
        ))
    }
}

/// Exact package inputs consumed (and possibly fetched) by a reviewed helper preset.
#[derive(Clone, Default)]
pub struct PresetPackageBytes(pub std::collections::BTreeMap<String, (PathBuf, Vec<u8>)>);
type PresetPackageBinding = std::sync::Arc<Mutex<(bool, PresetPackageBytes)>>;
thread_local! {
    static PRESET_PACKAGES: std::cell::RefCell<Option<PresetPackageBinding>> = const { std::cell::RefCell::new(None) };
}
/// Capture once, or replay only the captured package bytes. Scoped to this synchronous check.
pub fn with_preset_packages<T>(
    replay: Option<&PresetPackageBytes>,
    run: impl FnOnce() -> Result<T, String>,
) -> Result<(T, PresetPackageBytes), String> {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            PRESET_PACKAGES.with(|s| *s.borrow_mut() = None);
        }
    }
    PRESET_PACKAGES.with(|s| {
        assert!(s.borrow().is_none());
        *s.borrow_mut() = Some(std::sync::Arc::new(Mutex::new((
            replay.is_some(),
            replay.cloned().unwrap_or_default(),
        ))));
    });
    let _reset = Reset;
    let value = run()?;
    let bytes = PRESET_PACKAGES.with(|s| s.borrow().as_ref().unwrap().lock().unwrap().1.clone());
    Ok((value, bytes))
}

/// A Goodtype-owned Typst [`World`] scoped to one notebook root.
pub(crate) struct BlockWorld {
    root: PathBuf,
    granted_only: bool,
    preset_packages: Option<PresetPackageBinding>,
    main: FileId,
    /// Resolves packages: local directories, plus a download on a cache miss when the policy
    /// allows it. Project-file grants do not override this caller-owned package policy.
    packages: &'static SystemPackages,
    /// Generated, in-memory files (the entry document and its helpers) keyed by file id.
    overlay: HashMap<FileId, Bytes>,
    /// Decoded source cache so repeated `source()` calls during layout stay cheap.
    sources: Mutex<HashMap<FileId, Source>>,
    /// Raw disk bytes are also requested repeatedly by Typst, especially for images and PDFs.
    files: Mutex<HashMap<FileId, Bytes>>,
}

impl BlockWorld {
    pub(crate) fn for_main(
        main_name: &str,
        main_source: String,
        root: PathBuf,
        allow_remote_packages: bool,
    ) -> Self {
        let main = project_file_id(main_name);
        let mut overlay = HashMap::new();
        overlay.insert(main, Bytes::from_string(main_source));
        Self {
            root,
            granted_only: false,
            preset_packages: PRESET_PACKAGES.with(|s| s.borrow().clone()),
            main,
            packages: package_loader(allow_remote_packages),
            overlay,
            sources: Mutex::new(HashMap::new()),
            files: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn with_packages(mut self, packages: &'static SystemPackages) -> Self {
        self.packages = packages;
        self
    }

    pub(crate) fn grant(&mut self, files: Option<&crate::GrantedFiles>) {
        self.granted_only = files.is_some();
        if let Some(files) = files {
            for (path, bytes) in files {
                if VirtualPath::new(path).is_ok() && project_file_id(path) != self.main {
                    self.insert_binary(path, bytes.clone());
                }
            }
        }
    }

    pub(crate) fn insert_binary(&mut self, name: &str, bytes: Vec<u8>) {
        self.overlay
            .insert(project_file_id(name), Bytes::new(bytes));
    }

    /// Resolve a file id to a real path on disk. A package resolves inside *its own* root and a
    /// notebook file inside the notebook root; neither can address the other, and a path that
    /// escapes its own root is rejected.
    fn on_disk(&self, id: FileId) -> FileResult<PathBuf> {
        match id.root() {
            VirtualRoot::Package(spec) => {
                // Offline-first: the local data and cache directories are consulted first, and a
                // download happens only on a miss — and only when the user allows it.
                // Project grants and package-download policy are independent. Offline callers
                // supply the offline loader; reviewed preset replay never reaches this fallback.
                let root = self.packages.obtain(spec).map_err(FileError::Package)?;
                // FsRoot::resolve denies a virtual path that would escape the package root.
                root.resolve(id.vpath())
            }
            VirtualRoot::Project if self.granted_only => Err(FileError::Other(Some(
                format!("not shared: {}", id.vpath().get_with_slash()).into(),
            ))),
            VirtualRoot::Project => {
                let path = id
                    .vpath()
                    .realize(&self.root)
                    .map_err(|_| FileError::AccessDenied)?;
                // The same containment check as every other notebook path, not a second copy.
                goodtype_core::paths::contained(&self.root, &path, "Typst file").map_err(|error| {
                    match error {
                        goodtype_core::paths::PathError::Io(error) => {
                            FileError::from_io(error, &path)
                        }
                        _ => FileError::AccessDenied,
                    }
                })
            }
        }
    }

    fn disk_bytes(&self, id: FileId, maximum: usize) -> FileResult<Bytes> {
        if self.granted_only
            && matches!(id.root(), VirtualRoot::Package(_))
            && let Some(binding) = &self.preset_packages
        {
            // The World retains the binding, including reads on Typst worker threads.
            let mut binding = binding.lock().unwrap();
            let (replay, captured) = &mut *binding;
            let key = format!("{id:?}");
            if let Some((_, bytes)) = captured.0.get(&key) {
                return if bytes.len() <= maximum {
                    Ok(Bytes::new(bytes.clone()))
                } else {
                    Err(FileError::AccessDenied)
                };
            }
            if *replay {
                return Err(FileError::AccessDenied);
            }
            return (|| {
                let path = self.on_disk(id)?;
                let bytes = goodtype_core::storage::files::read_bounded(&path, maximum)
                    .map_err(|e| FileError::from_io(e, &path))?;
                let total: usize = captured.0.values().map(|(_, b)| b.len()).sum();
                if captured.0.len() >= 256 || total + bytes.len() > 16 * 1024 * 1024 {
                    return Err(FileError::AccessDenied);
                }
                captured.0.insert(key, (path, bytes.clone()));
                Ok(Bytes::new(bytes))
            })();
        }
        if let Some(bytes) = self.files.lock().unwrap().get(&id).cloned() {
            if bytes.len() <= maximum {
                return Ok(bytes);
            }
            let path = self.on_disk(id)?;
            return Err(FileError::from_io(too_large(maximum), &path));
        }
        let path = self.on_disk(id)?;
        let bytes = goodtype_core::storage::files::read_bounded(&path, maximum)
            .map_err(|error| FileError::from_io(error, &path))?;
        let bytes = Bytes::new(bytes);
        self.files.lock().unwrap().insert(id, bytes.clone());
        Ok(bytes)
    }
}

fn too_large(maximum: usize) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("Typst input exceeds {maximum} bytes"),
    )
}

impl World for BlockWorld {
    fn library(&self) -> &LazyHash<Library> {
        standard_library()
    }

    fn book(&self) -> &LazyHash<FontBook> {
        fonts().book()
    }

    fn main(&self) -> FileId {
        self.main
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if let Some(cached) = self.sources.lock().unwrap().get(&id).cloned() {
            return Ok(cached);
        }
        let text = if let Some(bytes) = self.overlay.get(&id) {
            if bytes.len() > crate::MAX_SOURCE_BYTES {
                return Err(FileError::Other(Some("Typst source exceeds limit".into())));
            }
            std::str::from_utf8(bytes)
                .map_err(|_| FileError::InvalidUtf8)?
                .to_owned()
        } else {
            let bytes = self.disk_bytes(id, crate::MAX_SOURCE_BYTES)?;
            std::str::from_utf8(&bytes)
                .map_err(|_| FileError::InvalidUtf8)?
                .to_owned()
        };
        let source = Source::new(id, text);
        self.sources.lock().unwrap().insert(id, source.clone());
        Ok(source)
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if let Some(bytes) = self.overlay.get(&id) {
            if bytes.len() > MAX_WORLD_FILE_BYTES {
                return Err(FileError::Other(Some("Typst file exceeds limit".into())));
            }
            return Ok(bytes.clone());
        }
        self.disk_bytes(id, MAX_WORLD_FILE_BYTES)
    }

    fn font(&self, index: usize) -> Option<Font> {
        fonts().font(index)
    }

    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        // Fixed clock: compilation is deterministic and independent of wall-clock time.
        Datetime::from_ymd(1970, 1, 1)
    }
}

/// Completion and analysis run on the same `World` as compilation, so they inherit
/// its containment rather than reimplementing it. The optional `packages()`/`files()` hooks are
/// left at their defaults: suggesting packages would mean consulting a remote index, and
/// suggesting files would mean walking the notebook.
impl IdeWorld for BlockWorld {
    fn upcast(&self) -> &dyn World {
        self
    }
}

fn project_file_id(name: &str) -> FileId {
    let vpath = VirtualPath::new(name).expect("generated Typst file name is a valid virtual path");
    FileId::new(RootedPath::new(VirtualRoot::Project, vpath))
}

#[cfg(test)]
#[path = "computation_spike.rs"]
mod computation_spike;
