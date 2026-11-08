use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Usage {
    Low,
    #[default]
    Balanced,
    Detailed,
}
impl Usage {
    pub fn limits(self) -> (usize, usize, usize, usize, u32) {
        match self {
            Self::Low => (16 * 1024, 5, 1, 10, 0),
            Self::Balanced => (64 * 1024, 10, 3, 30, 1024),
            Self::Detailed => (256 * 1024, 25, 6, 60, 2048),
        }
    }
}
/// What happens when the assistant asks for more context than the user attached.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Permission {
    /// Granted without a prompt, for the current request only.
    Allow,
    /// Goodtype shows an approval card; no answer within the timeout is a denial.
    #[default]
    Ask,
    /// Refused without a prompt.
    Never,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct HelperPermissions {
    pub pages: Permission,
    pub presets: Permission,
    pub page_names: Permission,
}
impl Default for HelperPermissions {
    fn default() -> Self {
        Self {
            pages: Permission::Ask,
            presets: Permission::Ask,
            page_names: Permission::Allow,
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HelperSettings {
    pub enabled: bool,
    pub usage: Usage,
    pub web_search: bool,
    pub codex_path: Option<String>,
    pub codex_model: Option<String>,
    pub allowed_mcp_servers: Vec<String>,
    pub permissions: HelperPermissions,
}
impl Default for HelperSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            usage: Usage::Balanced,
            web_search: true,
            codex_path: None,
            codex_model: None,
            allowed_mcp_servers: Vec::new(),
            permissions: HelperPermissions::default(),
        }
    }
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl Rect {
    pub fn valid(self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|v| v.is_finite())
            && self.x >= 0.0
            && self.y >= 0.0
            && self.width > 0.0
            && self.height > 0.0
    }
    pub fn contains(self, other: Self) -> bool {
        other.valid()
            && other.x >= self.x
            && other.y >= self.y
            && other.x + other.width <= self.x + self.width
            && other.y + other.height <= self.y + self.height
    }
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharePayload {
    pub root: String,
    pub page_id: String,
    pub object_ids: Vec<String>,
    pub stroke_ids: Vec<String>,
    pub crop_pt: Rect,
    pub session_ids: Option<Vec<String>>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareInfo {
    pub page_id: String,
    pub page_number: usize,
    pub object_count: usize,
    pub crop_pt: Rect,
    pub session_ids: Vec<String>,
    pub shared_at: u64,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: String,
    pub client_name: String,
    pub client_version: String,
    pub approved: bool,
    pub connected_at: u64,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HelperStatus {
    pub enabled: bool,
    pub listening: bool,
    pub relay_path: Option<String>,
    pub relay_problem: Option<UiProblem>,
    pub skill_path: Option<String>,
    pub usage: Usage,
    pub sessions: Vec<SessionInfo>,
    pub share: Option<ShareInfo>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextReply {
    pub request_id: String,
    pub status: String,
    pub root: Option<String>,
    pub page_id: Option<String>,
    pub edit_generation: Option<u64>,
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HandedOver {
    pub source_bytes: usize,
    pub strokes: usize,
    pub images: Vec<ImageSize>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ImageSize {
    pub width: u32,
    pub height: u32,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEntry {
    pub usage_override: bool,
    pub at: u64,
    pub session_id: String,
    pub client_name: String,
    pub tool: String,
    pub grant: String,
    pub outcome: String,
    pub handed_over: HandedOver,
    pub detail_code: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct UiProblem {
    pub code: String,
    pub params: std::collections::HashMap<String, String>,
}

/// Ephemeral draft intent. No field is canonical notebook data.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestContextIntent {
    #[serde(default)]
    pub share_page_names: bool,
    #[serde(default)]
    pub cells: Option<CellGrantIntent>,
    pub destination: RequestDestination,
    pub selection: Option<SharePayload>,
    pub page_ids: Vec<String>,
    pub presets: bool,
    pub intent_version: String,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestDestination {
    pub root: String,
    pub page_id: String,
}
#[derive(Clone)]
pub struct CapturedPage {
    pub base: goodtype_core::storage::ProposalBase,
    pub snapshot: goodtype_core::storage::NotebookSnapshot,
    pub files: goodtype_typst::GrantedFiles,
    pub render_files: goodtype_typst::GrantedFiles,
}
/// Installed only in the admitted session; snapshots and dependency bytes never persist.
#[derive(Clone)]
pub struct CapturedRequestGrant {
    pub share_page_names: bool,
    /// Python cells by page id: attached Python pages and an explicit cell selection alike.
    pub cells: std::collections::BTreeMap<String, super::cells::CapturedCells>,
    pub usage_override: bool,
    pub intent_version: String,
    pub request_id: String,
    pub conversation_id: String,
    pub session_id: String,
    pub epoch: u64,
    pub root: String,
    pub destination: goodtype_core::storage::NotebookSnapshot,
    pub destination_base: goodtype_core::storage::ProposalBase,
    pub anchor: SharePayload,
    pub info: ShareInfo,
    pub selection: Option<CapturedPage>,
    pub pages: std::collections::HashMap<String, CapturedPage>,
    pub preset_files: goodtype_typst::GrantedFiles,
    pub usage: Usage,
}
impl CapturedRequestGrant {
    pub fn files(&self) -> goodtype_typst::GrantedFiles {
        let mut files = self.preset_files.clone();
        for page in self.pages.values().chain(self.selection.iter()) {
            files.extend(page.files.clone());
        }
        files
    }
    pub fn source(&self, page: &str) -> Option<&CapturedPage> {
        self.pages.get(page).or_else(|| {
            self.selection
                .as_ref()
                .filter(|s| s.snapshot.page.id == page)
        })
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CellGrantIntent {
    pub calculation_id: String,
    pub part_id: String,
    pub cell_ids: Vec<String>,
    pub include_outputs: bool,
}
