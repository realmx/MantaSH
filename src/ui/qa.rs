//! Opt-in debug-only native acceptance driver; never enabled in ordinary application runs.
use super::*;
use alacritty_terminal::grid::Dimensions;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Request {
    pub sequence: u64,
    #[serde(flatten)]
    pub action: Action,
}
/// Default NSEvent click count for synthesized pointer gestures.
fn default_click_count() -> i64 {
    1
}
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    Snapshot,
    /// Read the official release endpoint now; never confirms or installs an update.
    CheckUpdates {
        #[serde(default)]
        manual: bool,
    },
    /// Open the client-drawn About dialog that carries the manual update entry.
    About,
    /// Send a bounded pointer gesture only to this isolated app's own native window.
    PointerGesture {
        points: Vec<[f32; 2]>,
        #[serde(default)]
        cancel: bool,
        /// Click count stamped on the synthesized NSEvent pair (2 = double-click).
        #[serde(default = "default_click_count")]
        click_count: i64,
    },
    /// Open and dismiss only this process's native Window menu without choosing an action.
    WindowMenuProbe,
    /// Select the first item of this process's real AppKit application menu.
    ApplicationAboutMenu,
    /// Close this isolated process's native About panel without touching the workbench.
    ApplicationAboutClose,
    /// Seed disconnected SSH views and public command fixtures without opening any transport.
    HistoryFixture {
        entries: Vec<HistoryEntry>,
    },
    HistoryQuery {
        query: String,
    },
    HistorySelect {
        index: usize,
        #[serde(default)]
        shift: bool,
        #[serde(default)]
        additive: bool,
    },
    /// Render isolated transfer records in the real dialog without a transport.
    TransferFixture {
        confirm_upload: bool,
        #[serde(default)]
        active: bool,
        #[serde(default)]
        failed: bool,
        #[serde(default)]
        download: bool,
        #[serde(default)]
        progress: Option<u32>,
        #[serde(default)]
        long_path: bool,
        #[serde(default)]
        count: Option<usize>,
        #[serde(default)]
        unicode: bool,
    },
    TransferSelect {
        index: usize,
    },
    /// Invoke the same current-session removal path as the footer button.
    RemoveTransferRecords,
    ReviewStopTransfers,
    ConfirmStopTransfers,
    TransferBackground,
    HistoryScroll {
        y: f32,
    },
    ConnectionSelect {
        index: usize,
        #[serde(default)]
        shift: bool,
        #[serde(default)]
        additive: bool,
    },
    ConnectionScroll {
        y: f32,
    },
    Draw,
    /// Offline rendering fixture; only available inside the explicitly isolated debug driver.
    OverviewFixture {
        samples: Vec<crate::monitor::Sample>,
    },
    /// Isolated, bounded port rows. Does not open a transport or enable sampling.
    PortsFixture {
        ports: Vec<crate::monitor::Port>,
        #[serde(default)]
        error: Option<String>,
        #[serde(default)]
        system_error: Option<String>,
        #[serde(default)]
        refresh_error: Option<String>,
    },
    PortsLoading {
        initial: bool,
    },
    PortsQuery {
        query: String,
    },
    PortsProtocol {
        protocol: String,
    },
    PortsSort {
        sort: String,
    },
    PortsToggle {
        index: usize,
    },
    PortsCopy {
        index: usize,
        number: bool,
    },
    PortsScroll {
        y: f32,
    },
    PortsDisconnect,
    Keystroke {
        key: String,
    },
    Paste {
        text: String,
    },
    Compose {
        text: String,
        cursor: usize,
    },
    CommitComposition {
        text: String,
    },
    CancelComposition,
    CursorClick {
        column: usize,
        row: usize,
    },
    ScrollTerminal {
        lines: i32,
    },
    SelectTerminal {
        start_column: usize,
        start_row: usize,
        end_column: usize,
        end_row: usize,
    },
    FocusPane {
        index: usize,
    },
    ClosePane {
        index: usize,
    },
    PanelWidth {
        width: Option<f32>,
    },
    HideTool,
    ReopenTool,
    SystemPage {
        page: SystemPage,
    },
    ResourceDetails {
        kind: String,
    },
    /// Focus a fixture resource row before dispatching a public GPUI keyboard event.
    FocusResource {
        kind: String,
    },
    WindowControl {
        command: crate::window_layout::Command,
    },
    WindowControls,
    WindowFields {
        width: String,
        height: String,
    },
    RefreshMonitor,
    Reconnect,
    OpenSaved {
        id: Id,
        #[serde(default)]
        new_instance: bool,
        #[serde(default)]
        background: bool,
    },
    FixtureCredentials {
        file: String,
        #[serde(default)]
        remember: Option<bool>,
    },
    SubmitCredentials,
    Sidebar,
    Settings,
    SshForm,
    /// Open the production font picker from the settings modal.
    OpenFontPicker {
        terminal: bool,
    },
    /// Apply a family through the production font-row completion path.
    SelectFont {
        family: String,
    },
    /// Open the production system-tools modal from the active page.
    OpenSystemTools {
        page: SystemPage,
    },
    /// Open a details modal with a non-actionable fixture process while the
    /// system-tools modal is active; no signal can be sent by this fixture.
    OpenProcessDetails,
    /// Synthetic original-instance detail view, never backed by a transport.
    ProcessFixture {
        mode: String,
        #[serde(default)]
        long: bool,
    },
    ProcessSampleFixture {
        mode: String,
    },
    ProcessReply {
        #[serde(default)]
        stale: bool,
        #[serde(default)]
        error: bool,
        #[serde(default)]
        long: bool,
    },
    ProcessRefresh,
    ProcessAttemptFixture {
        phase: String,
    },
    ProcessConfirmFixture {
        #[serde(default)]
        force: bool,
    },
    ProcessConfirmSubmitFixture,
    ProcessListFixture {
        count: usize,
    },
    ProcessListState {
        query: String,
        y: f32,
    },
    ReopenProcessPreview,
    /// Use the same credentials-toolbar edit path as the native button.
    EditCredentialConnection,
    /// Create an isolated SSH credential prompt without contacting a host.
    CredentialFixture,
    Dismiss,
    /// Route cancellation through the production confirm-aware close handler.
    CancelModal,
    Local {
        /// Explicit shell for isolated cross-platform PTY acceptance.
        #[serde(default)]
        shell: Option<String>,
    },
    Split {
        vertical: bool,
    },
    Theme {
        night: bool,
    },
    Language {
        english: bool,
    },
    FontSizes {
        ui: f32,
        terminal: f32,
    },
    Resize {
        width: f32,
        height: f32,
    },
    Type {
        text: String,
    },
    Tool {
        tool: Tool,
    },
    Profile {
        profile: Profile,
    },
    /// Filter the isolated connection library without sending input to a terminal.
    ConnectionQuery {
        query: String,
    },
    /// Open a saved loopback fixture through the production clone action, without connecting.
    CloneProfile {
        id: Id,
    },
    SubmitProfile {
        connect: bool,
    },
    Trust,
    Navigate {
        path: String,
    },
    /// Type into the SSH file panel's path input and focus it; submitting the
    /// entry still goes through the real keyboard dispatcher (Keystroke action)
    /// so the Enter pipeline keeps its production routing.
    FilePathInput {
        value: String,
    },
    /// Toggle one tree node's expansion (drives the virtualized tree rows).
    TreeToggle {
        path: String,
    },
    Open {
        path: String,
    },
    FileRow {
        session: Id,
        attempt: Id,
        request: Id,
        path: String,
        #[serde(default)]
        shift: bool,
        #[serde(default)]
        additive: bool,
        #[serde(default)]
        double: bool,
    },
    ToggleHiddenFiles,
    ReviewFileDelete,
    StageTransfer {
        upload: bool,
        local_paths: Vec<String>,
    },
    ConfirmTransfers {
        #[serde(default)]
        overwrite: bool,
    },
    CancelTransfer {
        id: Id,
    },
    RetryTransfer {
        id: Id,
    },
    Edit {
        text: String,
    },
    Save,
    Encoding {
        encoding: crate::encoding::Encoding,
        file: bool,
    },
    Tab {
        index: usize,
    },
    ScrollTabs {
        x: f32,
    },
    CloseTab {
        index: usize,
    },
    Import {
        text: String,
    },
    ConfirmImport,
    CloseDocument,
    ScrollModal {
        y: f32,
    },
    DiscardClose,
    Quit,
}

pub(super) struct Controller {
    directory: std::path::PathBuf,
    sequence: u64,
    last_snapshot: Instant,
    pub(super) header_controls: HashMap<&'static str, Bounds<Pixels>>,
    /// Painted connection row bounds keyed by profile UUID; uniform_list only
    /// reports the rows it actually laid out, so entries follow visibility.
    pub(super) connection_rows: HashMap<u128, Bounds<Pixels>>,
    pub(super) connection_thumb_bounds: Option<Bounds<Pixels>>,
    pub(super) connection_scrollbar_bounds: Option<Bounds<Pixels>>,
    pub(super) connection_add_bounds: Option<Bounds<Pixels>>,
    pub(super) modal_bounds: Option<Bounds<Pixels>>,
    pub(super) dialog_frame_bounds: Option<Bounds<Pixels>>,
    pub(super) transfer_footer_bounds: Option<Bounds<Pixels>>,
    pub(super) process_footer_bounds: Option<Bounds<Pixels>>,
    pub(super) process_geometry: HashMap<&'static str, Bounds<Pixels>>,
    pub(super) transfer_geometry: HashMap<&'static str, Bounds<Pixels>>,
    remote_root: Option<String>,
    pub(super) overview_bounds: HashMap<&'static str, Bounds<Pixels>>,
    pub(super) overview_revision: u64,
    pub(super) network_plot_bounds: Option<Bounds<Pixels>>,
    pub(super) files_input_bounds: Option<Bounds<Pixels>>,
    pub(super) files_toolbar_bounds: Option<Bounds<Pixels>>,
    overview_fixture: bool,
    pub(super) port_geometry: HashMap<&'static str, Bounds<Pixels>>,
    pub(super) port_revision: u64,
    pub(super) ports_fixture_owner: Option<Owner>,
    pub(super) process_fixture_owner: Option<Owner>,
    pub(super) pointer_events: Vec<(String, [f32; 2])>,
    pub(super) port_copy_result: Option<String>,
}
impl Controller {
    /// Require an explicit control path and an isolated database beneath the same QA directory.
    pub fn start(backend: &Backend) -> Option<Self> {
        let path = std::path::PathBuf::from(std::env::var_os("MANTASH_QA_CONTROL")?);
        let directory = path.parent()?.to_path_buf();
        if !path.is_absolute()
            || crate::platform::data_override().is_none()
            || !backend.data_directory.starts_with(&directory)
        {
            return None;
        }
        // Do not replay the last mutation (especially Quit/Type) when the test app restarts.
        let initial_sequence = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Request>(&bytes).ok())
            .map_or(0, |r| r.sequence);
        let events = backend.events.clone();
        backend.runtime.spawn(async move {
            let mut sequence = initial_sequence;
            loop {
                tokio::time::sleep(Duration::from_millis(100)).await;
                if let Ok(bytes) = tokio::fs::read(&path).await {
                    if let Ok(request) = serde_json::from_slice::<Request>(&bytes) {
                        if request.sequence > sequence {
                            sequence = request.sequence;
                            if events.send(Event::Qa(request)).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            }
        });
        Some(Self {
            directory,
            sequence: initial_sequence,
            last_snapshot: Instant::now() - Duration::from_secs(1),
            connection_scrollbar_bounds: None,
            connection_thumb_bounds: None,
            connection_add_bounds: None,
            modal_bounds: None,
            dialog_frame_bounds: None,
            transfer_footer_bounds: None,
            process_footer_bounds: None,
            process_geometry: HashMap::new(),
            header_controls: HashMap::new(),
            transfer_geometry: HashMap::new(),
            connection_rows: HashMap::new(),
            remote_root: std::env::var("MANTASH_QA_REMOTE_ROOT").ok(),
            overview_bounds: HashMap::new(),
            overview_revision: 0,
            network_plot_bounds: None,
            files_input_bounds: None,
            files_toolbar_bounds: None,
            overview_fixture: false,
            port_geometry: HashMap::new(),
            port_revision: 0,
            port_copy_result: None,
            ports_fixture_owner: None,
            process_fixture_owner: None,
            pointer_events: Vec::new(),
        })
    }
}

impl Workbench {
    /// Stand in for the OS path picker only; use the real confirmation and transfer service.
    fn qa_stage_transfers(
        &mut self,
        upload: bool,
        local_paths: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(qa) = &self.qa else {
            return;
        };
        let Some(remote_root) = &qa.remote_root else {
            return;
        };
        let Some(pane) = self.active_pane() else {
            return;
        };
        let SessionSpec::Ssh { profile, .. } = &pane.spec else {
            return;
        };
        if !matches!(profile.host.as_str(), "127.0.0.1" | "::1" | "localhost")
            || pane.state != ConnectionState::Connected
            || pane.files.loading
        {
            return;
        }
        let qa_root = match qa.directory.canonicalize() {
            Ok(p) => p,
            Err(_) => return,
        };
        let local_valid = |path: &str| {
            let path = std::path::Path::new(path);
            if path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return false;
            }
            path.canonicalize()
                .or_else(|_| path.parent().unwrap_or(path).canonicalize())
                .is_ok_and(|p| p.starts_with(&qa_root))
        };
        let remote_valid = |path: &str| {
            path.starts_with('/')
                && !path.split('/').any(|s| s == "..")
                && (path == remote_root
                    || path.starts_with(&format!("{}/", remote_root.trim_end_matches('/'))))
        };
        if local_paths.is_empty() || !local_paths.iter().all(|p| local_valid(p)) {
            return;
        }
        let owner = pane.owner;
        let mut records = Vec::new();
        if upload && remote_valid(&pane.files.path) {
            for local in local_paths {
                if let Some(name) = std::path::Path::new(&local)
                    .file_name()
                    .and_then(|n| n.to_str())
                {
                    if let Ok(remote) = crate::files::join(&pane.files.path, name) {
                        records.push(Self::transfer_record(
                            owner,
                            profile.clone(),
                            true,
                            local,
                            remote,
                        ));
                    }
                }
            }
        } else if !upload && local_paths.len() == 1 {
            for entry in &pane.files.entries {
                if pane.files.selected.contains(&entry.path) && remote_valid(&entry.path) {
                    let local = std::path::Path::new(&local_paths[0]).join(&entry.name);
                    records.push(Self::transfer_record(
                        owner,
                        profile.clone(),
                        false,
                        local.to_string_lossy().into_owned(),
                        entry.path.clone(),
                    ));
                }
            }
        }
        if !records.is_empty() {
            self.show_transfer_confirmation(owner, records, window, cx);
        }
    }
    /// Export logical coordinates measured during native prepaint, not inferred CSS widths.
    pub(super) fn qa_bounds(bounds: Bounds<Pixels>) -> serde_json::Value {
        serde_json::json!({
            "x":f32::from(bounds.origin.x), "y":f32::from(bounds.origin.y),
            "width":f32::from(bounds.size.width), "height":f32::from(bounds.size.height),
        })
    }
    pub(super) fn qa_request(
        &mut self,
        request: Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.qa.is_none() {
            return;
        }
        if let Some(qa) = &mut self.qa {
            qa.sequence = request.sequence;
            qa.last_snapshot = Instant::now() - Duration::from_secs(1);
        }
        match request.action {
            Action::Snapshot => {}
            Action::CheckUpdates { manual } => {
                if manual {
                    self.manual_check_updates(window, cx);
                } else {
                    self.check_updates(window, cx);
                }
            }
            Action::PortsFixture {
                ports,
                error,
                system_error,
                refresh_error,
            } => {
                if ports.len() > 96
                    || ports.iter().any(|port| {
                        [
                            &port.protocol,
                            &port.state,
                            &port.local,
                            &port.peer,
                            &port.process,
                        ]
                        .iter()
                        .any(|field| field.len() > 1024)
                    })
                {
                    self.notice = Some("Invalid ports rendering fixture".into());
                    return;
                }
                let mut sample = crate::monitor::Sample {
                    timestamp: chrono::Utc::now().timestamp(),
                    system: "Linux".into(),
                    ports,
                    ..Default::default()
                };
                if let Some(error) = error {
                    sample.errors.insert("ports".into(), error);
                }
                if let Some(error) = system_error {
                    sample.errors.insert("system".into(), error);
                }
                let existing = self
                    .qa
                    .as_ref()
                    .and_then(|qa| qa.ports_fixture_owner)
                    .filter(|owner| self.pane(*owner).is_some());
                let owner = if let Some(owner) = existing {
                    if let Some(pane) = self.pane_mut(owner) {
                        pane.state = ConnectionState::Connected;
                        pane.monitor = Some(sample);
                        pane.monitor_request = None;
                        pane.monitor_error = refresh_error;
                        pane.port_expanded.clear();
                    }
                    owner
                } else {
                    let profile = Profile {
                        id: Id::new_v4(),
                        name: "Ports fixture (QA)".into(),
                        host: "127.0.0.1".into(),
                        port: 1,
                        username: "qa".into(),
                    };
                    let mut pane = self.create_pane(
                        SessionSpec::Ssh {
                            profile,
                            encoding: crate::encoding::Encoding::Utf8,
                        },
                        false,
                        window,
                        cx,
                    );
                    pane.state = ConnectionState::Connected;
                    pane.monitor = Some(sample);
                    pane.monitor_error = refresh_error;
                    let owner = pane.owner;
                    self.tabs.push(Tab {
                        id: Id::new_v4(),
                        layout: PaneLayout::single(owner.session),
                        panes: vec![pane],
                        active: 0,
                        scroll: ScrollHandle::new(),
                    });
                    self.active = self.tabs.len() - 1;
                    if let Some(qa) = &mut self.qa {
                        qa.ports_fixture_owner = Some(owner);
                    }
                    owner
                };
                if !matches!(&self.modal, Some(Modal::SystemTools { owner: current, page: SystemPage::Ports }) if *current == owner)
                {
                    self.show_modal(
                        Modal::SystemTools {
                            owner,
                            page: SystemPage::Ports,
                        },
                        window,
                        cx,
                    );
                }
                cx.notify();
            }
            Action::PortsLoading { initial } => {
                if let Some(owner) = self.qa.as_ref().and_then(|qa| qa.ports_fixture_owner) {
                    if matches!(&self.modal, Some(Modal::SystemTools { owner: current, page: SystemPage::Ports }) if *current == owner)
                    {
                        if let Some(pane) = self.pane_mut(owner) {
                            pane.monitor_request = Some(Id::new_v4());
                            pane.monitor_error = None;
                            if initial {
                                pane.monitor = None;
                                pane.port_expanded.clear();
                            }
                            cx.notify();
                        }
                    }
                }
            }
            Action::PortsQuery { query } => {
                if let Some(owner) = self.qa.as_ref().and_then(|qa| qa.ports_fixture_owner) {
                    if let Some(pane) = self.pane(owner) {
                        pane.port_filter
                            .update(cx, |input, cx| input.set_value(query, window, cx));
                    }
                    self.prune_port_expansion(owner, cx);
                }
            }
            Action::PortsProtocol { protocol } => {
                let filter = match protocol.as_str() {
                    "all" => Some(ProtocolFilter::All),
                    "tcp" => Some(ProtocolFilter::Tcp),
                    "udp" => Some(ProtocolFilter::Udp),
                    _ => None,
                };
                if let (Some(owner), Some(filter)) = (
                    self.qa.as_ref().and_then(|qa| qa.ports_fixture_owner),
                    filter,
                ) {
                    if let Some(pane) = self.pane_mut(owner) {
                        pane.port_protocol = filter;
                    }
                    self.prune_port_expansion(owner, cx);
                }
            }
            Action::PortsSort { sort } => {
                let order = match sort.as_str() {
                    "ascending" => Some(PortSort::Ascending),
                    "descending" => Some(PortSort::Descending),
                    "protocol" => Some(PortSort::Protocol),
                    _ => None,
                };
                if let (Some(owner), Some(order)) = (
                    self.qa.as_ref().and_then(|qa| qa.ports_fixture_owner),
                    order,
                ) {
                    if let Some(pane) = self.pane_mut(owner) {
                        pane.port_sort = order;
                    }
                }
            }
            Action::PortsToggle { index } => {
                if let Some(owner) = self.qa.as_ref().and_then(|qa| qa.ports_fixture_owner) {
                    let key = self.pane(owner).and_then(|pane| {
                        pane.monitor.as_ref().and_then(|sample| {
                            crate::port_view::visible(
                                &sample.ports,
                                &pane.port_filter.read(cx).value(),
                                pane.port_protocol,
                                pane.port_sort,
                            )
                            .get(index)
                            .map(|row| row.key.clone())
                        })
                    });
                    if let Some(key) = key {
                        self.toggle_port_detail(owner, key, cx);
                    }
                }
            }
            Action::PortsCopy { index, number } => {
                if let Some(owner) = self.qa.as_ref().and_then(|qa| qa.ports_fixture_owner) {
                    let key = self.pane(owner).and_then(|pane| {
                        pane.monitor.as_ref().and_then(|sample| {
                            crate::port_view::visible(
                                &sample.ports,
                                &pane.port_filter.read(cx).value(),
                                pane.port_protocol,
                                pane.port_sort,
                            )
                            .get(index)
                            .map(|row| row.key.clone())
                        })
                    });
                    let copied =
                        key.is_some_and(|key| self.copy_port_value(owner, &key, number, cx));
                    if let Some(qa) = &mut self.qa {
                        qa.port_copy_result = copied
                            .then(|| cx.read_from_clipboard().and_then(|item| item.text()))
                            .flatten()
                            .map(|text| text.to_string());
                    }
                }
            }
            Action::PortsScroll { y } => {
                if let Some(owner) = self.qa.as_ref().and_then(|qa| qa.ports_fixture_owner) {
                    if let Some(pane) = self.pane(owner) {
                        pane.monitor_scroll[2].set_offset(point(px(0.), px(y)));
                    }
                }
            }
            Action::PortsDisconnect => {
                if let Some(owner) = self.qa.as_ref().and_then(|qa| qa.ports_fixture_owner) {
                    if let Some(pane) = self.pane_mut(owner) {
                        pane.state = ConnectionState::Disconnected;
                        pane.monitor = None;
                        pane.monitor_error = None;
                        pane.port_expanded.clear();
                    }
                }
            }
            Action::OverviewFixture { samples } => {
                // Build a disconnected test-only view. It never opens a transport or enables signals.
                if samples.is_empty()
                    || samples.len() > 61
                    || samples.iter().any(|sample| {
                        sample.system != "Linux"
                            || sample.cpu.len() > 129
                            || sample.disks.len() > 64
                            || sample.network.len() > 64
                            || !sample.processes.is_empty()
                            || !sample.ports.is_empty()
                    })
                {
                    self.notice = Some("Invalid overview rendering fixture".into());
                    return;
                }
                let profile = Profile {
                    id: Id::new_v4(),
                    name: "Overview layout fixture (QA)".into(),
                    host: "127.0.0.1".into(),
                    port: 1,
                    username: "layout-fixture".into(),
                };
                let mut pane = self.create_pane(
                    SessionSpec::Ssh {
                        profile,
                        encoding: crate::encoding::Encoding::Utf8,
                    },
                    false,
                    window,
                    cx,
                );
                for sample in &samples {
                    pane.monitor_history.push(sample);
                }
                pane.monitor = samples.last().cloned();
                pane.tool = Some(Tool::System);
                pane.last_tool = Tool::System;
                pane.monitor_page = SystemPage::Overview;
                self.tabs.push(Tab {
                    id: Id::new_v4(),
                    layout: PaneLayout::single(pane.owner.session),
                    panes: vec![pane],
                    active: 0,
                    scroll: ScrollHandle::new(),
                });
                self.active = self.tabs.len() - 1;
                if let Some(qa) = &mut self.qa {
                    qa.overview_fixture = true;
                    qa.overview_bounds.clear();
                }
                cx.notify();
            }
            Action::ApplicationAboutMenu => {
                #[cfg(target_os = "macos")]
                match window_native::prepare_about_menu(self.t("about_mantash")) {
                    Ok(menu) => cx.spawn(async move |_, _| menu.activate()).detach(),
                    Err(error) => self.notice = Some(error.to_string()),
                }
            }
            Action::ApplicationAboutClose => {
                #[cfg(target_os = "macos")]
                cx.spawn(async move |_, _| {
                    if let Err(error) = window_native::close_about_panel() {
                        eprintln!("About QA close failed: {error}");
                    }
                })
                .detach();
            }
            Action::WindowMenuProbe => {
                #[cfg(target_os = "macos")]
                if let Ok(menu) = window_native::prepare_system_menu(window) {
                    cx.spawn(async move |_, _| menu.probe()).detach();
                }
            }
            Action::Draw => {
                // Render a dirty hidden view without dispatching any input to its Shell/editor.
                window.defer(cx, |window, cx| window.draw(cx).clear());
            }
            Action::PointerGesture {
                points,
                cancel,
                click_count,
            } =>
            {
                #[cfg(target_os = "macos")]
                match window_native::prepare_test_gesture(window, &points, cancel, click_count) {
                    Ok(gesture) => cx.spawn(async move |_, _| gesture.post()).detach(),
                    Err(error) => self.notice = Some(error.to_string()),
                }
            }

            Action::HistoryFixture { entries } => {
                if entries.len() > 100 {
                    return;
                }
                let mut profiles = entries
                    .iter()
                    .filter_map(|entry| {
                        entry
                            .scope
                            .strip_prefix("ssh:")
                            .and_then(|value| Id::parse_str(value).ok())
                    })
                    .collect::<Vec<_>>();
                profiles.sort();
                profiles.dedup();
                if profiles.len() != 2 {
                    return;
                }
                self.history = entries;
                for (index, id) in profiles.into_iter().enumerate() {
                    let profile = Profile {
                        id,
                        name: format!("History fixture {}", index + 1),
                        host: "127.0.0.1".into(),
                        port: 1,
                        username: "history-qa".into(),
                    };
                    self.profiles.push(profile.clone());
                    let pane = self.create_pane(
                        SessionSpec::Ssh {
                            profile,
                            encoding: crate::encoding::Encoding::Utf8,
                        },
                        false,
                        window,
                        cx,
                    );
                    self.tabs.push(Tab {
                        id: Id::new_v4(),
                        layout: PaneLayout::single(pane.owner.session),
                        panes: vec![pane],
                        active: 0,
                        scroll: ScrollHandle::new(),
                    });
                }
                self.active = self.tabs.len() - 1;
                self.show_modal(Modal::LocalHistory, window, cx);
            }
            Action::HistoryQuery { query } => {
                if let Some(scope) = self.active_pane().map(|pane| pane.spec.history_scope()) {
                    let search = self.history_views[scope.index()].search.clone();
                    search.update(cx, |input, cx| input.set_value(query, window, cx));
                    search.focus_handle(cx).focus(window);
                }
            }
            Action::HistoryScroll { y } => {
                if let Some(scope) = self.active_pane().map(|pane| pane.spec.history_scope()) {
                    self.history_views[scope.index()]
                        .scroll
                        .set_offset(point(px(0.), px(y)));
                }
            }
            Action::HistorySelect {
                index,
                shift,
                additive,
            } => {
                if let Some(scope) = self.active_pane().map(|pane| pane.spec.history_scope()) {
                    let visible = self
                        .visible_history(scope, cx)
                        .iter()
                        .map(|entry| entry.id)
                        .collect::<Vec<_>>();
                    if let Some(id) = visible.get(index).copied() {
                        let view = &mut self.history_views[scope.index()];
                        update_visible_selection(
                            &visible,
                            &mut view.selected,
                            &mut view.anchor,
                            id,
                            shift,
                            additive,
                        );
                    }
                }
            }
            Action::TransferFixture {
                confirm_upload,
                active,
                download,
                progress,
                long_path,
                failed,
                count,
                unicode,
            } => {
                let Some((owner, profile)) = self.active_pane().and_then(|pane| {
                    if let SessionSpec::Ssh { profile, .. } = &pane.spec {
                        Some((pane.owner, profile.clone()))
                    } else {
                        None
                    }
                }) else {
                    return;
                };
                let long_name = if unicode {
                    "中文长文件名".repeat(36)
                } else {
                    "longfilename".repeat(32)
                };
                let records = (0..count.unwrap_or(3).clamp(1, 40))
                    .map(|index| crate::model::TransferRecord {
                        id: Id::new_v4(),
                        profile: profile.clone(),
                        upload: !download,
                        local: if long_path && index == 0 {
                            format!("/qa/{long_name}.txt")
                        } else {
                            format!("/qa/source-{index}")
                        },
                        remote: if long_path && index == 0 {
                            format!("/qa/target-{index}/{long_name}.txt")
                        } else {
                            format!("/qa/target-{index}")
                        },
                        session: Some(owner.session),
                        attempt: Some(owner.attempt),
                        state: if index == 0 && failed {
                            crate::model::TransferState::Failed
                        } else if active && index == 0 {
                            crate::model::TransferState::Running
                        } else {
                            crate::model::TransferState::Completed
                        },
                        bytes: if active && index == 0 {
                            u64::from(progress.unwrap_or(0).min(100))
                        } else {
                            0
                        },
                        total: if active && index == 0 {
                            progress.map(|_| 100)
                        } else {
                            None
                        },
                        error: (failed && index == 0).then(|| "QA transfer failure".into()),
                        timestamp: 0,
                    })
                    .collect::<Vec<_>>();
                self.transfers = records.clone();
                self.transfer_batches.clear();
                if confirm_upload {
                    let batch = Id::new_v4();
                    if active {
                        if let Some(pane) = self.pane(owner) {
                            self.transfer_batches.insert(
                                batch,
                                super::TransferBatch {
                                    owner,
                                    ids: records.iter().map(|record| record.id).collect(),
                                    file_path: pane.files.path.clone(),
                                    file_request: pane.files.request,
                                },
                            );
                        }
                    }
                    self.show_modal(
                        Modal::Transfer {
                            owner,
                            batch,
                            records,
                            overwrite: false,
                            review_origin: None,
                            phase: if active {
                                super::dialogs::TransferPhase::Running
                            } else if failed {
                                super::dialogs::TransferPhase::Result
                            } else {
                                super::dialogs::TransferPhase::Review
                            },
                        },
                        window,
                        cx,
                    );
                } else {
                    self.show_modal(Modal::Transfers { owner }, window, cx);
                }
            }
            Action::TransferSelect { index } => {
                if let Some(Modal::Transfers { owner }) = &self.modal {
                    let id = self
                        .transfers
                        .iter()
                        .rev()
                        .filter(|task| task.session == Some(owner.session))
                        .nth(index)
                        .map(|task| task.id);
                    if let Some(id) = id {
                        self.transfer_selected.clear();
                        self.transfer_selected.insert(id);
                        self.transfer_anchor = Some(id);
                        cx.notify();
                    }
                }
            }
            Action::RemoveTransferRecords => {
                if let Some(Modal::Transfers { owner }) = &self.modal {
                    self.remove_transfer_records(*owner, cx);
                }
            }
            Action::ConnectionSelect {
                index,
                shift,
                additive,
            } => {
                if matches!(self.modal, Some(Modal::Connections)) {
                    let visible = self
                        .visible_connections(cx)
                        .iter()
                        .map(|profile| profile.id)
                        .collect::<Vec<_>>();
                    if let Some(id) = visible.get(index).copied() {
                        update_visible_selection(
                            &visible,
                            &mut self.connection_multi,
                            &mut self.connection_anchor,
                            id,
                            shift,
                            additive,
                        );
                        self.connection_selected =
                            self.connection_multi.contains(&id).then_some(id);
                    }
                }
            }
            Action::ConnectionScroll { y } => {
                if matches!(self.modal, Some(Modal::Connections)) {
                    self.connection_scroll
                        .0
                        .borrow()
                        .base_handle
                        .set_offset(point(px(0.), px(y)));
                }
            }
            Action::Keystroke { key } => match Keystroke::parse(&key) {
                Ok(keystroke) => {
                    // Use the public keyboard dispatcher so native keymap precedence and
                    // focus routing are tested. Release Workbench's update borrow first.
                    window.defer(cx, move |window, cx| {
                        window.dispatch_keystroke(keystroke, cx);
                    });
                }
                Err(error) => self.notice = Some(format!("Invalid QA keystroke: {error}")),
            },
            Action::Paste { text } => {
                if let Some(view) = self.active_pane().and_then(|p| p.terminal.clone()) {
                    view.update(cx, |t, cx| t.paste_text(&text, cx));
                }
            }
            Action::Compose { text, cursor } => {
                if cursor <= text.encode_utf16().count() {
                    if let Some(view) = self.active_pane().and_then(|p| p.terminal.clone()) {
                        view.update(cx, |t, cx| {
                            t.replace_and_mark_text_in_range(
                                None,
                                &text,
                                Some(cursor..cursor),
                                window,
                                cx,
                            )
                        });
                    }
                }
            }
            Action::CommitComposition { text } => {
                if let Some(view) = self.active_pane().and_then(|p| p.terminal.clone()) {
                    view.update(cx, |t, cx| t.replace_text_in_range(None, &text, window, cx));
                }
            }
            Action::CancelComposition => {
                if let Some(view) = self.active_pane().and_then(|p| p.terminal.clone()) {
                    view.update(cx, |t, cx| t.unmark_text(window, cx));
                }
            }

            Action::CursorClick { column, row } => {
                if let Some(terminal) = self.active_pane().and_then(|p| p.terminal.clone()) {
                    terminal.update(cx, |t, cx| t.click_cell(column, row, window, cx));
                }
            }

            Action::ScrollTerminal { lines } => {
                if let Some(terminal) = self.active_pane().and_then(|p| p.terminal.clone()) {
                    terminal.update(cx, |t, cx| {
                        t.session.terminal.lock().scroll(lines);
                        cx.notify();
                    });
                }
            }
            Action::SelectTerminal {
                start_column,
                start_row,
                end_column,
                end_row,
            } => {
                if let Some(terminal) = self.active_pane().and_then(|p| p.terminal.clone()) {
                    terminal.update(cx, |t, cx| {
                        let mut buffer = t.session.terminal.lock();
                        buffer.select_start(start_column, start_row, false);
                        buffer.select_to(end_column, end_row);
                        cx.notify();
                    });
                }
            }

            Action::FocusPane { index } => {
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    if index < tab.panes.len() {
                        tab.active = index;
                    }
                }
                self.focus_active(window, cx);
                self.changed(cx);
            }
            Action::ClosePane { index } => {
                if let Some(pane) = self.tabs.get(self.active).and_then(|t| t.panes.get(index)) {
                    self.request_close(dialogs::CloseTarget::Pane(pane.owner), window, cx);
                }
            }
            Action::PanelWidth { width } => {
                self.prefs.tool_preferred_width = width;
                self.changed(cx);
            }
            Action::HideTool => self.set_active_tool(None, cx),
            Action::ReopenTool => self.reopen_tool(cx),
            Action::SystemPage { page } => {
                // Use the same transition as the tool button, including last-tool persistence.
                self.set_active_tool(Some(Tool::System), cx);
                if let Some(owner) = self.active_owner() {
                    if let Some(p) = self.pane_mut(owner) {
                        p.monitor_page = page;
                    }
                    self.changed(cx);
                }
            }
            Action::Reconnect => {
                if let Some(owner) = self.active_owner() {
                    self.reconnect(owner, String::new(), false, window, cx);
                }
            }
            Action::OpenSaved {
                id,
                new_instance,
                background,
            } => {
                if let Some(profile) = self.profiles.iter().find(|p| p.id == id).cloned() {
                    let previous = self.active;
                    self.open_saved(profile, new_instance, window, cx);
                    // Switch within the same UI turn so a fast loopback response cannot win
                    // the test's race before the next separate QA command is acknowledged.
                    if background && previous < self.tabs.len() {
                        self.active = previous;
                        self.focus_active(window, cx);
                    }
                }
            }
            Action::FixtureCredentials { file, remember: _ } => {
                let permitted = self.qa.as_ref().is_some_and(|qa| {
                    let path = std::path::Path::new(&file);
                    path.canonicalize()
                        .ok()
                        .zip(qa.directory.canonicalize().ok())
                        .is_some_and(|(path, root)| path.starts_with(root))
                });
                let local_prompt = match &self.modal {
                    Some(Modal::Credentials { owner, .. }) => self.pane(*owner).is_some_and(|p| matches!(&p.spec, SessionSpec::Ssh { profile, .. } if matches!(profile.host.as_str(), "127.0.0.1" | "::1" | "localhost"))),
                    _ => false,
                };
                if permitted && local_prompt {
                    // Test fixture file only. Never serialize the secret into snapshots or logs.
                    if let Ok(value) = std::fs::read_to_string(&file) {
                        let value = zeroize::Zeroizing::new(value);
                        if let Some(Modal::Credentials { secret, .. }) = &mut self.modal {
                            secret.update(cx, |s, cx| s.set_value(value.to_string(), window, cx));
                        }
                    }
                }
            }
            Action::SubmitCredentials => self.submit_credentials(window, cx),
            Action::ResourceDetails { kind } => {
                let kind = match kind.as_str() {
                    "cpu" => Some(system::ResourceKind::Cpu),
                    "disk" => Some(system::ResourceKind::Disk),
                    _ => None,
                };
                if let (Some(owner), Some(kind)) = (self.active_owner(), kind) {
                    self.show_resource_details(owner, kind, window, cx);
                }
            }
            Action::FocusResource { kind } => {
                if self.qa.as_ref().is_some_and(|qa| qa.overview_fixture)
                    && self.active_tool() == Some(Tool::System)
                    && self.modal.is_none()
                {
                    if let Some(index) = ["cpu", "disk"].iter().position(|key| *key == kind) {
                        if let Some(pane) = self.active_pane() {
                            pane.monitor_resource_focus[index].focus(window);
                        }
                    }
                }
            }
            Action::WindowControls => self.open_window_controls(window, cx),
            Action::WindowFields { width, height } => {
                if let Some(Modal::WindowControls(form)) = &mut self.modal {
                    form.width
                        .update(cx, |input, cx| input.set_value(width, window, cx));
                    form.height
                        .update(cx, |input, cx| input.set_value(height, window, cx));
                    form.height.focus_handle(cx).focus(window);
                }
            }
            Action::WindowControl { command } => self.arrange_window(command, window, cx),
            Action::RefreshMonitor => {
                if let Some(owner) = self.active_owner() {
                    self.refresh_monitor(owner, cx);
                }
            }
            Action::Sidebar => self.toggle_sidebar(window, cx),
            Action::Settings => self.open_settings(window, cx),
            Action::About => self.show_modal(Modal::About, window, cx),
            Action::SshForm => self.profile_form(None, None, window, cx),
            Action::OpenFontPicker { terminal } => self.open_font_picker(terminal, window, cx),
            Action::SelectFont { family } => self.select_font(family, window, cx),
            Action::OpenSystemTools { page } => {
                if let Some(owner) = self.active_owner() {
                    self.show_modal(Modal::SystemTools { owner, page }, window, cx);
                }
            }
            Action::OpenProcessDetails => {
                let owner = match &self.modal {
                    Some(Modal::SystemTools { owner, .. }) => Some(*owner),
                    _ => self.active_owner(),
                };
                if let Some(owner) = owner {
                    self.show_process_details(
                        owner,
                        crate::monitor::Process {
                            user: "qa".into(),
                            started_at: None,
                            identity: None,
                            pid: 4242,
                            parent: 1,
                            cpu: 0.,
                            memory: 0.,
                            rss: 0,
                            state: "fixture".into(),
                            command: "qa-process".into(),
                        },
                        window,
                        cx,
                    );
                }
            }
            Action::ProcessFixture { mode, long } => {
                self.qa_process_fixture(&mode, long, window, cx)
            }
            Action::ProcessSampleFixture { mode } => {
                self.qa_process_sample(&mode);
                cx.notify();
            }
            Action::ProcessReply { stale, error, long } => {
                self.qa_process_reply(stale, error, long, cx)
            }
            Action::ProcessRefresh => self.refresh_process_details(cx),
            Action::ProcessAttemptFixture { phase } => self.qa_process_attempt(&phase, cx),
            Action::ProcessConfirmFixture { force } => self.qa_process_confirm(force, window, cx),
            Action::ProcessConfirmSubmitFixture => {
                if let Some(Modal::ProcessConfirm {
                    owner,
                    process,
                    action,
                    ..
                }) = &self.modal
                {
                    self.submit_process(*owner, process.clone(), *action, window, cx);
                }
            }
            Action::ProcessListFixture { count } => self.qa_process_list_fixture(count, cx),
            Action::ProcessListState { query, y } => {
                self.qa_process_list_state(query, y, window, cx)
            }
            Action::ReopenProcessPreview => self.qa_reopen_process_preview(window, cx),
            Action::EditCredentialConnection => self.edit_credential_connection(window, cx),
            Action::CredentialFixture => {
                let profile = Profile {
                    id: Id::new_v4(),
                    name: "QA Credential Prompt".into(),
                    host: "127.0.0.1".into(),
                    port: 22,
                    username: "qa".into(),
                };
                let mut pane = self.create_pane(
                    SessionSpec::Ssh {
                        profile,
                        encoding: crate::encoding::Encoding::Utf8,
                    },
                    false,
                    window,
                    cx,
                );
                pane.state = ConnectionState::CredentialsRequired;
                let owner = pane.owner;
                self.tabs.push(Tab {
                    id: Id::new_v4(),
                    layout: PaneLayout::single(owner.session),
                    panes: vec![pane],
                    active: 0,
                    scroll: ScrollHandle::new(),
                });
                self.active = self.tabs.len() - 1;
                let secret = Self::input("", "", true, window, cx);
                let focus = secret.focus_handle(cx);
                let (reply, _receiver) = tokio::sync::oneshot::channel();
                self.show_modal(
                    Modal::Credentials {
                        owner,
                        reason: crate::credentials::PromptReason::Missing,
                        secret,
                        show_secret: false,
                        reply: Some(reply),
                    },
                    window,
                    cx,
                );
                focus.focus(window);
            }
            Action::Dismiss => self.dismiss(window, cx),
            Action::CancelModal => self.cancel_modal(window, cx),
            Action::Local { shell } => {
                if let Some(shell) = shell {
                    let mut spec = self.local_spec();
                    if let SessionSpec::Local { shell: target, .. } = &mut spec {
                        *target = shell;
                    }
                    self.new_local_with_spec(spec, window, cx);
                } else {
                    self.new_local(window, cx);
                }
            }
            Action::Split { vertical } => self.split(
                if vertical {
                    Split::Vertical
                } else {
                    Split::Horizontal
                },
                None,
                window,
                cx,
            ),
            Action::Theme { night } => {
                self.prefs.theme = if night { Theme::Night } else { Theme::Day };
                self.apply_preferences(window, cx);
            }
            Action::Language { english } => {
                self.prefs.language = if english { Language::En } else { Language::Zh };
                self.apply_preferences(window, cx);
            }
            Action::FontSizes { ui, terminal } => {
                self.prefs.ui_size = ui;
                self.prefs.terminal_size = terminal;
                self.apply_preferences(window, cx);
            }
            Action::Resize { width, height } => {
                window.resize(size(px(width.max(960.)), px(height.max(640.))))
            }
            Action::Type { text } => {
                if let Some(view) = self.active_pane().and_then(|p| p.terminal.clone()) {
                    view.update(cx, |t, cx| t.type_text(&text, cx));
                }
            }
            Action::Tool { tool } => {
                if self.active_tool() != Some(tool) {
                    self.set_tool(tool, cx);
                }
            }
            Action::Profile { profile } => {
                if matches!(profile.host.as_str(), "127.0.0.1" | "::1" | "localhost") {
                    self.profile_form(Some(profile), None, window, cx);
                } else {
                    self.notice = Some("QA connections must use loopback".into());
                }
            }
            Action::SubmitProfile { connect } => self.submit_profile(connect, window, cx),
            Action::ConnectionQuery { query } => {
                if matches!(self.modal, Some(Modal::Connections)) {
                    self.connection_search
                        .update(cx, |input, cx| input.set_value(query, window, cx));
                    self.connection_search.focus_handle(cx).focus(window);
                }
            }
            Action::CloneProfile { id } => {
                if let Some(profile) = self
                    .profiles
                    .iter()
                    .find(|profile| {
                        profile.id == id
                            && matches!(profile.host.as_str(), "127.0.0.1" | "::1" | "localhost")
                    })
                    .cloned()
                {
                    self.clone_connection(&profile, window, cx);
                }
            }
            Action::Trust => {
                if let Some(Modal::Trust { host, reply, .. }) = &mut self.modal {
                    if matches!(host.as_str(), "127.0.0.1" | "::1" | "localhost") {
                        if let Some(reply) = reply.take() {
                            let _ = reply.send(true);
                        }
                        self.dismiss(window, cx);
                    }
                }
            }
            Action::Navigate { path } => {
                if let Some(owner) = self.active_owner() {
                    self.navigate(owner, Some(path), cx);
                }
            }
            Action::FilePathInput { value } => {
                if let Some(pane) = self.active_pane() {
                    let input = pane.files.input.clone();
                    input.update(cx, |input, cx| input.set_value(value, window, cx));
                    input.focus_handle(cx).focus(window);
                }
            }
            Action::TreeToggle { path } => {
                if let Some(owner) = self.active_owner() {
                    self.tree_toggle(owner, &path, cx);
                }
            }
            Action::Open { path } => {
                if let Some(owner) = self.active_owner() {
                    self.open_document(owner, path, window, cx);
                }
            }
            Action::FileRow {
                session,
                attempt,
                request,
                path,
                shift,
                additive,
                double,
            } => {
                self.file_row_action(
                    Owner { session, attempt },
                    Some(request),
                    &path,
                    shift,
                    additive,
                    double,
                    window,
                    cx,
                );
            }
            Action::ToggleHiddenFiles => {
                if let Some(owner) = self.active_owner() {
                    self.toggle_hidden_files(owner, cx);
                }
            }
            Action::ReviewFileDelete => {
                if let Some(owner) = self.active_owner() {
                    self.review_file_delete(owner, window, cx);
                }
            }
            Action::StageTransfer {
                upload,
                local_paths,
            } => self.qa_stage_transfers(upload, local_paths, window, cx),
            Action::ConfirmTransfers { overwrite } => {
                if let Some(Modal::Transfer {
                    overwrite: value, ..
                }) = &mut self.modal
                {
                    *value = overwrite;
                }
                self.confirm_transfers(window, cx);
            }
            Action::ReviewStopTransfers => {
                if let Some(Modal::Transfer { owner, batch, .. }) = &self.modal {
                    self.review_stop_transfers(*owner, *batch, window, cx);
                }
            }
            Action::ConfirmStopTransfers => {
                if let Some(Modal::CancelTransfers {
                    owner,
                    batch,
                    records,
                    overwrite,
                    ..
                }) = &self.modal
                {
                    self.stop_transfer_batch(*owner, *batch, records.clone(), *overwrite, cx);
                }
            }
            Action::TransferBackground => {
                if matches!(
                    self.modal,
                    Some(Modal::Transfer {
                        phase: super::dialogs::TransferPhase::Running,
                        ..
                    })
                ) {
                    self.dismiss(window, cx);
                }
            }
            Action::CancelTransfer { id } => self.cancel_transfer(id, cx),
            Action::RetryTransfer { id } => {
                if let Some(record) = self
                    .transfers
                    .iter()
                    .find(|r| r.id == id && !r.state.active())
                    .cloned()
                {
                    self.retry_transfer(record, window, cx);
                }
            }
            Action::Edit { text } => {
                if let Some((owner, id)) = self
                    .active_pane()
                    .and_then(|p| p.active_document.map(|id| (p.owner, id)))
                {
                    if let Some(doc) = self.document_mut(owner, id) {
                        doc.input.update(cx, |s, cx| s.set_value(text, window, cx));
                    }
                }
            }
            Action::Save => {
                if let Some((owner, id)) = self
                    .active_pane()
                    .and_then(|p| p.active_document.map(|id| (p.owner, id)))
                {
                    self.save_document(owner, id, false, cx);
                }
            }
            Action::Encoding { encoding, file } => {
                if let Some(owner) = self.active_owner() {
                    if file {
                        if let Some(p) = self.pane_mut(owner) {
                            p.files.encoding = Some(encoding);
                        }
                    } else if let Some(p) = self.pane_mut(owner) {
                        if let Some(t) = &p.terminal {
                            t.read(cx).session.terminal.lock().set_encoding(encoding);
                        }
                        // Mirror the encoding modal: the live session owns its encoding.
                        if let SessionSpec::Local { encoding: e, .. } = &mut p.spec {
                            *e = encoding;
                        } else if let SessionSpec::Ssh { encoding: e, .. } = &mut p.spec {
                            *e = encoding;
                        }
                    }
                }
            }
            Action::Tab { index } => {
                if index < self.tabs.len() {
                    self.active = index;
                    self.focus_active(window, cx);
                    self.changed(cx);
                }
            }
            // This controls native scroll state; it does not inject OS wheel events.
            Action::ScrollTabs { x } => self.tab_strip.scroll.set_offset(point(px(x), px(0.))),
            Action::CloseTab { index } => {
                if let Some(tab) = self.tabs.get(index) {
                    self.request_close(dialogs::CloseTarget::Tab(tab.id), window, cx);
                }
            }
            Action::Import { text } => match crate::connections::preview(&text, &self.profiles) {
                Ok(preview) => self.show_modal(
                    Modal::Import {
                        preview,
                        replace: false,
                    },
                    window,
                    cx,
                ),
                Err(error) => self.notice = Some(error.to_string()),
            },
            Action::ConfirmImport => {
                if let Some(Modal::Import { preview, replace }) = &self.modal {
                    self.profiles = crate::connections::merge(&self.profiles, preview, *replace);
                    self.backend.save_profiles(self.profiles.clone());
                }
                self.dismiss(window, cx);
            }
            Action::CloseDocument => {
                if let Some((owner, id)) = self
                    .active_pane()
                    .and_then(|p| p.active_document.map(|id| (p.owner, id)))
                {
                    self.request_close(dialogs::CloseTarget::Document(owner, id), window, cx);
                }
            }
            Action::ScrollModal { y } => self.modal_scroll.set_offset(point(px(0.), px(y))),
            Action::DiscardClose => {
                if let Some(Modal::Close(target)) = &self.modal {
                    self.finish_close(target.clone(), window, cx);
                }
            }
            Action::Quit => self.request_close(dialogs::CloseTarget::Window, window, cx),
        }
        cx.notify();
    }
    /// Export only modal navigation state needed to verify parent restoration;
    /// focus handles and confirmation targets are represented by safe labels.
    pub(super) fn qa_modal_navigation(&self, window: &Window, cx: &App) -> serde_json::Value {
        let current_focus = window.focused(cx).map(|focus| format!("{focus:?}"));
        serde_json::json!({
            "parents": self.modal_stack.iter().map(|frame| frame.modal.qa_key()).collect::<Vec<_>>(),
            "confirmation": self.modal.as_ref().is_some_and(Modal::is_confirmation),
            "confirmation_parent": self.modal_confirm_return.as_ref().map(|frame| frame.modal.qa_key()),
            "parent_focuses": self.modal_stack.iter().map(|frame| frame.focused.as_ref().map(|focus| format!("{focus:?}"))).collect::<Vec<_>>(),
            "current_focus": current_focus,
            "focus_within": self.modal.is_some() && window.focused(cx).is_some(),
        })
    }

    pub(super) fn qa_tick(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(qa) = &mut self.qa else {
            return;
        };
        if qa.last_snapshot.elapsed() < Duration::from_millis(250) {
            return;
        }
        qa.last_snapshot = Instant::now();
        let files_input_bounds = qa.files_input_bounds;
        let files_toolbar_bounds = qa.files_toolbar_bounds;
        let path = qa.directory.join("state.json");
        let sequence = qa.sequence;
        let strip = &self.tab_strip.scroll;
        let header = serde_json::json!({
            "bounds":Self::qa_bounds(strip.bounds()),
            "offset_x":f32::from(strip.offset().x),
            "pointer_events":qa.pointer_events,
            "completed_drags":self.tab_strip.completed_drags,
            "drag":self.tab_strip.drag.as_ref().map(|drag|serde_json::json!({"tab":drag.tab,"moved":drag.moved,"slot":drag.slot,"before":drag.before})),
            "max_offset_x":f32::from(strip.max_offset().width),
            "controls":qa.header_controls.iter().map(|(name, bounds)|
                ((*name).to_string(), Self::qa_bounds(*bounds))).collect::<serde_json::Map<_, _>>(),
            "tabs":(0..self.tabs.len()).map(|index|strip.bounds_for_item(index).map(|mut bounds| {
                bounds.origin += strip.offset();
                Self::qa_bounds(bounds)
            })).collect::<Vec<_>>(),
        });
        let modal = self.modal.as_ref().map(Modal::qa_key);
        let modal_stack = self
            .modal_stack
            .iter()
            .map(|frame| frame.modal.qa_key())
            .collect::<Vec<_>>();
        let modal_navigation = self.qa_modal_navigation(window, cx);
        let mut tabs: Vec<_> = self.tabs.iter().map(|tab| serde_json::json!({"id":tab.id,"active_pane":tab.active,"layout":tab.layout,"title":tab.panes.get(tab.active).map(|p|self.pane_label(p)),"panes":tab.panes.iter().map(|pane| {
            let terminal = pane.terminal.as_ref().map(|view| { let metrics = view.read(cx).scroll_metrics(); let buffer = view.read(cx).session.terminal.lock(); let frame = buffer.frame(); let mut rows = vec![String::new(); frame.size.rows]; for cell in frame.cells { if cell.row < rows.len() && !cell.cell.flags.intersects(alacritty_terminal::term::cell::Flags::WIDE_CHAR_SPACER | alacritty_terminal::term::cell::Flags::LEADING_WIDE_CHAR_SPACER) { rows[cell.row].push(cell.cell.c); } }
                serde_json::json!({"geometry":view.read(cx).qa_geometry(),"scroll_metrics":metrics,"display_offset":buffer.term.grid().display_offset(),"preedit":view.read(cx).qa_preedit(),"focused":view.read(cx).focus.is_focused(window),"cursor":frame.cursor.map(|(row,column,_)|serde_json::json!({"row":row,"column":column})),"command_editing":buffer.command_cursor.editing(),"mouse_reporting":buffer.term.mode().intersects(alacritty_terminal::term::TermMode::MOUSE_MODE),"history_lines":buffer.term.grid().history_size(),"columns":frame.size.cols,"rows":frame.size.rows,"text":rows.iter().map(|l|l.trim_end()).collect::<Vec<_>>().join("\n"),"matches":buffer.search_hits.len(),"paint_statistics":view.read(cx).paint_statistics}) });
            serde_json::json!({"owner":pane.owner.session,"attempt":pane.owner.attempt,"output_pending":pane.terminal.as_ref().is_some_and(|t|t.read(cx).session.output_wakeup.pending()),"label":pane.spec.label(),"state":format!("{:?}",pane.state),"directory":pane.directory,"files":pane.files.entries.iter().map(|e|e.name.clone()).collect::<Vec<_>>(),"file_path":pane.files.path,"file_input":pane.files.input.read(cx).value().to_string(),"file_input_focused":pane.files.input.read(cx).focus_handle(cx).is_focused(window),"file_request":pane.files.request,"file_tree_loading":pane.files.tree_request.as_ref().map(|(_, pending)|pending.clone()),"file_tree_rows":pane.files.tree_rows.len(),"file_loading":pane.files.loading,"file_show_hidden":pane.files.show_hidden,"file_list_bounds":Self::qa_bounds(pane.files.scroll.bounds()),"file_input_bounds":files_input_bounds.map(Self::qa_bounds),"file_toolbar_bounds":files_toolbar_bounds.map(Self::qa_bounds),"file_max_y":f32::from(pane.files.scroll.max_offset().height),"file_offset_y":f32::from(pane.files.scroll.offset().y),"file_selection":pane.files.selected.iter().cloned().collect::<Vec<_>>(),"file_anchor":pane.files.selected.anchor(),"file_lead":pane.files.selected.lead(),"file_list_focused":pane.files.focus.is_focused(window),"file_error":pane.files.error,"documents":pane.documents.iter().map(|d|serde_json::json!({"path":d.original.path,"cursor":format!("{:?}",d.input.read(cx).cursor_position()),"focused":d.input.focus_handle(cx).is_focused(window),"dirty":d.dirty,"saving":d.saving,"revision":d.revision,"error":d.error})).collect::<Vec<_>>(),"terminal":terminal,"monitor_error":pane.monitor_error,"monitor":pane.monitor,"tool":pane.tool,"system_page":pane.monitor_page,"encoding":pane.spec.encoding()})
        }).collect::<Vec<_>>() })).collect();
        for (snapshot, tab) in tabs.iter_mut().zip(&self.tabs) {
            snapshot["viewport_max_y"] =
                serde_json::json!(f32::from(tab.scroll.max_offset().height));
            snapshot["viewport_height"] =
                serde_json::json!(f32::from(tab.scroll.bounds().size.height));
        }
        let delete_targets = match &self.modal {
            Some(Modal::DeleteFiles { owner, paths }) => Some(
                serde_json::json!({"session":owner.session,"attempt":owner.attempt,"paths":paths}),
            ),
            _ => None,
        };
        let pending_transfers = match &self.modal {
            Some(Modal::Transfer { records, .. }) => records.clone(),
            _ => Vec::new(),
        };
        let transfer_view = match &self.modal {
            Some(Modal::Transfer {
                batch,
                phase,
                records,
                ..
            }) => Some(serde_json::json!({
                "batch": batch, "phase": format!("{phase:?}").to_lowercase(),
                "active": self.transfer_batches.contains_key(batch),
                "endpoints": records.iter().map(|record| {
                    let (source, target) = super::transfer_paths::endpoints(record);
                    serde_json::json!({"id":record.id,"name":super::transfer_paths::display_name(record),"source":source,"target":target})
                }).collect::<Vec<_>>(),
                "footer_bounds": self.qa.as_ref().and_then(|qa| qa.transfer_footer_bounds.map(Self::qa_bounds)),
                "geometry": self.qa.as_ref().map(|qa| qa.transfer_geometry.iter().map(|(name, bounds)| (*name, Self::qa_bounds(*bounds))).collect::<HashMap<_,_>>()),
            })),
            _ => None,
        };
        let port_view = match &self.modal {
            Some(Modal::SystemTools { owner, page: SystemPage::Ports }) => self.pane(*owner).map(|pane| {
                let query = pane.port_filter.read(cx).value();
                let sample = pane.monitor.as_ref();
                let rows = sample.filter(|s| pane.state == ConnectionState::Connected
                    && !s.errors.contains_key("ports") && !s.errors.contains_key("system"))
                    .map(|sample| crate::port_view::visible(&sample.ports, &query, pane.port_protocol, pane.port_sort))
                    .unwrap_or_default();
                let scroll = &pane.monitor_scroll[2];
                serde_json::json!({
                    "owner":owner.session,"query":query,"protocol":format!("{:?}",pane.port_protocol),
                    "sort":format!("{:?}",pane.port_sort),"connected":pane.state == ConnectionState::Connected,
                    "count":sample.map_or(0, |sample| sample.ports.len()),
                    "refreshing":pane.monitor_request.is_some(),
                    "error":sample.and_then(|sample| sample.errors.get("ports").or_else(|| sample.errors.get("system"))),"refresh_error":pane.monitor_error,
                    "rows":rows.iter().map(|row| serde_json::json!({"local":row.source.local,"address":row.address,
                        "label":row.label,"number":row.number,"protocol":row.source.protocol,"state":row.source.state,
                        "peer":row.source.peer,"process":row.source.process,"expanded":pane.port_expanded.contains(&row.key)})).collect::<Vec<_>>(),
                    "offset_y":f32::from(scroll.offset().y),"max_y":f32::from(scroll.max_offset().height),
                    "max_x":f32::from(scroll.max_offset().width),
                    "bounds":Self::qa_bounds(scroll.bounds()),"copied":self.qa.as_ref().and_then(|qa| qa.port_copy_result.clone()),
                    "geometry":self.qa.as_ref().map(|qa| qa.port_geometry.iter().map(|(key,bounds)| (*key,Self::qa_bounds(*bounds))).collect::<HashMap<_,_>>()),
                    "revision":self.qa.as_ref().map_or(0, |qa| qa.port_revision)
                })
            }),
            _ => None,
        };
        let transfer_stop_targets = match &self.modal {
            Some(Modal::CancelTransfers {
                stop_ids, records, ..
            }) => Some(serde_json::json!({
                "ids":stop_ids,"host":records.first().map(|record| format!("{}:{}",record.profile.host,record.profile.port)),
            })),
            _ => None,
        };
        let credential_prompt = match &self.modal {
            Some(Modal::Credentials { owner, reason, .. }) => Some(
                serde_json::json!({"session":owner.session,"attempt":owner.attempt,"reason":format!("{reason:?}")}),
            ),
            _ => None,
        };
        let overview=self.qa.as_ref().map(|qa|serde_json::json!({"fixture":qa.overview_fixture,"revision":qa.overview_revision,"network_plot":qa.network_plot_bounds.map(Self::qa_bounds),"bounds":qa.overview_bounds.iter().map(|(name,bounds)|(*name,Self::qa_bounds(*bounds))).collect::<HashMap<_,_>>()}));
        let resource_focus = self.active_pane().and_then(|pane| {
            pane.monitor_resource_focus
                .iter()
                .position(|focus| focus.is_focused(window))
                .map(|index| ["cpu", "disk"][index])
        });
        // Read actual laid-out scroll geometry so QA can distinguish a usable viewport from acknowledgement.
        let modal_scroll = serde_json::json!({"bounds": Self::qa_bounds(self.modal_scroll.bounds()),
            "dialog_height": self.resource_modal_height,
            "content_bounds": self.modal_scroll.bounds_for_item(0).map(Self::qa_bounds),
            "max_x": f32::from(self.modal_scroll.max_offset().width),
            "max_y": f32::from(self.modal_scroll.max_offset().height)});
        let resource_details = match &self.modal {
            Some(Modal::ResourceDetails {
                owner,
                timestamp,
                data,
            }) => {
                let rows = match data {
                    system::ResourceSnapshot::Cpu(cpus) => {
                        cpus.iter().map(|cpu| cpu.name.clone()).collect::<Vec<_>>()
                    }
                    system::ResourceSnapshot::Disk(disks) => {
                        disks.iter().map(|disk| disk.mount.clone()).collect()
                    }
                };
                Some(
                    serde_json::json!({"kind":data.kind().key(),"owner":owner.session,"attempt":owner.attempt,"timestamp":timestamp,"rows":rows}),
                )
            }
            _ => None,
        };
        let window_info = window_native::info(window, cx).ok();
        let window_form = match &self.modal {
            Some(Modal::WindowControls(form)) => Some(
                serde_json::json!({"width":form.width.read(cx).value(),"height":form.height.read(cx).value(),"error":form.error}),
            ),
            _ => None,
        };
        // Only public connection identity and laid-out geometry are exported; secrets stay private.
        let connection_scroll_base = self.connection_scroll.0.borrow().base_handle.clone();
        let connection_library = serde_json::json!({
            "selected":self.selected_connection(cx).map(|profile|profile.id),
            "highlighted":self.connection_selected,
            "anchor":self.connection_anchor,
            "multi":self.connection_multi.iter().cloned().collect::<Vec<_>>(),
            "visible":self.visible_connections(cx).iter().map(|profile|profile.id).collect::<Vec<_>>(),
            "query":self.connection_search.read(cx).value(),
            "bounds":Self::qa_bounds(connection_scroll_base.bounds()),
            "scrollbar_right_edge":f32::from(connection_scroll_base.bounds().right()),
            "scrollbar_width":10.0,
            "scrollbar_bounds":self.qa.as_ref().and_then(|qa| qa.connection_scrollbar_bounds.map(Self::qa_bounds)),
            "thumb_bounds":self.qa.as_ref().and_then(|qa| qa.connection_thumb_bounds.map(Self::qa_bounds)),
            "add_bounds":self.qa.as_ref().and_then(|qa| qa.connection_add_bounds.map(Self::qa_bounds)),
            "modal_bounds":self.qa.as_ref().and_then(|qa| qa.modal_bounds.map(Self::qa_bounds)),
            "offset_y":f32::from(connection_scroll_base.offset().y),
            "max_y":f32::from(connection_scroll_base.max_offset().height),
            "rows":self.qa.as_ref().map(|qa|qa.connection_rows.iter().map(|(id,bounds)|serde_json::json!({"id":id.to_string(),"bounds":Self::qa_bounds(*bounds)})).collect::<Vec<_>>()),
            "returning":self.connection_return,
            "form":match &self.modal {Some(Modal::Profile(form))=>Some(form.qa_metadata(cx)),_=>None},
        });
        let history_view = self.active_pane().map(|pane| {
            let scope = pane.spec.history_scope();
            let view = &self.history_views[scope.index()];
            serde_json::json!({"scope":scope,"query":view.search.read(cx).value(),"rows":self.visible_history(scope, cx).iter().map(|entry|entry.id).collect::<Vec<_>>(),
                "selected":view.selected.iter().copied().collect::<Vec<_>>(),"anchor":view.anchor,"modal_bounds":self.qa.as_ref().and_then(|qa| qa.modal_bounds.map(Self::qa_bounds)),"adaptive_height":self.resource_modal_height,"content_marker":self.history_content_marker.get().map(Self::qa_bounds),"scroll_y":f32::from(view.scroll.offset().y),"max_y":f32::from(view.scroll.max_offset().height),"bounds":Self::qa_bounds(view.scroll.bounds())})
        });
        let transfer_selection = serde_json::json!({"selected":self.transfer_selected.iter().copied().collect::<Vec<_>>(),"anchor":self.transfer_anchor});
        #[cfg(target_os = "macos")]
        let application_menu = window_native::application_menu_snapshot();
        #[cfg(not(target_os = "macos"))]
        let application_menu = serde_json::Value::Null;
        #[cfg(target_os = "macos")]
        let about_panel = window_native::about_panel_snapshot();
        #[cfg(not(target_os = "macos"))]
        let about_panel = serde_json::Value::Null;
        #[cfg(target_os = "macos")]
        let system_window_menu = window_native::system_menu_snapshot();
        #[cfg(not(target_os = "macos"))]
        let system_window_menu = serde_json::Value::Null;
        #[cfg(target_os = "macos")]
        let titlebar_routing = self
            .native_titlebar
            .as_ref()
            .map(|routing| routing.snapshot());
        #[cfg(not(target_os = "macos"))]
        let titlebar_routing = serde_json::Value::Null;
        let mut state = serde_json::json!({
            "vault_state": format!("{:?}", self.backend.vault.state()),
            "titlebar_routing": titlebar_routing,
            "system_window_menu": system_window_menu,
            "history_view": history_view,
            "transfer_selection": transfer_selection,
            "connection_library": connection_library,
            "window_form": window_form,
            "dialog_frame_bounds": self.qa.as_ref().and_then(|qa| qa.dialog_frame_bounds.map(Self::qa_bounds)),
            "window_info": window_info,
            "window_change": self.window_change,
            "window_restore": self.window_restore,
            "fullscreen": window.is_fullscreen(),
            "resource_focus": resource_focus,
            "modal_scroll": modal_scroll,
            "port_view": port_view,
            "resource_details": resource_details,
            "overview": overview,
            "sequence": sequence,
            "credential_prompt": credential_prompt,
            "transfers": self.transfers,
            "pending_transfers": pending_transfers,
            "file_delete_targets": delete_targets,
            "focused_control": window.focused(cx).map(|focus| format!("{focus:?}")),
            "header": header,
            "pid": std::process::id(),
            "preferences": self.prefs,
            "split_bounds": self.split_bounds.iter().map(|(id, b)| serde_json::json!({
                "id": id, "x": f32::from(b.origin.x), "y": f32::from(b.origin.y),
                "width": f32::from(b.size.width), "height": f32::from(b.size.height)
            })).collect::<Vec<_>>(),
            "tool_width": if self.active_tool().is_some() { Some(self.tool_width(window)) } else { None },
            "modal": modal,
            "update": self.qa_update_status(),
            "modal_stack": modal_stack,
            "modal_navigation": modal_navigation,
            "modal_scroll_y": f32::from(self.modal_scroll.offset().y),
            "notice": self.notice,
            "active_tab": self.active,
            "tabs": tabs,
            "history_count": self.history.len(),
            "profiles": self.profiles.len(),
            "profile_order": self.profiles.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
            "width": f32::from(window.viewport_size().width),
            "height": f32::from(window.viewport_size().height)
        });
        state["application_menu"] = application_menu;
        state["about_panel"] = about_panel;
        state["transfer_view"] = serde_json::to_value(transfer_view).unwrap_or_default();
        state["transfer_stop_targets"] =
            serde_json::to_value(transfer_stop_targets).unwrap_or_default();
        state["process_view"] = self.qa_process_snapshot();
        state["process_list"] = self.qa_process_list_snapshot(cx);
        self.backend.runtime.spawn(async move {
            if let Ok(bytes) = serde_json::to_vec_pretty(&state) {
                let temporary = path.with_extension("json.tmp");
                if tokio::fs::write(&temporary, bytes).await.is_ok() {
                    let _ = tokio::fs::rename(temporary, path).await;
                }
            }
        });
    }
}
