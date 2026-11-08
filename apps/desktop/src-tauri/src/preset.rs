use std::path::Path;

use goodtype_core::storage;
use goodtype_typst::{Diagnostic, PresetCheck, page_source::PresetForm};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::Emitter;
use tauri_plugin_dialog::DialogExt;

use crate::{
    settings::RemotePackages,
    tinymist::Tinymist,
    workspace::{AllowedRoots, ensure_allowed},
};

/// Emitted to the main window after every successful Rust write of a notebook preset file.
pub const PRESET_CHANGED_EVENT: &str = "presets://changed";

/// One preset file that changed on disk. `root` is the notebook root exactly as the frontend
/// named it, `path` the import path pages use (`/styles/x.typ`), and `revision` a digest of the
/// new bytes. The payload carries no source text.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetChange {
    pub root: String,
    pub path: String,
    pub revision: String,
}

pub(crate) fn preset_change(root: &str, relative: &str, bytes: &[u8]) -> PresetChange {
    let digest = format!("{:x}", Sha256::digest(bytes));
    PresetChange {
        root: root.into(),
        path: format!("/{}", relative.trim_start_matches('/').replace('\\', "/")),
        revision: digest[..16].into(),
    }
}

pub(crate) fn notify(app: &tauri::AppHandle, changes: &[PresetChange]) {
    for change in changes {
        let _ = app.emit_to("main", PRESET_CHANGED_EVENT, change);
    }
}

const CLEAN_NOTES: &str = r##"#let preset(body, rhythm: 16pt) = {
  let step = if rhythm == none { 1em } else { rhythm }
  set par(justify: false)
  show heading.where(level: 1): set block(above: step, below: step / 2)
  show heading.where(level: 1): set text(size: 1.45em, weight: "bold", fill: rgb("#285a92"))
  show heading.where(level: 2): set block(above: step, below: step / 2)
  show heading.where(level: 2): set text(size: 1.2em, weight: "bold")
  body
}
"##;

const COMPACT_STEM: &str = r##"#let preset(body, rhythm: 16pt) = {
  let step = if rhythm == none { 0.8em } else { rhythm }
  set par(justify: false, spacing: step / 2)
  show heading: set block(above: step / 2, below: step / 2)
  show heading.where(level: 1): set text(size: 1.3em, weight: "bold", fill: rgb("#285a92"))
  show heading.where(level: 2): set text(size: 1.12em, weight: "bold")
  show math.equation.where(block: true): set block(above: step / 2, below: step / 2)
  body
}
"##;

const FORMAL_REPORT: &str = r##"#let preset(body, rhythm: 16pt) = {
  let step = if rhythm == none { 1em } else { rhythm }
  set par(justify: true, first-line-indent: 1.2em)
  show heading.where(level: 1): set block(above: step * 2, below: step)
  show heading.where(level: 1): set text(size: 1.55em, weight: "bold")
  show heading.where(level: 2): set block(above: step, below: step / 2)
  show heading.where(level: 2): set text(size: 1.25em, weight: "bold")
  show heading.where(level: 3): set text(size: 1.05em, weight: "bold", style: "italic")
  body
}
"##;

struct Builtin {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    source: &'static str,
}

const BUILTINS: &[Builtin] = &[
    Builtin {
        id: "clean-notes",
        name: "Clean Notes",
        description: "Open headings and relaxed notes",
        source: CLEAN_NOTES,
    },
    Builtin {
        id: "compact-stem",
        name: "Compact STEM",
        description: "Tighter rhythm for equations and derivations",
        source: COMPACT_STEM,
    },
    Builtin {
        id: "formal-report",
        name: "Formal Report",
        description: "Structured headings and justified prose",
        source: FORMAL_REPORT,
    },
];

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PresetChoice {
    #[default]
    None,
    Builtin {
        id: String,
    },
    Imported {
        name: String,
        source: String,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub import_path: Option<String>,
    pub kind: &'static str,
    /// Function or preamble, when the preset could be checked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub form: Option<PresetForm>,
    /// Why this preset cannot be used. A listing keeps the entry rather than failing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<PresetProblem>,
    /// Everything checking it against the representative sample reported, warnings included.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

/// `code` is stable for the UI to localize; `detail` is the full producer text, shown on demand.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetProblem {
    pub code: &'static str,
    pub detail: String,
}

impl PresetSummary {
    fn new(
        id: String,
        name: String,
        description: &str,
        import_path: Option<String>,
        kind: &'static str,
    ) -> Self {
        Self {
            id,
            name,
            description: description.into(),
            import_path,
            kind,
            form: None,
            problem: None,
            diagnostics: Vec::new(),
        }
    }
}

fn builtin(id: &str) -> Result<&'static Builtin, String> {
    BUILTINS
        .iter()
        .find(|preset| preset.id == id)
        .ok_or_else(|| format!("Unknown Typst preset: {id}"))
}

fn source(choice: &PresetChoice) -> Result<Option<(&str, &str)>, String> {
    match choice {
        PresetChoice::None => Ok(None),
        PresetChoice::Builtin { id } => {
            let preset = builtin(id)?;
            Ok(Some((preset.name, preset.source)))
        }
        PresetChoice::Imported { name, source } => Ok(Some((name, source))),
    }
}

/// Check a chosen preset against the representative sample under Rust's package policy.
/// Errors refuse it; warnings are returned with the check.
fn validate(
    choice: &PresetChoice,
    allow_remote_packages: bool,
) -> Result<Option<PresetCheck>, String> {
    let Some((_, source)) = source(choice)? else {
        return Ok(None);
    };
    let check = goodtype_typst::check_preset_candidate(source, allow_remote_packages)?;
    match check.error() {
        Some(error) => Err(error),
        None => Ok(Some(check)),
    }
}

/// Recheck dependencies as well as the preset: an imported file may have changed.
fn check_listing_preset(root: &Path, filename: &str, allow_remote_packages: bool) -> PresetCheck {
    let mut check = goodtype_typst::check_notebook_preset(root, filename, allow_remote_packages)
        .unwrap_or_else(|error| PresetCheck {
            form: None,
            sample_revision: goodtype_typst::PRESET_SAMPLE_REVISION,
            rendered: false,
            width_pt: None,
            height_pt: None,
            diagnostics: vec![Diagnostic::error(error)],
            svg: None,
        });
    check.svg = None;
    check
}

/// One listed notebook preset: readable entries are checked; unreadable ones keep a problem.
fn notebook_summary(
    root: &Path,
    entry: storage::TypstStyleEntry,
    allow_remote_packages: bool,
) -> PresetSummary {
    let filename = entry.path.trim_start_matches("styles/").to_owned();
    let is_default = filename == "default.typ";
    let fallback_name = if is_default {
        "Custom preset".to_owned()
    } else {
        name_from_filename(&filename)
    };
    let mut summary = PresetSummary::new(
        entry.path.clone(),
        fallback_name,
        if is_default {
            "Notebook default"
        } else {
            "Notebook preset"
        },
        Some(format!("/{}", entry.path.replace('\\', "/"))),
        if is_default { "default" } else { "custom" },
    );
    let bytes = match entry.bytes {
        Ok(bytes) => bytes,
        Err(error) => {
            let code = match error {
                storage::StorageError::InvalidNotebook(_) => "too_large",
                _ => "unreadable",
            };
            summary.problem = Some(PresetProblem {
                code,
                detail: error.to_string(),
            });
            return summary;
        }
    };
    let Ok(text) = std::str::from_utf8(&bytes) else {
        summary.problem = Some(PresetProblem {
            code: "not_utf8",
            detail: format!("{} is not UTF-8 text", entry.path),
        });
        return summary;
    };
    if let Some(name) = BUILTINS
        .iter()
        .find(|preset| preset.source == text)
        .map(|preset| preset.name.to_owned())
        .or_else(|| template_name(text))
    {
        summary.name = name;
    }
    let check = check_listing_preset(root, &filename, allow_remote_packages);
    summary.form = check.form;
    if let Some(error) = check.error() {
        summary.problem = Some(PresetProblem {
            code: "invalid",
            detail: error,
        });
    }
    summary.diagnostics = check.diagnostics;
    summary
}

fn safe_stem(name: &str) -> String {
    let name = Path::new(name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(name);
    goodtype_core::paths::safe_stem(name, 48, "preset").to_ascii_lowercase()
}

fn page_filename(choice: &PresetChoice, source_text: &str) -> Result<String, String> {
    if let PresetChoice::Builtin { id } = choice {
        return Ok(format!("{}.typ", builtin(id)?.id));
    }
    let name = source(choice)?.map(|(name, _)| name).unwrap_or("preset");
    let digest = format!("{:x}", Sha256::digest(source_text.as_bytes()));
    Ok(format!("{}-{}.typ", safe_stem(name), &digest[..8]))
}

/// Notebook creation installs the chosen default. Its candidate was already checked by the
/// picker under the live package policy, which also cached any package it needs, so this
/// re-check stays offline.
pub(crate) fn install_default(
    root: &Path,
    choice: &PresetChoice,
    expected_source: Option<&str>,
) -> Result<Option<PresetSummary>, String> {
    install_default_with(root, "", choice, expected_source, false).map(|(summary, _)| summary)
}

fn install_default_with(
    root: &Path,
    root_name: &str,
    choice: &PresetChoice,
    expected_source: Option<&str>,
    allow_remote_packages: bool,
) -> Result<(Option<PresetSummary>, Vec<PresetChange>), String> {
    validate(choice, allow_remote_packages)?;
    let Some((name, source)) = source(choice)? else {
        return Ok((None, vec![]));
    };
    let mut changes = Vec::new();
    let current = storage::list_typst_styles(root)
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|file| file.path == "styles/default.typ");
    if current.as_ref().map(|file| file.bytes.as_slice()) != expected_source.map(str::as_bytes) {
        return Err(storage::StorageError::ExternalChange("styles/default.typ".into()).to_string());
    }
    if let Some(previous) = expected_source {
        if previous != source {
            let digest = format!("{:x}", Sha256::digest(previous.as_bytes()));
            let backup = storage::install_typst_style(
                root,
                &format!("default-saved-{digest}.typ"),
                previous,
            )
            .map_err(|error| error.to_string())?;
            changes.push(preset_change(root_name, &backup, previous.as_bytes()));
        }
        storage::update_typst_style(root, "default.typ", source, previous)
            .map_err(|error| error.to_string())?;
    } else {
        storage::install_typst_style(root, "default.typ", source)
            .map_err(|error| error.to_string())?;
    }
    changes.push(preset_change(
        root_name,
        "styles/default.typ",
        source.as_bytes(),
    ));
    Ok((
        Some(PresetSummary::new(
            "default".into(),
            name.into(),
            "Notebook default",
            Some("/styles/default.typ".into()),
            "default",
        )),
        changes,
    ))
}

/// Built-ins, then every notebook preset. Non-Typst files in `styles/` are skipped; a preset
/// that cannot be read or does not compile stays listed with its problem, so one bad file never
/// hides the others.
fn list_presets(
    root: Option<&Path>,
    allow_remote_packages: bool,
) -> Result<Vec<PresetSummary>, String> {
    let mut summaries = BUILTINS
        .iter()
        .map(|preset| {
            let mut summary = PresetSummary::new(
                preset.id.into(),
                preset.name.into(),
                preset.description,
                None,
                "builtin",
            );
            summary.form = Some(PresetForm::Function);
            summary
        })
        .collect::<Vec<_>>();
    let Some(root) = root else {
        return Ok(summaries);
    };
    for entry in storage::list_typst_style_entries(root).map_err(|error| error.to_string())? {
        summaries.push(notebook_summary(root, entry, allow_remote_packages));
    }
    Ok(summaries)
}

#[tauri::command]
pub async fn list_typst_presets(
    roots: tauri::State<'_, AllowedRoots>,
    packages: tauri::State<'_, RemotePackages>,
    root: Option<String>,
) -> Result<Vec<PresetSummary>, String> {
    let allow_remote_packages = packages.allowed();
    let root = root.map(|root| ensure_allowed(&roots, &root)).transpose()?;
    tauri::async_runtime::spawn_blocking(move || {
        list_presets(root.as_deref(), allow_remote_packages)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn pick_typst_preset(
    app: tauri::AppHandle,
    packages: tauri::State<'_, RemotePackages>,
    ui_language: Option<String>,
) -> Result<Option<PresetChoice>, String> {
    let allow_remote_packages = packages.allowed();
    let Some(picked) = app
        .dialog()
        .file()
        .set_title(crate::settings::dialog_label(
            ui_language.as_deref(),
            "Import Typst preset",
            "Typst-Vorlage importieren",
        ))
        .add_filter(
            crate::settings::dialog_label(ui_language.as_deref(), "Typst preset", "Typst-Vorlage"),
            &["typ"],
        )
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(|error| error.to_string())?;
    let bytes = crate::store::read_bounded(&path, goodtype_typst::MAX_SOURCE_BYTES)
        .map_err(|error| format!("Preset: {error}"))?;
    let source = String::from_utf8(bytes).map_err(|_| "Preset must be UTF-8".to_owned())?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("preset.typ")
        .to_owned();
    let choice = PresetChoice::Imported { name, source };
    // Checked like every ordinary import: the representative sample under the package policy.
    let checked = choice.clone();
    tauri::async_runtime::spawn_blocking(move || validate(&checked, allow_remote_packages))
        .await
        .map_err(|error| error.to_string())??;
    Ok(Some(choice))
}

/// The structured check of a chosen preset: form, warnings and sample size, or the error.
#[tauri::command]
pub async fn validate_typst_preset(
    packages: tauri::State<'_, RemotePackages>,
    choice: PresetChoice,
) -> Result<Option<PresetCheck>, String> {
    let allow_remote_packages = packages.allowed();
    tauri::async_runtime::spawn_blocking(move || validate(&choice, allow_remote_packages))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn set_default_typst_preset(
    app: tauri::AppHandle,
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, crate::notebook::NotebookHistories>,
    tinymist: tauri::State<'_, Tinymist>,
    packages: tauri::State<'_, RemotePackages>,
    root: String,
    choice: PresetChoice,
    expected_source: Option<String>,
) -> Result<Option<PresetSummary>, String> {
    let allow_remote_packages = packages.allowed();
    let root_name = root.clone();
    let (result, changes) =
        crate::notebook::on_notebook(&roots, &histories, root, move |root, _| {
            install_default_with(
                root,
                &root_name,
                &choice,
                expected_source.as_deref(),
                allow_remote_packages,
            )
            .map_err(Into::into)
        })
        .await
        .map_err(|error| error.to_string())?;
    tinymist.reset();
    notify(&app, &changes);
    Ok(result)
}

fn install_page_preset(
    root: &Path,
    root_name: &str,
    choice: &PresetChoice,
    allow_remote_packages: bool,
) -> Result<(Option<PresetSummary>, Vec<PresetChange>), String> {
    validate(choice, allow_remote_packages)?;
    let Some((name, source_text)) = source(choice)? else {
        return Ok((None, vec![]));
    };
    let filename = page_filename(choice, source_text)?;
    let path = storage::install_typst_style(root, &filename, source_text)
        .map_err(|error| error.to_string())?;
    let change = preset_change(root_name, &path, source_text.as_bytes());
    Ok((
        Some(PresetSummary::new(
            path.clone(),
            name.into(),
            "Page override",
            Some(format!("/{path}")),
            "custom",
        )),
        vec![change],
    ))
}

#[tauri::command]
pub async fn install_page_typst_preset(
    app: tauri::AppHandle,
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, crate::notebook::NotebookHistories>,
    tinymist: tauri::State<'_, Tinymist>,
    packages: tauri::State<'_, RemotePackages>,
    root: String,
    choice: PresetChoice,
) -> Result<Option<PresetSummary>, String> {
    let allow_remote_packages = packages.allowed();
    let root_name = root.clone();
    let (summary, changes) =
        crate::notebook::on_notebook(&roots, &histories, root, move |root, _| {
            install_page_preset(root, &root_name, &choice, allow_remote_packages)
                .map_err(Into::into)
        })
        .await
        .map_err(|error| error.to_string())?;
    tinymist.reset();
    notify(&app, &changes);
    Ok(summary)
}

/// What a new template starts as. Written like the top of an ordinary Typst document, with no
/// enclosing `{ }`: pasting a header into the middle of a `preset` function's body put every
/// `#import` and `#let` into code mode, where `#` is an error. Anything pasted or typed anywhere
/// in this starter stays valid. Templates defining `preset` are still accepted.
const STARTER: &str = r##"// Page template, written like the top of a Typst document: #import packages,
// #let your own commands, and #set / #show rules. It applies to every page using it.
// Any installed font works, e.g. #set text(font: "Arial").

#set text(font: "New Computer Modern")
#set par(justify: false)
#show heading.where(level: 1): set text(weight: "bold")
"##;

/// The longest name a template's `// Name:` line may give it.
const MAX_TEMPLATE_NAME_CHARS: usize = 60;

/// A template names itself with a `// Name: …` comment among its first lines. The name lives in
/// the file rather than in its filename, so renaming never touches the pages importing it, and
/// the name travels with the file when it is copied to another notebook.
fn template_name(source: &str) -> Option<String> {
    source.lines().take(5).find_map(|line| {
        let comment = line.trim().strip_prefix("//")?.trim();
        let (label, rest) = comment.split_once(':')?;
        if !label.trim().eq_ignore_ascii_case("name") {
            return None;
        }
        let name = rest.trim();
        (!name.is_empty()).then(|| name.chars().take(MAX_TEMPLATE_NAME_CHARS).collect())
    })
}

/// `template-2.typ` reads as "Template 2" where a file carries no `// Name:` line.
fn name_from_filename(filename: &str) -> String {
    let words = filename.trim_end_matches(".typ").replace('-', " ");
    let mut characters = words.chars();
    characters
        .next()
        .map(|first| first.to_uppercase().chain(characters).collect())
        .unwrap_or_default()
}

/// `/styles/name.typ` (the import path pages use) to the bare filename storage accepts.
fn style_filename(path: &str) -> Result<String, String> {
    path.trim_start_matches('/')
        .strip_prefix("styles/")
        .filter(|name| !name.contains('/') && !name.contains('\\'))
        .map(str::to_owned)
        .ok_or_else(|| format!("Not a notebook template: {path}"))
}

/// The first `template-N.typ` not already in the notebook.
fn next_template_filename(existing: &[String]) -> String {
    (1..)
        .map(|number| format!("template-{number}.typ"))
        .find(|name| {
            !existing
                .iter()
                .any(|path| path.ends_with(&format!("styles/{name}")))
        })
        .expect("an unused template number exists")
}

fn create_preset(root: &Path, root_name: &str) -> Result<(PresetSummary, PresetChange), String> {
    // Unreadable entries still occupy their names.
    let existing = storage::list_typst_style_entries(root)
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|entry| entry.path.replace('\\', "/"))
        .collect::<Vec<_>>();
    let filename = next_template_filename(&existing);
    let name = name_from_filename(&filename);
    let source = format!("// Name: {name}\n{STARTER}");
    let path = storage::install_typst_style(root, &filename, &source)
        .map_err(|error| error.to_string())?;
    let change = preset_change(root_name, &path, source.as_bytes());
    Ok((
        PresetSummary::new(
            path.clone(),
            name,
            "Notebook preset",
            Some(format!("/{path}")),
            "custom",
        ),
        change,
    ))
}

/// A new, editable template in the notebook's `styles/` folder.
#[tauri::command]
pub async fn create_typst_preset(
    app: tauri::AppHandle,
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, crate::notebook::NotebookHistories>,
    tinymist: tauri::State<'_, Tinymist>,
    root: String,
) -> Result<PresetSummary, String> {
    let root_name = root.clone();
    let (summary, change) =
        crate::notebook::on_notebook(&roots, &histories, root, move |root, _| {
            create_preset(root, &root_name).map_err(Into::into)
        })
        .await
        .map_err(|error| error.to_string())?;
    tinymist.reset();
    notify(&app, &[change]);
    Ok(summary)
}

/// The text of a template, for the editor.
#[tauri::command]
pub async fn read_typst_preset(
    roots: tauri::State<'_, AllowedRoots>,
    root: String,
    path: String,
) -> Result<String, String> {
    let root = ensure_allowed(&roots, &root)?;
    let filename = style_filename(&path)?;
    tauri::async_runtime::spawn_blocking(move || {
        let entry = storage::list_typst_style_entries(&root)
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|entry| entry.path.replace('\\', "/") == format!("styles/{filename}"))
            .ok_or_else(|| format!("Template {filename} is not in this notebook"))?;
        let bytes = entry.bytes.map_err(|error| error.to_string())?;
        String::from_utf8(bytes).map_err(|_| format!("{filename} is not UTF-8"))
    })
    .await
    .map_err(|error| error.to_string())?
}

fn save_preset(
    root: &Path,
    root_name: &str,
    filename: &str,
    source: &str,
    expected_source: &str,
) -> Result<PresetChange, storage::StorageError> {
    let path = storage::update_typst_style(root, filename, source, expected_source)?;
    Ok(preset_change(root_name, &path, source.as_bytes()))
}

/// Saves an edited template. Not validated as a complete preset: it is saved while being typed,
/// and a half-written template must not be refused and lost. Pages keep their last good render
/// until it compiles again; the listing that follows the change notice diagnoses the draft.
#[tauri::command]
pub async fn save_typst_preset(
    app: tauri::AppHandle,
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, crate::notebook::NotebookHistories>,
    root: String,
    path: String,
    source: String,
    expected_source: String,
) -> Result<(), String> {
    let filename = style_filename(&path)?;
    let root_name = root.clone();
    let change = crate::notebook::on_notebook(&roots, &histories, root, move |root, _| {
        save_preset(root, &root_name, &filename, &source, &expected_source).map_err(Into::into)
    })
    .await
    .map_err(|error| error.to_string())?;
    notify(&app, &[change]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_implement_the_contract() {
        for preset in BUILTINS {
            let check = goodtype_typst::check_preset_candidate(preset.source, false).unwrap();
            assert!(check.rendered, "{}: {:?}", preset.id, check.diagnostics);
            assert_eq!(check.form, Some(PresetForm::Function));
            assert!(check.diagnostics.is_empty(), "{:?}", check.diagnostics);
        }
    }

    /// P3: the editor's starter is a preamble and validates as one, as does every new template.
    #[test]
    fn the_editor_starter_validates_as_a_preamble() {
        let root = tempfile::tempdir().unwrap();
        let (summary, _) = create_preset(root.path(), "book").unwrap();
        let listed = list_presets(Some(root.path()), false).unwrap();
        let created = listed.iter().find(|entry| entry.id == summary.id).unwrap();
        assert_eq!(created.form, Some(PresetForm::Preamble));
        assert_eq!(created.problem, None, "{:?}", created.diagnostics);
        let choice = PresetChoice::Imported {
            name: "starter.typ".into(),
            source: STARTER.into(),
        };
        assert_eq!(
            validate(&choice, false).unwrap().unwrap().form,
            Some(PresetForm::Preamble)
        );
    }

    /// P2: every successful Rust preset write yields a narrow change for its file; a refused
    /// write yields none.
    #[test]
    fn every_preset_write_reports_its_root_path_and_revision() {
        let root = tempfile::tempdir().unwrap();
        let (_, created) = create_preset(root.path(), "C:/Book").unwrap();
        assert_eq!(created.root, "C:/Book");
        assert_eq!(created.path, "/styles/template-1.typ");
        let original = std::fs::read_to_string(root.path().join("styles/template-1.typ")).unwrap();
        let saved = save_preset(
            root.path(),
            "C:/Book",
            "template-1.typ",
            "#set par(justify: true)",
            &original,
        )
        .unwrap();
        assert_eq!(saved.path, created.path);
        assert_ne!(saved.revision, created.revision);
        assert!(save_preset(root.path(), "C:/Book", "template-1.typ", "x", "stale").is_err());
        let (_, installed) = install_page_preset(
            root.path(),
            "C:/Book",
            &PresetChoice::Builtin {
                id: "clean-notes".into(),
            },
            false,
        )
        .unwrap();
        assert_eq!(installed[0].path, "/styles/clean-notes.typ");
        let builtin = PresetChoice::Builtin {
            id: "compact-stem".into(),
        };
        let (_, first) =
            install_default_with(root.path(), "C:/Book", &builtin, None, false).unwrap();
        assert_eq!(
            first.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(),
            ["/styles/default.typ"]
        );
        let other = PresetChoice::Builtin {
            id: "formal-report".into(),
        };
        let (_, replaced) =
            install_default_with(root.path(), "C:/Book", &other, Some(COMPACT_STEM), false)
                .unwrap();
        assert_eq!(
            replaced.len(),
            2,
            "the backup and the new default both changed"
        );
        assert!(replaced[0].path.starts_with("/styles/default-saved-"));
        assert!(
            install_default_with(root.path(), "C:/Book", &other, Some("stale"), false).is_err()
        );
        let json = serde_json::to_value(&saved).unwrap();
        assert_eq!(
            json.as_object().unwrap().len(),
            3,
            "no source text in the notice"
        );
    }

    /// P7/P8: listing keeps valid presets visible, skips unrelated files, reports unreadable,
    /// non-UTF-8 and failing presets per entry, and keeps a font warning on a usable preset.
    #[test]
    fn listing_reports_bad_presets_individually_and_keeps_warnings() {
        let root = tempfile::tempdir().unwrap();
        storage::write_typst_style(
            root.path(),
            "good.typ",
            "// Name: Good\n#set par(justify: true)",
        )
        .unwrap();
        storage::write_typst_style(
            root.path(),
            "font.typ",
            "// Name: Font\n#set text(font: \"Goodtype Missing Font\")",
        )
        .unwrap();
        storage::write_typst_style(
            root.path(),
            "broken.typ",
            "// Name: Broken\n#set text(size: ",
        )
        .unwrap();
        let styles = root.path().join("styles");
        std::fs::write(styles.join("notes.txt"), "not a preset").unwrap();
        std::fs::write(styles.join("latin1.typ"), [0xe4u8]).unwrap();
        let listed = list_presets(Some(root.path()), false).unwrap();
        let find = |id: &str| listed.iter().find(|entry| entry.id == id).unwrap();
        assert!(listed.iter().all(|entry| !entry.id.ends_with(".txt")));
        assert_eq!(
            listed.iter().filter(|e| e.kind == "builtin").count(),
            BUILTINS.len()
        );
        let good = find("styles/good.typ");
        assert_eq!((good.name.as_str(), &good.problem), ("Good", &None));
        assert!(good.diagnostics.is_empty());
        let font = find("styles/font.typ");
        assert_eq!(font.problem, None);
        assert!(
            font.diagnostics
                .iter()
                .any(|d| d.severity == goodtype_typst::DiagnosticSeverity::Warning)
        );
        assert_eq!(
            find("styles/broken.typ").problem.as_ref().unwrap().code,
            "invalid"
        );
        assert_eq!(
            find("styles/latin1.typ").problem.as_ref().unwrap().code,
            "not_utf8"
        );
        let json = serde_json::to_value(font).unwrap();
        assert_eq!(json["diagnostics"][0]["severity"], "warning");
        assert_eq!(json["form"], "preamble");
    }

    #[test]
    fn listing_rechecks_imported_dependencies_without_preset_edits() {
        let root = tempfile::tempdir().unwrap();
        storage::write_typst_style(
            root.path(),
            "base.typ",
            "#let preset(body, rhythm: 16pt) = body",
        )
        .unwrap();
        storage::write_typst_style(root.path(), "dependent.typ", "#import \"base.typ\": preset")
            .unwrap();
        let problem = || {
            list_presets(Some(root.path()), false)
                .unwrap()
                .into_iter()
                .find(|entry| entry.id == "styles/dependent.typ")
                .unwrap()
                .problem
        };
        assert!(problem().is_none());
        storage::write_typst_style(root.path(), "base.typ", "#let preset(body) = ").unwrap();
        assert!(problem().is_some());
    }
    #[test]
    fn imported_names_are_safe_and_content_addressed() {
        let choice = PresetChoice::Imported {
            name: "../../My Style.typ".into(),
            source: CLEAN_NOTES.into(),
        };
        let filename = page_filename(&choice, CLEAN_NOTES).unwrap();
        assert!(filename.starts_with("my-style-"));
        assert!(filename.ends_with(".typ"));
        assert!(!filename.contains(".."));
    }

    #[test]
    fn invalid_import_writes_nothing() {
        let root = tempfile::tempdir().unwrap();
        let choice = PresetChoice::Imported {
            name: "broken.typ".into(),
            source: "#let preset(body) = ".into(),
        };
        assert!(install_default(root.path(), &choice, None).is_err());
        assert!(!root.path().join("styles/default.typ").exists());
    }

    #[test]
    fn installed_default_compiles_through_the_literal_project_import() {
        let root = tempfile::tempdir().unwrap();
        install_default(
            root.path(),
            &PresetChoice::Builtin {
                id: "compact-stem".into(),
            },
            None,
        )
        .unwrap();
        let result = goodtype_typst::compile_block(
            root.path(),
            &goodtype_typst::CompileRequest {
                source: "#let goodtype_rhythm = 16pt\n#import \"/styles/default.typ\": preset\n#show: preset.with(rhythm: goodtype_rhythm)\n= Energy\n\n$ E = m c^2 $".into(),
                width_pt: 420.0,
                generation: 1,
                allow_remote_packages: false,
            },
        )
        .unwrap();
        assert!(result.svg.is_some(), "{:?}", result.diagnostics);
    }

    #[test]
    fn templates_get_the_next_free_number_and_a_valid_starter() {
        let notebook = tempfile::tempdir().unwrap();
        let compiled = goodtype_typst::compile_block(
            notebook.path(),
            &goodtype_typst::CompileRequest {
                source: format!("{STARTER}\n= Heading\nText"),
                width_pt: 300.0,
                generation: 1,
                allow_remote_packages: false,
            },
        )
        .unwrap();
        assert!(compiled.svg.is_some(), "{:?}", compiled.diagnostics);
        assert!(
            !STARTER.contains("{"),
            "the starter must have no block to paste into"
        );
        assert_eq!(next_template_filename(&[]), "template-1.typ");
        let existing = [
            "styles/template-1.typ".to_owned(),
            "styles/default.typ".to_owned(),
        ];
        assert_eq!(next_template_filename(&existing), "template-2.typ");
    }

    #[test]
    fn templates_are_named_by_their_name_line_or_their_file() {
        assert_eq!(
            template_name("// Name: Messtechnik\n#set text(font: \"Arial\")").as_deref(),
            Some("Messtechnik")
        );
        assert_eq!(
            template_name("  //name:  Lab notes  ").as_deref(),
            Some("Lab notes")
        );
        assert_eq!(template_name("// Name:\n#set par(justify: true)"), None);
        assert_eq!(template_name("#let name = 1\n// renamed: x"), None);
        assert_eq!(name_from_filename("template-2.typ"), "Template 2");
    }

    #[test]
    fn only_paths_inside_styles_are_templates() {
        assert_eq!(
            style_filename("/styles/template-1.typ").unwrap(),
            "template-1.typ"
        );
        assert!(style_filename("/blocks/page.typ").is_err());
        assert!(style_filename("/styles/../goodtype.json").is_err());
        assert!(style_filename("/styles/sub/x.typ").is_err());
    }

    #[test]
    fn replacing_default_preserves_the_edited_source_and_rejects_stale_choices() {
        let root = tempfile::tempdir().unwrap();
        let edited = "// Name: My handwritten template\n#set text(size: 13pt)";
        storage::write_typst_style(root.path(), "default.typ", edited).unwrap();
        let choice = PresetChoice::Builtin {
            id: "clean-notes".into(),
        };
        assert!(install_default(root.path(), &choice, None).is_err());
        assert!(install_default(root.path(), &choice, Some("stale")).is_err());
        assert_eq!(storage::list_typst_styles(root.path()).unwrap().len(), 1);
        install_default(root.path(), &choice, Some(edited)).unwrap();
        let saved = storage::list_typst_styles(root.path()).unwrap();
        assert_eq!(saved.len(), 2);
        assert!(
            saved
                .iter()
                .any(|file| file.path.starts_with("styles/default-saved-")
                    && file.bytes == edited.as_bytes())
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("styles/default.typ")).unwrap(),
            CLEAN_NOTES
        );
        // Reselecting the same preset neither overwrites a backup nor adds another one.
        install_default(root.path(), &choice, Some(CLEAN_NOTES)).unwrap();
        assert_eq!(storage::list_typst_styles(root.path()).unwrap().len(), 2);
    }

    #[test]
    fn a_backup_collision_preserves_both_existing_files() {
        let root = tempfile::tempdir().unwrap();
        let previous = "#set text(size: 13pt)";
        storage::write_typst_style(root.path(), "default.typ", previous).unwrap();
        let digest = format!("{:x}", Sha256::digest(previous.as_bytes()));
        let backup = format!("default-saved-{digest}.typ");
        storage::write_typst_style(root.path(), &backup, "edited backup").unwrap();
        let choice = PresetChoice::Builtin {
            id: "clean-notes".into(),
        };
        assert!(install_default(root.path(), &choice, Some(previous)).is_err());
        assert_eq!(
            std::fs::read_to_string(root.path().join("styles/default.typ")).unwrap(),
            previous
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("styles").join(backup)).unwrap(),
            "edited backup"
        );
    }
}
