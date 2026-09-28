//! Serializable metadata and ownership rules. Runtime secrets never enter these types.
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

pub type Id = Uuid;

/// Identifies one connection attempt, including retries in the same pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Owner {
    pub session: Id,
    pub attempt: Id,
}

impl Owner {
    /// Allocate an independent session and first attempt.
    pub fn new() -> Self {
        Self {
            session: Id::new_v4(),
            attempt: Id::new_v4(),
        }
    }
}
impl Default for Owner {
    fn default() -> Self {
        Self::new()
    }
}

/// An exportable connection profile. A credential key is derived from its UUID.
/// Authentication is password-only; terminal encoding lives on the session, not here.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Profile {
    #[serde(default = "Id::new_v4")]
    pub id: Id,
    pub name: String,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub username: String,
}
fn default_port() -> u16 {
    22
}

impl Profile {
    /// Validate metadata without resolving hosts.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("Connection name is required".into());
        }
        if self.host.trim().is_empty()
            || self.host.chars().any(char::is_whitespace)
            || self.host.contains(['/', '\0'])
        {
            return Err("Host must be a hostname or IP address without spaces".into());
        }
        if self.port == 0 {
            return Err("Port must be between 1 and 65535".into());
        }
        if self.username.trim().is_empty() || self.username.contains(['\n', '\r', '\0']) {
            return Err("A valid username is required".into());
        }
        Ok(())
    }

    /// Match imports by explicit ID or complete named endpoint, preserving alternate profiles.
    pub fn duplicates(&self, other: &Self) -> bool {
        self.id == other.id
            || (self.name == other.name
                && self.host.eq_ignore_ascii_case(&other.host)
                && self.port == other.port
                && self.username == other.username)
    }
    /// Preserve the command's originating profile; the SSH history view combines these identities.
    pub fn scope(&self) -> String {
        format!("ssh:{}", self.id)
    }
    /// A display address keeps non-default ports and brackets IPv6 literals.
    pub fn address(&self) -> String {
        let host = if self.host.contains(':') && !self.host.starts_with('[') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        if self.port == 22 {
            host
        } else {
            format!("{host}:{}", self.port)
        }
    }
    /// Human-readable endpoint for target confirmations.
    pub fn endpoint(&self) -> String {
        format!("{}@{}:{}", self.username, self.host, self.port)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionSpec {
    Local {
        shell: String,
        directory: String,
        encoding: crate::encoding::Encoding,
    },
    Ssh {
        profile: Profile,
        /// Live-session terminal encoding; defaults for restored workspaces.
        #[serde(default)]
        encoding: crate::encoding::Encoding,
    },
}
impl SessionSpec {
    /// Persist the original source identity so existing history remains compatible.
    pub fn scope(&self) -> String {
        match self {
            Self::Local { .. } => "local".into(),
            Self::Ssh { profile, .. } => profile.scope(),
        }
    }
    /// All SSH sessions share one history view, while local commands remain separate.
    pub fn history_scope(&self) -> HistoryScope {
        match self {
            Self::Local { .. } => HistoryScope::Local,
            Self::Ssh { .. } => HistoryScope::Ssh,
        }
    }
    /// The live session owns its encoding, independent of later profile edits.
    pub fn encoding(&self) -> crate::encoding::Encoding {
        match self {
            Self::Local { encoding, .. } => *encoding,
            Self::Ssh { encoding, .. } => *encoding,
        }
    }
    /// Compact identity used in the tab and target confirmations.
    /// Fixed identity text for confirmations, including the SSH address when a friendly name exists.
    pub fn context_label(&self) -> String {
        match self {
            Self::Ssh { profile, .. } => format!(
                "{} · {}@{}",
                self.label(),
                profile.username,
                profile.address()
            ),
            Self::Local { directory, .. } => format!("Local · {directory}"),
        }
    }
    pub fn label(&self) -> String {
        match self {
            Self::Local { shell, .. } => std::path::Path::new(shell)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            Self::Ssh { profile, .. } => {
                if profile.name.trim().is_empty() {
                    profile.address()
                } else {
                    profile.name.clone()
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    Day,
    Night,
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    Zh,
    En,
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Split {
    #[default]
    Horizontal,
    Vertical,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    Files,
    History,
    System,
    Editor,
}

/// Preferences use serde defaults so a missing newly introduced property is harmless.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub theme: Theme,
    pub language: Language,
    pub ui_font: String,
    pub ui_size: f32,
    pub terminal_font: String,
    pub terminal_size: f32,
    pub tool_preferred_width: Option<f32>,
    pub files_preferred_height: Option<f32>,
    pub tool: Option<Tool>,
    pub window_width: f32,
    pub window_height: f32,
    pub window_x: Option<f32>,
    pub window_y: Option<f32>,
    pub local_encoding: crate::encoding::Encoding,
    /// Last folder chosen in the export dialog; Downloads until the user picks one.
    pub export_directory: Option<String>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: Theme::Day,
            language: Language::Zh,
            ui_font: crate::platform::ui_font().into(),
            ui_size: 14.,
            terminal_font: crate::platform::terminal_font().into(),
            terminal_size: 12.,
            tool_preferred_width: None,
            files_preferred_height: None,
            tool: None,
            window_width: 1280.,
            window_height: 800.,
            window_x: None,
            window_y: None,
            local_encoding: crate::encoding::Encoding::Utf8,
            export_directory: None,
        }
    }
}
impl Preferences {
    /// Reject unusable persisted sizes without touching unrelated settings.
    pub fn normalize(&mut self) {
        fn bounded(value: f32, fallback: f32, low: f32, high: f32) -> f32 {
            if value.is_finite() {
                value.clamp(low, high)
            } else {
                fallback
            }
        }
        self.ui_size = bounded(self.ui_size, 14., 12., 18.);
        self.terminal_size = bounded(self.terminal_size, 12., 12., 18.);
        if self
            .tool_preferred_width
            .is_some_and(|v| !v.is_finite() || v < 0.)
        {
            self.tool_preferred_width = None;
        }
        if let Some(value) = self.files_preferred_height {
            self.files_preferred_height = value.is_finite().then(|| value.clamp(120., 600.));
        }
        self.window_width = bounded(self.window_width, 1280., 960., 8000.);
        self.window_height = bounded(self.window_height, 800., 640., 8000.);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemPage {
    #[default]
    Overview,
    Processes,
    Ports,
}

fn default_tool() -> Tool {
    Tool::Files
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedPane {
    pub id: Id,
    pub spec: SessionSpec,
    #[serde(default)]
    pub tool: Option<Tool>,
    #[serde(default = "default_tool")]
    pub last_tool: Tool,
    #[serde(default)]
    pub system_page: SystemPage,
}
impl SavedPane {
    pub fn new(spec: SessionSpec) -> Self {
        let tool = if matches!(spec, SessionSpec::Ssh { .. }) {
            Some(Tool::Files)
        } else {
            None
        };
        Self {
            id: Id::new_v4(),
            spec,
            tool,
            last_tool: Tool::Files,
            system_page: SystemPage::Overview,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedTab {
    pub id: Id,
    pub panes: Vec<SavedPane>,
    pub active_pane: Id,
    #[serde(default, deserialize_with = "read_layout")]
    pub layout: Option<crate::layout::PaneLayout>,
}
/// A damaged layout must not prevent otherwise valid session records from being recovered.
fn read_layout<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<crate::layout::PaneLayout>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).ok())
}
impl SavedTab {
    pub fn single(id: Id, pane: SavedPane) -> Self {
        Self {
            id,
            active_pane: pane.id,
            layout: Some(crate::layout::PaneLayout::single(pane.id)),
            panes: vec![pane],
        }
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Workspace {
    pub tabs: Vec<SavedTab>,
    pub active_tab: usize,
}
impl Workspace {
    /// Repair graphs and focus without silently dropping usable local or SSH sessions.
    pub fn normalize(&mut self) {
        use std::collections::HashSet;
        let mut result = Vec::new();
        let mut active = 0;
        let mut pane_ids = HashSet::new();
        let mut tab_ids = HashSet::new();
        for (index, mut tab) in std::mem::take(&mut self.tabs).into_iter().enumerate() {
            if tab.panes.is_empty() {
                continue;
            }
            if !tab_ids.insert(tab.id) {
                tab.id = Id::new_v4();
                tab_ids.insert(tab.id);
            }
            let selected = tab
                .panes
                .iter()
                .position(|p| p.id == tab.active_pane)
                .unwrap_or(0);
            for pane in &mut tab.panes {
                if !pane_ids.insert(pane.id) {
                    pane.id = Id::new_v4();
                    pane_ids.insert(pane.id);
                }
                if matches!(pane.spec, SessionSpec::Local { .. }) {
                    pane.tool = None;
                }
            }
            tab.active_pane = tab.panes[selected].id;
            let ids: Vec<_> = tab.panes.iter().map(|p| p.id).collect();
            let valid = tab.panes.len() <= crate::layout::MAX_LOCAL_PANES
                && (tab.panes.len() == 1
                    || tab
                        .panes
                        .iter()
                        .all(|p| matches!(p.spec, SessionSpec::Local { .. })))
                && tab.layout.as_mut().is_some_and(|l| l.normalize(&ids));
            if valid {
                if index == self.active_tab {
                    active = result.len();
                }
                result.push(tab);
            } else {
                for (pane_index, pane) in tab.panes.into_iter().enumerate() {
                    if index == self.active_tab && pane_index == selected {
                        active = result.len();
                    }
                    result.push(SavedTab::single(
                        if pane_index == 0 {
                            tab.id
                        } else {
                            Id::new_v4()
                        },
                        pane,
                    ));
                }
            }
        }
        self.active_tab = active.min(result.len().saturating_sub(1));
        self.tabs = result;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: Id,
    pub scope: String,
    pub command: String,
    pub timestamp: i64,
}

/// Logical history lists share browsing state without rewriting stored command origins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryScope {
    Local,
    Ssh,
}
impl HistoryScope {
    /// Stable index for the two window-owned history views.
    pub fn index(self) -> usize {
        match self {
            Self::Local => 0,
            Self::Ssh => 1,
        }
    }
    /// Resolve a display label without using the active host as the shared history's identity.
    pub fn label_key(self) -> &'static str {
        match self {
            Self::Local => "local_history",
            Self::Ssh => "shared_ssh_history",
        }
    }
    /// Include legacy SSH profile UUIDs even after a profile is renamed or removed.
    pub fn includes(self, origin: &str) -> bool {
        match self {
            Self::Local => origin == "local",
            Self::Ssh => {
                origin == "ssh"
                    || origin
                        .strip_prefix("ssh:")
                        .is_some_and(|id| Id::parse_str(id).is_ok())
            }
        }
    }
    /// Resolve a stored origin into its logical list; unknown origins stay unlisted.
    pub fn group_of(origin: &str) -> Option<Self> {
        [Self::Local, Self::Ssh]
            .into_iter()
            .find(|scope| scope.includes(origin))
    }
}
/// Apply one plain, Shift-range, or additive click to a visible ordered selection.
/// The anchor is kept by UUID, so filtering and virtualized row rebuilding are safe.
pub fn update_visible_selection(
    visible: &[Id],
    selected: &mut HashSet<Id>,
    anchor: &mut Option<Id>,
    id: Id,
    shift: bool,
    additive: bool,
) {
    if !visible.contains(&id) {
        return;
    }
    if shift {
        let anchor_id = anchor
            .filter(|anchor| visible.contains(anchor))
            .unwrap_or(id);
        let Some(start) = visible.iter().position(|row| *row == anchor_id) else {
            return;
        };
        let Some(end) = visible.iter().position(|row| *row == id) else {
            return;
        };
        if !additive {
            selected.clear();
        }
        selected.extend(visible[start.min(end)..=start.max(end)].iter().copied());
    } else if additive {
        if !selected.remove(&id) {
            selected.insert(id);
        }
    } else {
        selected.clear();
        selected.insert(id);
    }
    if !shift {
        *anchor = Some(id);
    }
}
/// Select only currently visible, explicitly selected history UUIDs for deletion.
/// The returned list is frozen before confirmation, so later records stay untouched.
pub fn history_delete_targets(visible: &[Id], selected: &std::collections::HashSet<Id>) -> Vec<Id> {
    visible
        .iter()
        .copied()
        .filter(|id| selected.contains(id))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    Restored,
    Connecting,
    HostVerification,
    CredentialsRequired,
    Authenticating,
    Connected,
    Disconnected,
    Cancelled,
    Failed(String),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransferState {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
}
impl TransferState {
    /// Terminal states reject late progress and completion from cancelled workers.
    pub fn active(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferRecord {
    pub id: Id,
    pub profile: Profile,
    pub upload: bool,
    pub local: String,
    pub remote: String,
    #[serde(default)]
    pub session: Option<Id>,
    #[serde(default)]
    pub attempt: Option<Id>,
    pub state: TransferState,
    pub bytes: u64,
    pub total: Option<u64>,
    pub error: Option<String>,
    pub timestamp: i64,
}
impl TransferRecord {
    /// Match the exact original transport, not another instance of the same saved profile.
    pub fn belongs_to(&self, owner: Owner) -> bool {
        self.session == Some(owner.session) && self.attempt == Some(owner.attempt)
    }
    /// Apply progress only to an active task, preserving terminal-state ownership.
    pub fn progress(&mut self, bytes: u64, total: Option<u64>) {
        if self.state.active() {
            self.state = TransferState::Running;
            self.bytes = bytes;
            self.total = total;
        }
    }
}
