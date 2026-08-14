//! App-local settings and the recent/pinned notebook list.
//!
//! Both files live in the app configuration directory, never inside a notebook: they are
//! device preferences and navigation history, not canonical notebook content.

use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};
use tauri::Manager;

const MAX_SETTINGS_BYTES: usize = 64 * 1024;
const MAX_RECENTS: usize = 30;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub pen_presets: Vec<PenPreset>,
    pub highlighter: PenPreset,
    /// Editable swatch rows. These are the palette the writer curates; a pen preset's `color`
    /// is merely which swatch that pen is currently set to. Keeping them separate is what lets
    /// a colour be added or edited without silently retargeting a pen.
    pub pen_swatches: Vec<String>,
    pub highlighter_swatches: Vec<String>,
    /// Width chips offered for each tool, in points.
    pub pen_widths: Vec<f64>,
    pub highlighter_widths: Vec<f64>,
    /// Most-recently-used colours, newest first. This is how colours accumulate in practice.
    pub recent_colors: Vec<String>,
    /// Whether stylus pressure varies stroke width at all.
    pub pressure_enabled: bool,
    /// Highlighter-only: pens stay fully opaque so ink stays legible over figures.
    pub highlighter_opacity: f64,
    pub highlighter_straighten: bool,
    pub highlighter_behind_ink: bool,
    pub eraser_size: String,
    pub calibration: PressureCalibration,
    pub undo_scope: UndoScope,
    pub palette_dock: String,
    /// Which side the full-height Typst source view opens on: "left" or "right".
    pub side_editor_dock: String,
    /// Width of that view in CSS pixels.
    pub side_editor_width: f64,
    pub reduced_motion: bool,
    /// Release velocity retained by one-finger touch panning. 1.0 is the original glide.
    pub touch_glide: f64,
    /// Keep Page text headings and display equations on whole paper rows.
    pub page_text_baseline_grid: bool,
    /// Allow downloading Typst Universe packages on a cache miss. Cached packages
    /// keep working either way.
    pub remote_packages: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PenPreset {
    pub width_pt: f64,
    pub color: String,
    /// Which nib from the pen library this slot is set to.
    #[serde(default = "default_pen_type")]
    pub r#type: String,
    /// Whether stylus force varies this slot's width.
    #[serde(default)]
    pub pressure: bool,
}

fn default_pen_type() -> String {
    "fountain".into()
}

const PEN_TYPES: [&str; 5] = ["fountain", "ballpoint", "pencil", "marker", "technical"];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PressureCalibration {
    pub minimum: f64,
    pub maximum: f64,
    pub curve: f64,
    pub smoothing: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UndoScope {
    Page,
    Notebook,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            pen_presets: vec![
                PenPreset {
                    width_pt: 1.6,
                    color: "#1e232b".into(),
                    r#type: "fountain".into(),
                    pressure: true,
                },
                PenPreset {
                    width_pt: 2.8,
                    color: "#2f6fdb".into(),
                    r#type: "ballpoint".into(),
                    pressure: false,
                },
            ],
            highlighter: PenPreset {
                width_pt: 3.78,
                color: "#e0912b".into(),
                r#type: "marker".into(),
                pressure: false,
            },
            pen_swatches: ["#1e232b", "#4c8df0", "#e5645e"].map(String::from).to_vec(),
            highlighter_swatches: ["#e0912b", "#e9d636", "#57c08a"].map(String::from).to_vec(),
            // Two apiece, and both of them a width the shipped nibs actually use, so the row
            // opens showing the pen in hand as the selected one. More can be added; starting
            // with a full row invites picking from a ladder instead of setting a width.
            pen_widths: vec![1.6, 2.8],
            highlighter_widths: vec![3.78, 5.2],
            recent_colors: Vec::new(),
            pressure_enabled: true,
            highlighter_opacity: 0.6,
            highlighter_straighten: false,
            highlighter_behind_ink: true,
            eraser_size: "medium".into(),
            calibration: PressureCalibration {
                minimum: 0.0,
                maximum: 1.0,
                curve: 1.0,
                smoothing: 0.2,
            },
            undo_scope: UndoScope::Page,
            palette_dock: "bottom".into(),
            side_editor_dock: "left".into(),
            side_editor_width: 420.0,
            reduced_motion: false,
            touch_glide: 2.0,
            page_text_baseline_grid: true,
            remote_packages: true,
        }
    }
}

fn clamp(value: f64, minimum: f64, maximum: f64, fallback: f64) -> f64 {
    if value.is_finite() {
        value.clamp(minimum, maximum)
    } else {
        fallback
    }
}

/// Colours the bar carries per tool. Six rather than twelve: the row is two dots wide in a
/// vertical bar, and twelve made a block taller than the rest of the palette combined. The panel
/// keeps presets and recents, so a colour off the row is a tap away. Mirrors `MAX_SWATCHES` in
/// `apps/desktop/src/lib/settings.ts`.
const MAX_SWATCHES: usize = 6;
/// Width chips per tool. Far fewer than colours: the tiles are told apart only by the thickness
/// of the line drawn on them, and past four they stop being distinguishable at a glance. Mirrors
/// `MAX_WIDTHS` in `apps/desktop/src/lib/settings.ts`.
const MAX_WIDTHS: usize = 4;
const MAX_RECENT_COLORS: usize = 8;

/// Accepts `#rrggbb`. Eight-digit hex is deliberately rejected here *and* never produced by the
/// UI: highlighter translucency is a separate opacity setting, so alpha never rides along inside
/// a colour string where a stricter reader would silently drop it.
///
/// Narrower than `goodtype_core::valid_hex_color`, which canonical notebook content uses and
/// which does take alpha. The difference is the point: a swatch is a pen setting, not stored ink.
fn valid_color(color: &str) -> bool {
    color.len() == 7
        && color.starts_with('#')
        && color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn sanitize_swatches(swatches: Vec<String>, fallback: &[String]) -> Vec<String> {
    // `dedup` only collapses neighbours, so a row of red, blue, red kept both reds while reading
    // as though it had not. A swatch row is a set of colours the writer curated; the same colour
    // twice is a wasted slot out of six.
    let mut kept: Vec<String> = Vec::new();
    for color in swatches {
        if valid_color(&color) && !kept.contains(&color) {
            kept.push(color);
        }
    }
    kept.truncate(MAX_SWATCHES);
    if kept.is_empty() {
        fallback.to_vec()
    } else {
        kept
    }
}

fn sanitize_widths(widths: Vec<f64>, fallback: &[f64], minimum: f64, maximum: f64) -> Vec<f64> {
    let kept: Vec<f64> = widths
        .into_iter()
        .filter(|width| width.is_finite() && *width >= minimum && *width <= maximum)
        .take(MAX_WIDTHS)
        .collect();
    if kept.is_empty() {
        fallback.to_vec()
    } else {
        kept
    }
}

/// Settings come from an editable file, so every numeric field is clamped to the documented
/// interaction bounds before it can reach the pen pipeline.
fn sanitize(mut settings: AppSettings) -> AppSettings {
    let defaults = AppSettings::default();
    settings.pen_presets.truncate(4);
    if settings.pen_presets.is_empty() {
        settings.pen_presets = defaults.pen_presets.clone();
    }
    for (index, preset) in settings.pen_presets.iter_mut().enumerate() {
        preset.width_pt = clamp(preset.width_pt, 0.2, 12.0, 1.6);
        if !PEN_TYPES.contains(&preset.r#type.as_str()) {
            preset.r#type = default_pen_type();
        }
        if !valid_color(&preset.color) {
            preset.color = defaults
                .pen_presets
                .get(index)
                .map(|fallback| fallback.color.clone())
                .unwrap_or_else(|| "#1e232b".into());
        }
    }
    settings.highlighter.width_pt = clamp(settings.highlighter.width_pt, 1.0, 20.0, 3.78);
    if !PEN_TYPES.contains(&settings.highlighter.r#type.as_str()) {
        settings.highlighter.r#type = "marker".into();
    }
    if !valid_color(&settings.highlighter.color) {
        settings.highlighter.color = defaults.highlighter.color;
    }
    if !matches!(settings.eraser_size.as_str(), "small" | "medium" | "large") {
        settings.eraser_size = defaults.eraser_size;
    }
    settings.calibration.minimum = clamp(settings.calibration.minimum, 0.0, 0.99, 0.0);
    settings.calibration.maximum = clamp(
        settings.calibration.maximum,
        settings.calibration.minimum + 0.01,
        1.0,
        1.0,
    );
    settings.calibration.curve = clamp(settings.calibration.curve, 0.1, 4.0, 1.0);
    settings.calibration.smoothing = clamp(settings.calibration.smoothing, 0.0, 1.0, 0.2);
    if !matches!(
        settings.palette_dock.as_str(),
        "left" | "right" | "top" | "bottom"
    ) {
        settings.palette_dock = defaults.palette_dock;
    }
    if !matches!(settings.side_editor_dock.as_str(), "left" | "right") {
        settings.side_editor_dock = defaults.side_editor_dock;
    }

    // Swatch rows are writer-curated, so they are filtered rather than replaced: one bad entry
    // must not discard the whole palette. An empty result falls back to the defaults so the row
    // is never unusable.
    settings.pen_swatches = sanitize_swatches(settings.pen_swatches, &defaults.pen_swatches);
    settings.highlighter_swatches = sanitize_swatches(
        settings.highlighter_swatches,
        &defaults.highlighter_swatches,
    );
    settings.recent_colors.retain(|color| valid_color(color));
    settings.recent_colors.truncate(MAX_RECENT_COLORS);
    settings.pen_widths = sanitize_widths(settings.pen_widths, &defaults.pen_widths, 0.2, 12.0);
    settings.highlighter_widths = sanitize_widths(
        settings.highlighter_widths,
        &defaults.highlighter_widths,
        1.0,
        20.0,
    );
    settings.highlighter_opacity = clamp(settings.highlighter_opacity, 0.05, 1.0, 0.6);
    // A generous sanity bound only: the real ceiling is half the window, enforced in the UI,
    // which on a wide display is far more than any fixed pixel cap would allow.
    settings.side_editor_width = clamp(settings.side_editor_width, 280.0, 2400.0, 420.0);
    settings.touch_glide = clamp(settings.touch_glide, 0.0, 4.0, 2.0);
    settings
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RecentNotebooks {
    pub entries: Vec<RecentNotebook>,
    /// Ordered notebook tabs. App-local view state; roots are revalidated before restoration.
    pub open_roots: Vec<String>,
    /// The notebook that was still open when the webview or application last stopped.
    /// App-local view state only; an explicit Close notebook clears it.
    pub active_root: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentNotebook {
    pub root: String,
    pub title: String,
    pub pinned: bool,
    pub last_opened: String,
    #[serde(default)]
    pub last_page_id: Option<String>,
}

fn config_file(app: &tauri::AppHandle, name: &str) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_config_dir()
        .map_err(|error| error.to_string())?;
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    Ok(directory.join(name))
}

fn read_config<T: Default + serde::de::DeserializeOwned>(
    app: &tauri::AppHandle,
    name: &str,
) -> Result<T, String> {
    let path = config_file(app, name)?;
    if !path.is_file() {
        return Ok(T::default());
    }
    let metadata = fs::metadata(&path).map_err(|error| error.to_string())?;
    if metadata.len() > MAX_SETTINGS_BYTES as u64 {
        return Err(format!("{name} exceeds {MAX_SETTINGS_BYTES} bytes"));
    }
    let bytes = fs::read(&path).map_err(|error| error.to_string())?;
    // A corrupt preferences file must not block the application; fall back to defaults.
    Ok(serde_json::from_slice(&bytes).unwrap_or_default())
}

fn write_config<T: Serialize>(app: &tauri::AppHandle, name: &str, value: &T) -> Result<(), String> {
    let path = config_file(app, name)?;
    crate::store::write_json(&path, value, MAX_SETTINGS_BYTES)
        .map_err(|error| format!("{name}: {error}"))
}

#[tauri::command]
pub fn load_app_settings(app: tauri::AppHandle) -> Result<AppSettings, String> {
    read_config::<AppSettings>(&app, "settings.json").map(sanitize)
}

#[tauri::command]
pub fn save_app_settings(
    app: tauri::AppHandle,
    packages: tauri::State<'_, RemotePackages>,
    settings: AppSettings,
) -> Result<AppSettings, String> {
    let settings = sanitize(settings);
    write_config(&app, "settings.json", &settings)?;
    // Keep the compile path's in-memory policy in step with what was just stored.
    packages.set(settings.remote_packages);
    Ok(settings)
}

/// Which directory the writer chose as their library.
///
/// Its own file rather than a field on [`AppSettings`]: that struct is the drafting instrument —
/// nibs, pressure, palette — and is rewritten whenever a swatch changes. Where the work lives is
/// a different kind of fact with a different lifetime, and it should not ride along with a
/// setting the writer changes forty times an hour.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StoredLibrary {
    pub root: Option<String>,
}

pub fn read_library(app: &tauri::AppHandle) -> Result<StoredLibrary, String> {
    read_config::<StoredLibrary>(app, "library.json")
}

pub fn write_library(app: &tauri::AppHandle, library: &StoredLibrary) -> Result<(), String> {
    write_config(app, "library.json", library)
}

#[tauri::command]
pub fn list_recent_notebooks(app: tauri::AppHandle) -> Result<Vec<RecentNotebook>, String> {
    let mut recents = read_config::<RecentNotebooks>(&app, "recents.json")?;
    // Pinned first, then most recently opened.
    recents.entries.sort_by(|a, b| {
        b.pinned
            .cmp(&a.pinned)
            .then(b.last_opened.cmp(&a.last_opened))
    });
    Ok(recents.entries)
}

pub fn is_recent_notebook(app: &tauri::AppHandle, root: &str) -> Result<bool, String> {
    Ok(read_config::<RecentNotebooks>(app, "recents.json")?
        .entries
        .iter()
        .any(|entry| entry.root == root))
}

pub fn active_recent_root(app: &tauri::AppHandle) -> Result<Option<String>, String> {
    Ok(read_config::<RecentNotebooks>(app, "recents.json")?.active_root)
}

fn session_roots(recents: &RecentNotebooks) -> Vec<String> {
    if recents.open_roots.is_empty() {
        recents.active_root.clone().into_iter().collect()
    } else {
        recents.open_roots.clone()
    }
}

pub fn recent_session(app: &tauri::AppHandle) -> Result<(Vec<String>, Option<String>), String> {
    let recents = read_config::<RecentNotebooks>(app, "recents.json")?;
    let roots = session_roots(&recents);
    Ok((roots, recents.active_root))
}

pub fn record_recent_session(
    app: &tauri::AppHandle,
    roots: Vec<String>,
    active_root: Option<String>,
) -> Result<(), String> {
    let mut recents = read_config::<RecentNotebooks>(app, "recents.json")?;
    let mut open_roots = Vec::new();
    for root in roots {
        if recents.entries.iter().any(|entry| entry.root == root) && !open_roots.contains(&root) {
            open_roots.push(root);
        }
    }
    open_roots.truncate(MAX_RECENTS);
    recents.active_root = active_root.filter(|root| open_roots.contains(root));
    recents.open_roots = open_roots;
    write_config(app, "recents.json", &recents)
}

pub fn clear_active_recent(app: &tauri::AppHandle, root: &str) -> Result<(), String> {
    let mut recents = read_config::<RecentNotebooks>(app, "recents.json")?;
    let before = recents.open_roots.len();
    recents.open_roots.retain(|known| known != root);
    let was_active = recents.active_root.as_deref() == Some(root);
    if was_active {
        recents.active_root = None;
    }
    if before == recents.open_roots.len() && !was_active {
        return Ok(());
    }
    write_config(app, "recents.json", &recents)
}

pub fn record_recent(
    app: &tauri::AppHandle,
    root: &str,
    title: &str,
    opened_at: &str,
) -> Result<(), String> {
    let mut recents = read_config::<RecentNotebooks>(app, "recents.json")?;
    let pinned = recents
        .entries
        .iter()
        .find(|entry| entry.root == root)
        .is_some_and(|entry| entry.pinned);
    let last_page_id = recents
        .entries
        .iter()
        .find(|entry| entry.root == root)
        .and_then(|entry| entry.last_page_id.clone());
    recents.entries.retain(|entry| entry.root != root);
    recents.entries.insert(
        0,
        RecentNotebook {
            root: root.to_owned(),
            title: title.to_owned(),
            pinned,
            last_opened: opened_at.to_owned(),
            last_page_id,
        },
    );
    recents.active_root = Some(root.to_owned());
    if !recents.open_roots.iter().any(|known| known == root) {
        recents.open_roots.push(root.to_owned());
    }
    while recents.entries.len() > MAX_RECENTS {
        // Drop the oldest unpinned entry; pinned entries survive the cap.
        let Some(index) = recents.entries.iter().rposition(|entry| !entry.pinned) else {
            break;
        };
        recents.entries.remove(index);
    }
    write_config(app, "recents.json", &recents)
}

pub fn record_recent_page(app: &tauri::AppHandle, root: &str, page_id: &str) -> Result<(), String> {
    if page_id.is_empty() || page_id.len() > 128 {
        return Err("page id must be present and bounded".to_owned());
    }
    let mut recents = read_config::<RecentNotebooks>(app, "recents.json")?;
    let entry = recents
        .entries
        .iter_mut()
        .find(|entry| entry.root == root)
        .ok_or_else(|| "notebook is not in recents".to_owned())?;
    entry.last_page_id = Some(page_id.to_owned());
    write_config(app, "recents.json", &recents)
}

#[tauri::command]
pub fn set_notebook_pinned(
    app: tauri::AppHandle,
    root: String,
    pinned: bool,
) -> Result<Vec<RecentNotebook>, String> {
    let mut recents = read_config::<RecentNotebooks>(&app, "recents.json")?;
    for entry in &mut recents.entries {
        if entry.root == root {
            entry.pinned = pinned;
        }
    }
    write_config(&app, "recents.json", &recents)?;
    list_recent_notebooks(app)
}

#[tauri::command]
pub fn remove_recent_notebook(
    app: tauri::AppHandle,
    root: String,
) -> Result<Vec<RecentNotebook>, String> {
    let mut recents = read_config::<RecentNotebooks>(&app, "recents.json")?;
    recents.entries.retain(|entry| entry.root != root);
    recents.open_roots.retain(|known| known != &root);
    if recents.active_root.as_deref() == Some(root.as_str()) {
        recents.active_root = None;
    }
    write_config(&app, "recents.json", &recents)?;
    list_recent_notebooks(app)
}

/// Whether an uncached Typst package may be downloaded, held in memory so the
/// compile path never reads the settings file. Seeded at startup and updated whenever settings
/// are saved.
#[derive(Debug)]
pub struct RemotePackages(std::sync::atomic::AtomicBool);

impl Default for RemotePackages {
    fn default() -> Self {
        Self(std::sync::atomic::AtomicBool::new(
            AppSettings::default().remote_packages,
        ))
    }
}

impl RemotePackages {
    pub fn allowed(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn set(&self, allowed: bool) {
        self.0.store(allowed, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Seed the in-memory policy from the stored settings at startup.
pub fn seed_remote_packages(app: &tauri::AppHandle, policy: &RemotePackages) {
    if let Ok(settings) = read_config::<AppSettings>(app, "settings.json") {
        policy.set(sanitize(settings).remote_packages);
    }
}

#[cfg(test)]
mod tests {
    use super::{AppSettings, RecentNotebooks, sanitize, session_roots};

    #[test]
    fn touch_glide_has_a_bounded_hardware_tuning_range() {
        let mut settings = AppSettings::default();
        assert_eq!(settings.touch_glide, 2.0);
        settings.touch_glide = 5.0;
        assert_eq!(sanitize(settings).touch_glide, 4.0);
    }

    #[test]
    fn old_settings_enable_the_page_text_baseline_grid() {
        let settings: AppSettings =
            serde_json::from_str("{}").expect("legacy settings should load");
        assert!(settings.page_text_baseline_grid);
    }

    #[test]
    fn old_recents_files_default_to_no_active_notebook() {
        let recents: RecentNotebooks =
            serde_json::from_str(r#"{"entries":[]}"#).expect("legacy recents should still load");
        assert!(recents.active_root.is_none());
        assert!(recents.open_roots.is_empty());
    }

    #[test]
    fn old_active_notebook_becomes_the_only_restored_tab() {
        let recents: RecentNotebooks =
            serde_json::from_str(r#"{"entries":[],"activeRoot":"C:\\notes\\physics"}"#)
                .expect("single-root session should still load");
        assert_eq!(session_roots(&recents), [r"C:\notes\physics"]);
    }
}
