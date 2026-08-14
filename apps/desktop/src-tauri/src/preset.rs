use std::{fs, path::Path};

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
    let mut stem = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    while stem.contains("--") {
        stem = stem.replace("--", "-");
    }
    stem = stem.trim_matches('-').chars().take(48).collect();
    if stem.is_empty() {
        "preset".into()
    } else {
        stem
    }
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
) -> Result<Option<PresetSummary>, String> {
    validate(choice)?;
    let Some((name, source)) = source(choice)? else {
        return Ok(None);
    };
    storage::write_typst_style(root, "default.typ", source).map_err(|error| error.to_string())?;
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
                .unwrap_or_else(|| {
                    if filename == "default.typ" {
                        "Custom preset".into()
                    } else {
                        filename.trim_end_matches(".typ").replace('-', " ")
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
pub async fn pick_typst_preset(app: tauri::AppHandle) -> Result<Option<PresetChoice>, String> {
    let Some(picked) = app
        .dialog()
        .file()
        .add_filter("Typst preset", &["typ"])
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(|error| error.to_string())?;
    let metadata = fs::metadata(&path).map_err(|error| error.to_string())?;
    if metadata.len() > goodtype_typst::MAX_SOURCE_BYTES as u64 {
        return Err(format!(
            "Preset is {} bytes; maximum is {}",
            metadata.len(),
            goodtype_typst::MAX_SOURCE_BYTES
        ));
    }
    let bytes = fs::read(&path).map_err(|error| error.to_string())?;
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
    tinymist: tauri::State<'_, Tinymist>,
    root: String,
    choice: PresetChoice,
) -> Result<Option<PresetSummary>, String> {
    let root = ensure_allowed(&roots, &root)?;
    let result = tauri::async_runtime::spawn_blocking(move || install_default(&root, &choice))
        .await
        .map_err(|error| error.to_string())??;
    tinymist.reset();
    Ok(result)
}

#[tauri::command]
pub async fn install_page_typst_preset(
    roots: tauri::State<'_, AllowedRoots>,
    tinymist: tauri::State<'_, Tinymist>,
    root: String,
    choice: PresetChoice,
) -> Result<Option<PresetSummary>, String> {
    let root = ensure_allowed(&roots, &root)?;
    let summary = tauri::async_runtime::spawn_blocking(move || {
        validate(&choice)?;
        let Some((name, source_text)) = source(&choice)? else {
            return Ok(None);
        };
        let filename = page_filename(&choice, source_text)?;
        let path = storage::write_typst_style(&root, &filename, source_text)
            .map_err(|error| error.to_string())?;
        Ok::<_, String>(Some(PresetSummary {
            id: path.clone(),
            name: name.into(),
            description: "Page override".into(),
            import_path: Some(format!("/{path}")),
            kind: "custom",
        }))
    })
    .await
    .map_err(|error| error.to_string())??;
    tinymist.reset();
    Ok(summary)
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
        assert!(install_default(root.path(), &choice).is_err());
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
}
