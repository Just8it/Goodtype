use std::path::Path;

use goodtype_core::storage;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri_plugin_dialog::DialogExt;

use crate::{
    tinymist::Tinymist,
    workspace::{AllowedRoots, ensure_allowed},
};

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

fn validate(choice: &PresetChoice) -> Result<(), String> {
    let Some((_, source)) = source(choice)? else {
        return Ok(());
    };
    goodtype_typst::validate_preset_source(source)
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

pub(crate) fn install_default(
    root: &Path,
    choice: &PresetChoice,
    expected_source: Option<&str>,
) -> Result<Option<PresetSummary>, String> {
    validate(choice)?;
    let Some((name, source)) = source(choice)? else {
        return Ok(None);
    };
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
            storage::install_typst_style(root, &format!("default-saved-{digest}.typ"), previous)
                .map_err(|error| error.to_string())?;
        }
        storage::update_typst_style(root, "default.typ", source, previous)
            .map_err(|error| error.to_string())?;
    } else {
        storage::install_typst_style(root, "default.typ", source)
            .map_err(|error| error.to_string())?;
    }
    Ok(Some(PresetSummary {
        id: "default".into(),
        name: name.into(),
        description: "Notebook default".into(),
        import_path: Some("/styles/default.typ".into()),
        kind: "default",
    }))
}

#[tauri::command]
pub async fn list_typst_presets(
    roots: tauri::State<'_, AllowedRoots>,
    root: Option<String>,
) -> Result<Vec<PresetSummary>, String> {
    let mut summaries = BUILTINS
        .iter()
        .map(|preset| PresetSummary {
            id: preset.id.into(),
            name: preset.name.into(),
            description: preset.description.into(),
            import_path: None,
            kind: "builtin",
        })
        .collect::<Vec<_>>();
    let Some(root) = root else {
        return Ok(summaries);
    };
    let root = ensure_allowed(&roots, &root)?;
    let files = tauri::async_runtime::spawn_blocking(move || storage::list_typst_styles(&root))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
    for file in files {
        let filename = file.path.trim_start_matches("styles/");
        let text =
            String::from_utf8(file.bytes).map_err(|_| format!("{} is not UTF-8", file.path))?;
        let matched = BUILTINS.iter().find(|preset| preset.source == text);
        summaries.push(PresetSummary {
            id: file.path.clone(),
            name: matched
                .map(|preset| preset.name.to_owned())
                .or_else(|| template_name(&text))
                .unwrap_or_else(|| {
                    if filename == "default.typ" {
                        "Custom preset".into()
                    } else {
                        name_from_filename(filename)
                    }
                }),
            description: if filename == "default.typ" {
                "Notebook default".into()
            } else {
                "Notebook preset".into()
            },
            import_path: Some(format!("/{}", file.path.replace('\\', "/"))),
            kind: if filename == "default.typ" {
                "default"
            } else {
                "custom"
            },
        });
    }
    Ok(summaries)
}

#[tauri::command]
pub async fn pick_typst_preset(
    app: tauri::AppHandle,
    ui_language: Option<String>,
) -> Result<Option<PresetChoice>, String> {
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
    goodtype_typst::validate_preset_source(&source)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("preset.typ")
        .to_owned();
    Ok(Some(PresetChoice::Imported { name, source }))
}

#[tauri::command]
pub async fn validate_typst_preset(choice: PresetChoice) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || validate(&choice))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn set_default_typst_preset(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, crate::notebook::NotebookHistories>,
    tinymist: tauri::State<'_, Tinymist>,
    root: String,
    choice: PresetChoice,
    expected_source: Option<String>,
) -> Result<Option<PresetSummary>, String> {
    let result = crate::notebook::on_notebook(&roots, &histories, root, move |root, _| {
        install_default(root, &choice, expected_source.as_deref()).map_err(Into::into)
    })
    .await
    .map_err(|error| error.to_string())?;
    tinymist.reset();
    Ok(result)
}

#[tauri::command]
pub async fn install_page_typst_preset(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, crate::notebook::NotebookHistories>,
    tinymist: tauri::State<'_, Tinymist>,
    root: String,
    choice: PresetChoice,
) -> Result<Option<PresetSummary>, String> {
    let summary = crate::notebook::on_notebook(&roots, &histories, root, move |root, _| {
        validate(&choice)?;
        let Some((name, source_text)) = source(&choice)? else {
            return Ok(None);
        };
        let filename = page_filename(&choice, source_text)?;
        let path = storage::install_typst_style(root, &filename, source_text)
            .map_err(|error| error.to_string())?;
        Ok(Some(PresetSummary {
            id: path.clone(),
            name: name.into(),
            description: "Page override".into(),
            import_path: Some(format!("/{path}")),
            kind: "custom",
        }))
    })
    .await
    .map_err(|error| error.to_string())?;
    tinymist.reset();
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

/// A new, editable template in the notebook's `styles/` folder.
#[tauri::command]
pub async fn create_typst_preset(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, crate::notebook::NotebookHistories>,
    tinymist: tauri::State<'_, Tinymist>,
    root: String,
) -> Result<PresetSummary, String> {
    let summary = crate::notebook::on_notebook(&roots, &histories, root, move |root, _| {
        let existing = storage::list_typst_styles(root)
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|file| file.path.replace('\\', "/"))
            .collect::<Vec<_>>();
        let filename = next_template_filename(&existing);
        let name = name_from_filename(&filename);
        let source = format!("// Name: {name}\n{STARTER}");
        let path = storage::install_typst_style(root, &filename, &source)
            .map_err(|error| error.to_string())?;
        Ok(PresetSummary {
            id: path.clone(),
            name,
            description: "Notebook preset".into(),
            import_path: Some(format!("/{path}")),
            kind: "custom",
        })
    })
    .await
    .map_err(|error| error.to_string())?;
    tinymist.reset();
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
        let file = storage::list_typst_styles(&root)
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|file| file.path.replace('\\', "/") == format!("styles/{filename}"))
            .ok_or_else(|| format!("Template {filename} is not in this notebook"))?;
        String::from_utf8(file.bytes).map_err(|_| format!("{filename} is not UTF-8"))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Saves an edited template. Not validated as a complete preset: it is saved while being typed,
/// and a half-written template must not be refused and lost. Pages keep their last good render
/// until it compiles again.
#[tauri::command]
pub async fn save_typst_preset(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, crate::notebook::NotebookHistories>,
    root: String,
    path: String,
    source: String,
    expected_source: String,
) -> Result<(), String> {
    let filename = style_filename(&path)?;
    crate::notebook::on_notebook(&roots, &histories, root, move |root, _| {
        storage::update_typst_style(root, &filename, &source, &expected_source)
            .map(|_| ())
            .map_err(Into::into)
    })
    .await
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_implement_the_contract() {
        for preset in BUILTINS {
            goodtype_typst::validate_preset_source(preset.source).unwrap();
        }
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
            source: "#let not_a_preset = 1".into(),
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
