//! Handwriting transcription through Windows Ink and an optional math add-on (ADR 0040).
//!
//! The frontend sends a copy of the selected strokes in page points. Nothing here reads or writes
//! a notebook: a confirmed transcription becomes an ordinary
//! Typst block through the normal page commit, and the ink stays where it was.
//!
//! Text uses Windows Ink; math uses the optional offline add-on.

use serde::{Deserialize, Serialize};

mod latex;
pub(crate) mod math_process;

const MAX_STROKES: usize = 2_000;
const MAX_POINTS: usize = 200_000;
const MAX_COORDINATE_PT: f64 = 100_000.0;
const MAX_RECOGNIZER_NAME_CHARS: usize = 200;

/// Stroke polylines in page points, already through the stroke transform.
pub type Strokes = Vec<Vec<[f64; 2]>>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    /// Installed handwriting recognizers, by their Windows display name. Empty means no text.
    pub recognizers: Vec<String>,
    pub math: crate::importer::ImporterStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextRecognition {
    pub text: String,
}

/// Offline add-on proposal for review before insertion.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MathRecognition {
    /// Typst math content, without the surrounding `$`. Absent when the LaTeX could not be
    /// translated faithfully.
    pub typst: Option<String>,
    /// The first construct the translator refused, for the writer to see. Absent on success.
    pub unsupported: Option<String>,
    pub latex: String,
}

#[derive(Debug, Deserialize)]
pub struct InkSelection {
    strokes: Strokes,
}

fn validated(selection: InkSelection) -> Result<Strokes, String> {
    let strokes = selection.strokes;
    if strokes.is_empty() || strokes.len() > MAX_STROKES {
        return Err(format!("a transcription takes 1 to {MAX_STROKES} strokes"));
    }
    let points: usize = strokes.iter().map(Vec::len).sum();
    if points > MAX_POINTS {
        return Err(format!("a transcription takes at most {MAX_POINTS} points"));
    }
    let finite = strokes
        .iter()
        .flatten()
        .flatten()
        .all(|value| value.is_finite() && value.abs() <= MAX_COORDINATE_PT);
    if !finite {
        return Err("stroke coordinates must be finite page points".to_owned());
    }
    // A stroke without points has nothing to recognize and the native builders reject it.
    Ok(strokes
        .into_iter()
        .filter(|stroke| !stroke.is_empty())
        .collect())
}

#[cfg_attr(not(windows), allow(dead_code))]
fn origin(strokes: &Strokes) -> (f64, f64) {
    strokes
        .iter()
        .flatten()
        .fold((f64::INFINITY, f64::INFINITY), |(x, y), point| {
            (x.min(point[0]), y.min(point[1]))
        })
}

#[tauri::command]
pub async fn recognition_capabilities(
    app: tauri::AppHandle,
    importers: tauri::State<'_, crate::importer::Importers>,
) -> Result<Capabilities, String> {
    let math = crate::importer::importer_status(app, importers, "math".into()).await?;
    let recognizers = tauri::async_runtime::spawn_blocking(native::capabilities)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Capabilities { recognizers, math })
}

#[tauri::command]
pub async fn prepare_math_recognition(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        use tauri::Manager;
        app.state::<math_process::MathProcess>().prepare(&app)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn recognize_ink_text(
    selection: InkSelection,
    recognizer: Option<String>,
) -> Result<TextRecognition, String> {
    let strokes = validated(selection)?;
    if recognizer
        .as_ref()
        .is_some_and(|name| name.chars().count() > MAX_RECOGNIZER_NAME_CHARS)
    {
        return Err("the recognizer name is too long".to_owned());
    }
    tauri::async_runtime::spawn_blocking(move || {
        native::recognize_text(&strokes, recognizer.as_deref())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn recognize_ink_math(
    app: tauri::AppHandle,
    selection: math_process::MathSelection,
) -> Result<MathRecognition, String> {
    selection.validate()?;
    tauri::async_runtime::spawn_blocking(move || {
        use tauri::Manager;
        let latex = app
            .state::<math_process::MathProcess>()
            .recognize(&app, selection)?;
        Ok(match latex::latex_to_typst(&latex) {
            Ok(typst) => MathRecognition {
                typst: Some(typst),
                unsupported: None,
                latex,
            },
            Err(unsupported) => MathRecognition {
                typst: None,
                unsupported: Some(unsupported),
                latex,
            },
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(not(windows))]
mod native {
    use super::{Strokes, TextRecognition};
    pub fn capabilities() -> Vec<String> {
        Vec::new()
    }
    pub fn recognize_text(
        _strokes: &Strokes,
        _recognizer: Option<&str>,
    ) -> Result<TextRecognition, String> {
        Err("handwriting recognition is only available on Windows".into())
    }
}

#[cfg(windows)]
mod native {
    use super::{Strokes, TextRecognition, origin};
    use windows::Foundation::Point;
    use windows::UI::Input::Inking::{
        InkRecognitionTarget, InkRecognizerContainer, InkStrokeBuilder, InkStrokeContainer,
    };
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
    use windows::core::HSTRING;
    const DIP_PER_PT: f64 = 96.0 / 72.0;
    const MARGIN_DIP: f64 = 10.0;

    /// Each recognition runs on its own thread with its own COM apartment, so neither the async
    /// runtime's pooled threads nor the UI thread inherit an apartment they did not ask for.
    fn on_com_thread<T: Send + 'static>(
        apartment: windows::Win32::System::Com::COINIT,
        work: impl FnOnce() -> windows::core::Result<T> + Send + 'static,
    ) -> Result<T, String> {
        std::thread::spawn(move || unsafe {
            CoInitializeEx(None, apartment).ok()?;
            let result = work();
            CoUninitialize();
            result
        })
        .join()
        .map_err(|_| "the recognition thread stopped unexpectedly".to_owned())?
        .map_err(|error| error.message())
    }

    pub fn capabilities() -> Vec<String> {
        on_com_thread(COINIT_MULTITHREADED, || {
            let names = InkRecognizerContainer::new()?.GetRecognizers()?;
            names
                .into_iter()
                .map(|recognizer| Ok(recognizer.Name()?.to_string()))
                .collect::<windows::core::Result<Vec<_>>>()
        })
        .unwrap_or_default()
    }

    pub fn recognize_text(
        strokes: &Strokes,
        recognizer: Option<&str>,
    ) -> Result<TextRecognition, String> {
        let strokes = strokes.clone();
        let recognizer = recognizer.map(str::to_owned);
        on_com_thread(COINIT_MULTITHREADED, move || {
            let engine = InkRecognizerContainer::new()?;
            if let Some(name) = recognizer {
                let wanted = HSTRING::from(name);
                if let Some(found) = engine
                    .GetRecognizers()?
                    .into_iter()
                    .find(|candidate| candidate.Name().is_ok_and(|n| n == wanted))
                {
                    engine.SetDefaultRecognizer(&found)?;
                }
            }
            let (left, top) = origin(&strokes);
            let builder = InkStrokeBuilder::new()?;
            let container = InkStrokeContainer::new()?;
            for stroke in &strokes {
                let points: Vec<Point> = stroke
                    .iter()
                    .map(|point| Point {
                        X: (MARGIN_DIP + (point[0] - left) * DIP_PER_PT) as f32,
                        Y: (MARGIN_DIP + (point[1] - top) * DIP_PER_PT) as f32,
                    })
                    .collect();
                container.AddStroke(
                    &builder.CreateStroke(&windows_collections::IIterable::from(points))?,
                )?;
            }
            let segments = engine
                .RecognizeAsync(&container, InkRecognitionTarget::All)?
                .get()?;
            let mut words = Vec::new();
            for segment in segments {
                if let Ok(best) = segment.GetTextCandidates()?.GetAt(0) {
                    words.push(best.to_string());
                }
            }
            Ok(TextRecognition {
                text: words.join(" "),
            })
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    /// Needs an installed Windows handwriting recognizer: `cargo test -- --ignored native_text`.
    #[test]
    #[ignore = "depends on the recognizers installed on this Windows machine"]
    fn native_text_recognizes_synthetic_digits() {
        let capabilities = native::capabilities();
        assert!(
            !capabilities.is_empty(),
            "no handwriting recognizer installed"
        );
        // "1", then "7": vertical bar; bar with a top stroke. Page points, roughly 20pt tall.
        let strokes = vec![
            (0..=20).map(|i| [10.0, 10.0 + f64::from(i)]).collect(),
            (0..=10)
                .map(|i| [24.0 + f64::from(i), 10.0])
                .chain((0..=20).map(|i| [34.0 - f64::from(i) * 0.4, 10.0 + f64::from(i)]))
                .collect(),
        ];
        let result = native::recognize_text(&strokes, None).expect("text recognition");
        eprintln!("recognizers {:?}, text {:?}", capabilities, result.text);
        assert!(!result.text.trim().is_empty());
    }

    #[test]
    fn rejects_unbounded_ink() {
        assert!(
            validated(InkSelection {
                strokes: Vec::new()
            })
            .is_err()
        );
        assert!(
            validated(InkSelection {
                strokes: vec![vec![[f64::NAN, 0.0]]]
            })
            .is_err()
        );
        assert!(
            validated(InkSelection {
                strokes: vec![vec![[0.0, 0.0]; MAX_POINTS + 1]]
            })
            .is_err()
        );
        assert_eq!(
            validated(InkSelection {
                strokes: vec![vec![[1.0, 2.0]], Vec::new()]
            })
            .map(|s| s.len()),
            Ok(1)
        );
    }
}
