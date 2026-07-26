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
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_ide::{CompletionKind, IdeWorld};
use typst_kit::downloader::{Downloader, SystemDownloader};
use typst_kit::packages::{FsPackages, SystemPackages, UniversePackages};
use typst_layout::PagedDocument;
use typst_svg::SvgOptions;

use crate::{CompileResult, Diagnostic, DiagnosticSeverity};

/// Compile a single wrapped block source in-process and render its first page to SVG.
pub(crate) fn compile_block(
    root: &Path,
    generation: u64,
    wrapper: String,
    pad_pt: f64,
    allow_remote_packages: bool,
) -> CompileResult {
    let world = BlockWorld::for_main(
        "main.typ",
        wrapper,
        root.to_path_buf(),
        allow_remote_packages,
    );
    let compiled = typst::compile::<PagedDocument>(&world);

    let mut diagnostics: Vec<Diagnostic> = compiled
        .warnings
        .iter()
        .map(diagnostic_from_source)
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
            diagnostics.extend(errors.iter().map(diagnostic_from_source));
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
    let mut world = BlockWorld::for_main(
        main_name,
        main_source,
        root.to_path_buf(),
        allow_remote_packages,
    );
    for (name, bytes) in overlay {
        world.insert_binary(&name, bytes);
    }

    let compiled = typst::compile::<PagedDocument>(&world);
    let document = compiled.output.map_err(|errors| {
        errors
            .iter()
            .map(|error| error.message.as_str().to_owned())
            .collect::<Vec<_>>()
            .join("; ")
    })?;

    typst_pdf::pdf(&document, &typst_pdf::PdfOptions::default()).map_err(|errors| {
        errors
            .iter()
            .map(|error| error.message.as_str().to_owned())
            .collect::<Vec<_>>()
            .join("; ")
    })
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

fn diagnostic_from_source(diagnostic: &SourceDiagnostic) -> Diagnostic {
    Diagnostic {
        severity: match diagnostic.severity {
            Severity::Error => DiagnosticSeverity::Error,
            Severity::Warning => DiagnosticSeverity::Warning,
        },
        message: diagnostic.message.as_str().to_owned(),
    }
}

/// Lazily parsed embedded font set (New Computer Modern, etc.). Shared across compiles so the
/// fonts are only decoded once for the life of the process.
struct EmbeddedFonts {
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
}

fn embedded_fonts() -> &'static EmbeddedFonts {
    static FONTS: OnceLock<EmbeddedFonts> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut fonts = Vec::new();
        for data in typst_assets::fonts() {
            fonts.extend(Font::iter(Bytes::new(data)));
        }
        let book = FontBook::from_fonts(&fonts);
        EmbeddedFonts {
            book: LazyHash::new(book),
            fonts,
        }
    })
}

/// Identifies Goodtype to the package registry.
const PACKAGE_USER_AGENT: &str = concat!("goodtype/", env!("CARGO_PKG_VERSION"));

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

/// A Goodtype-owned Typst [`World`] scoped to one notebook root.
struct BlockWorld {
    library: LazyHash<Library>,
    root: PathBuf,
    main: FileId,
    /// Whether a package may be downloaded on a cache miss.
    allow_remote_packages: bool,
    /// Generated, in-memory files (the entry document and its helpers) keyed by file id.
    overlay: HashMap<FileId, Bytes>,
    /// Decoded source cache so repeated `source()` calls during layout stay cheap.
    sources: Mutex<HashMap<FileId, Source>>,
}

impl BlockWorld {
    fn for_main(
        main_name: &str,
        main_source: String,
        root: PathBuf,
        allow_remote_packages: bool,
    ) -> Self {
        let main = project_file_id(main_name);
        let mut overlay = HashMap::new();
        overlay.insert(main, Bytes::from_string(main_source));
        Self {
            library: LazyHash::new(Library::default()),
            root,
            main,
            allow_remote_packages,
            overlay,
            sources: Mutex::new(HashMap::new()),
        }
    }

    fn insert_binary(&mut self, name: &str, bytes: Vec<u8>) {
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
                let packages = if self.allow_remote_packages {
                    downloading_packages()
                } else {
                    local_packages()
                };
                let root = packages.obtain(spec).map_err(FileError::Package)?;
                // FsRoot::resolve denies a virtual path that would escape the package root.
                root.resolve(id.vpath())
            }
            VirtualRoot::Project => {
                let path = id
                    .vpath()
                    .realize(&self.root)
                    .map_err(|_| FileError::AccessDenied)?;
                let canonical = path
                    .canonicalize()
                    .map_err(|error| FileError::from_io(error, &path))?;
                if !canonical.starts_with(&self.root) {
                    return Err(FileError::AccessDenied);
                }
                Ok(canonical)
            }
        }
    }
}

impl World for BlockWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &embedded_fonts().book
    }

    fn main(&self) -> FileId {
        self.main
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if let Some(cached) = self.sources.lock().unwrap().get(&id).cloned() {
            return Ok(cached);
        }
        let text = if let Some(bytes) = self.overlay.get(&id) {
            std::str::from_utf8(bytes)
                .map_err(|_| FileError::InvalidUtf8)?
                .to_owned()
        } else {
            let path = self.on_disk(id)?;
            std::fs::read_to_string(&path).map_err(|error| FileError::from_io(error, &path))?
        };
        let source = Source::new(id, text);
        self.sources.lock().unwrap().insert(id, source.clone());
        Ok(source)
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if let Some(bytes) = self.overlay.get(&id) {
            return Ok(bytes.clone());
        }
        let path = self.on_disk(id)?;
        std::fs::read(&path)
            .map(Bytes::new)
            .map_err(|error| FileError::from_io(error, &path))
    }

    fn font(&self, index: usize) -> Option<Font> {
        embedded_fonts().fonts.get(index).cloned()
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
