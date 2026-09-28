//! Native workbench. Every view action targets an explicit session/document identifier.
mod i18n;
mod shell;
mod terminal_view;
pub mod theme;
pub use shell::bind_keys;
mod connection_reorder;
mod controls;
mod dialogs;
mod history;
#[cfg(target_os = "macos")]
mod macos_titlebar;
mod ports;
#[cfg(debug_assertions)]
mod process_qa;
#[cfg(debug_assertions)]
pub mod qa;
#[cfg(not(debug_assertions))]
mod qa {
    use super::*;

    pub(super) struct Controller {
        directory: std::path::PathBuf,
        sequence: u64,
        last_snapshot: Instant,
        pub(super) header_controls: HashMap<&'static str, Bounds<Pixels>>,
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
        pub(super) overview_fixture: bool,
        pub(super) port_geometry: HashMap<&'static str, Bounds<Pixels>>,
        pub(super) port_revision: u64,
        pub(super) ports_fixture_owner: Option<Owner>,
        pub(super) process_fixture_owner: Option<Owner>,
        pub(super) pointer_events: Vec<(String, [f32; 2])>,
        pub(super) port_copy_result: Option<String>,
    }

    impl Controller {
        pub(super) fn start(_backend: &Backend) -> Option<Self> {
            None
        }
    }
}
mod system;
mod tab_reorder;
mod tools;
mod transfer_paths;
mod window_actions;
mod window_native;

use crate::{
    events::Event,
    files::{FileEntry, OpenedFile, SaveResult},
    layout::{MAX_LOCAL_PANES, PaneLayout},
    model::*,
    monitor::Sample,
    port_view::{PortKey, PortSort, ProtocolFilter},
    services::Backend,
    storage::Snapshot,
};
use dialogs::Modal;
use gpui::prelude::*;
use gpui::*;
use gpui_component::Sizable;
use gpui_component::input::{InputEvent, InputState};
use gpui_component::scroll::ScrollableElement;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    rc::Rc,
    time::{Duration, Instant},
};
use terminal_view::{PaneFocused, TerminalView};

actions!(
    mantash,
    [
        NewLocal,
        NewSsh,
        OpenConnections,
        CloseActive,
        SplitRight,
        SplitDown,
        OpenSettings,
        OpenAbout,
        OpenWindowControls,
        MinimizeWindow,
        FullscreenWindow,
        SaveFile,
        Quit,
        Escape,
        ToggleFiles,
        ToggleHistory,
        ToggleSystem,
        NextPane,
        OpenResourceDetails,
        ModalNext,
        ModalPrevious
    ]
);
/// Route the application menu's About command independently of the focused workbench control.
pub fn register_about(cx: &mut App) {
    cx.on_action(|_: &OpenAbout, cx| {
        #[cfg(target_os = "macos")]
        cx.defer(|_| {
            if let Err(error) = window_native::show_about() {
                eprintln!("Could not open system About panel: {error}");
            }
        });
        #[cfg(target_os = "windows")]
        if let Some(handle) = cx.active_window().or_else(|| cx.windows().first().copied()) {
            cx.defer(move |cx| {
                let prepared = handle.update(cx, |_, window, _| {
                    window_native::prepare_about(window, crate::APP_VERSION)
                });
                match prepared {
                    Ok(Ok(about)) => {
                        if let Err(error) = about.show() {
                            eprintln!("Could not open system About dialog: {error}");
                        }
                    }
                    Ok(Err(error)) => eprintln!("Could not open system About dialog: {error}"),
                    Err(error) => eprintln!("About window is unavailable: {error}"),
                }
            });
        }
    });
}

pub(super) struct FileTool {
    path: String,
    input: Entity<InputState>,
    entries: Vec<FileEntry>,
    selected: crate::file_selection::RowSelection,
    focus: FocusHandle,
    request: Option<Id>,
    loading: bool,
    loaded: bool,
    link_target: Option<(String, String, bool)>,
    error: Option<String>,
    show_hidden: bool,
    scroll: ScrollHandle,
    /// Horizontal handle for the outer columns container that keeps the fixed
    /// header aligned with the rows while both scroll sideways together.
    scroll_x: ScrollHandle,
    /// `name_extra = None` keeps the name column flexing to fill. Once the
    /// name edge is dragged it holds an exact width and a trailing filler
    /// owns the slack, so every edge follows the classic right-widens gesture.
    name_extra: Option<f32>,
    size_extra: f32,
    /// Directory tree state: cached children, expanded set, and pending request.
    tree_children: std::collections::HashMap<String, Vec<(String, bool)>>,
    tree_expanded: std::collections::HashSet<String>,
    /// Flattened visible tree rows; rebuilt only when expansion or directory data changes.
    tree_rows: Rc<Vec<(String, usize)>>,
    /// Exact widest-row width from the last rebuild (see rebuild_tree_rows).
    /// Cached width inputs keep scroll-frame layout O(1) even for large trees.
    tree_content_w: f32,
    tree_request: Option<(Id, String)>,
    tree_auto_load: bool,
    tree_scroll: UniformListScrollHandle,
    /// Manual tree panel width; default 180, clamped to 50% of the files panel.
    tree_width: f32,
    /// Path of the tree node whose list load is pending; shows a spinner on that node.
    tree_loading_path: Option<String>,
    encoding: Option<crate::encoding::Encoding>,
    /// Session-local copy buffer for the file context menu: source paths
    /// captured by "copy", pasted with a remote `cp -R`. Never leaves the pane.
    clipboard: Option<Vec<String>>,
}
pub(super) struct Document {
    owner: Owner,
    id: Id,
    original: OpenedFile,
    input: Entity<InputState>,
    encoding: crate::encoding::Encoding,
    revision: u64,
    dirty: bool,
    /// Latest remote read request for this document. Older responses are
    /// ignored so a rapid reopen cannot restore stale content.
    open_request: Option<Id>,
    /// Set while a programmatic refresh or revert rewrites the buffer; the
    /// deferred Change event must not mark the document dirty.
    reverting: bool,
    saving: bool,
    error: Option<String>,
}
pub(super) struct Pane {
    owner: Owner,
    spec: SessionSpec,
    state: ConnectionState,
    terminal: Option<Entity<TerminalView>>,
    pending_terminal: Option<Entity<TerminalView>>,
    directory: String,
    title: String,
    program: Option<String>,
    encoding_warning: bool,
    tool: Option<Tool>,
    last_tool: Tool,
    /// SSH files panel below the terminal; local panes never use this.
    files_open: bool,
    monitor_page: SystemPage,
    monitor_history: crate::monitor_history::History,
    monitor_resource_focus: [FocusHandle; 2],
    process_filter: Entity<InputState>,
    port_filter: Entity<InputState>,
    port_protocol: ProtocolFilter,
    port_sort: PortSort,
    port_expanded: HashSet<PortKey>,
    monitor_scroll: [ScrollHandle; 3],
    process_sort: system::ProcessSort,
    process_descending: bool,
    process_attempts: Vec<system::ProcessAttempt>,
    files: FileTool,
    documents: Vec<Document>,
    active_document: Option<Id>,
    monitor: Option<Sample>,
    monitor_request: Option<Id>,
    monitor_error: Option<String>,
    last_sample: Instant,
    /// Pending file reads keyed by request; the value identifies the document
    /// to refresh, or None when the first instance is still being created.
    opened_requests: HashMap<Id, (String, Option<Id>)>,
    /// Latest request per path; older responses are discarded.
    open_latest: HashMap<String, Id>,
}
pub(super) struct Tab {
    id: Id,
    panes: Vec<Pane>,
    active: usize,
    layout: PaneLayout,
    scroll: ScrollHandle,
}
/// Header scrolling is independent from each tab's terminal workspace scrolling.
#[derive(Default)]
pub(super) struct TabStrip {
    scroll: ScrollHandle,
    active: Option<(Id, usize)>,
    label: String,
    viewport_width: Pixels,
    fullscreen: bool,
    font: String,
    font_size: f32,
    language: Language,
    drag: Option<tab_reorder::TabDrag>,
    suppress_click: Option<Id>,
    completed_drags: u64,
    #[cfg(target_os = "macos")]
    control_bounds: std::rc::Rc<std::cell::RefCell<HashMap<&'static str, Bounds<Pixels>>>>,
}
#[derive(Clone, PartialEq)]
pub(super) enum Resize {
    Tools,
    ConnectionScroll,
    FileScroll(Owner),
    TreeScroll(Owner),
    /// Horizontal drag of the file tree's overlay scrollbar.
    TreeScrollX(Owner),
    /// Horizontal drag of the file list's overlay scrollbar.
    FileScrollX(Owner),
    /// The settings dialog's content scrollbar (modal-body scroll handle).
    ModalScroll,
    FilesHeight,
    TreeWidth,
    Split(Id, Id, Split),
    FileName(Owner),
    FileSize(Owner),
}
/// One submitted transfer batch, retained while its dialog runs in the background.
struct TransferBatch {
    owner: Owner,
    ids: Vec<Id>,
    file_path: String,
    file_request: Option<Id>,
}
pub struct Workbench {
    backend: Backend,
    receiver: async_channel::Receiver<Event>,
    prefs: Preferences,
    profiles: Vec<Profile>,
    tabs: Vec<Tab>,
    tab_strip: TabStrip,
    #[cfg(target_os = "macos")]
    native_titlebar: Option<macos_titlebar::TitlebarRegions>,
    active: usize,
    history: Vec<HistoryEntry>,
    transfers: Vec<TransferRecord>,
    transfer_batches: HashMap<Id, TransferBatch>,
    history_views: [history::HistoryView; 2],
    connection_search: Entity<InputState>,
    connection_selected: Option<Id>,
    /// Multi-select set for batch deletion in the connection library.
    connection_multi: std::collections::HashSet<Id>,
    /// Row where the last plain/additive click landed; Shift+click extends from here.
    connection_anchor: Option<Id>,
    /// Filtered rows for the library list, cached by query and profile
    /// fingerprint so scroll frames do not re-filter or re-allocate.
    connection_rows_cache: RefCell<shell::ConnectionRowsCache>,
    /// Virtualized-list handle for the connection library; offset reads and
    /// writes go through its tracked base scroll handle.
    connection_scroll: UniformListScrollHandle,
    /// Active manual-sort drag in the connection library, if any.
    connection_drag: Option<connection_reorder::ConnectionDrag>,
    /// Row whose click must be swallowed after a completed drag.
    connection_suppress_click: Option<Id>,
    connection_return: bool,
    history_return: Option<Owner>,
    /// Owner whose editor dialog to restore when a nested dialog (encoding,
    /// go-to, conflict, close confirmation) opened from it is dismissed.
    editor_return: Option<Owner>,
    /// Transfer-record selection inside the transfers dialog: selected ids
    /// and the Shift-range anchor, both cleared whenever it reopens.
    transfer_selected: std::collections::HashSet<Id>,
    transfer_anchor: Option<Id>,
    /// Parent state for a confirmation that has an explicit business return path.
    /// It is separate from the ordinary stack so a confirm cannot accidentally
    /// turn into a normal modal push/pop transition.
    modal_confirm_return: Option<dialogs::ModalFrame>,
    /// Ordinary modal parents are moved here while a child picker, form, list
    /// or detail dialog is active. Confirmation dialogs keep their explicit
    /// legacy return behavior and do not enter this stack.
    modal_stack: Vec<dialogs::ModalFrame>,
    tree_reveal: RefCell<Option<(Owner, String)>>,
    modal: Option<Modal>,
    modal_focus: FocusHandle,
    return_focus: Option<FocusHandle>,
    queued_trust: VecDeque<Event>,
    queued_credentials: VecDeque<Event>,
    compact_sidebar_open: bool,
    modal_scroll: ScrollHandle,
    /// Font-picker state: the highlighted family and the scroll position of
    /// the virtualized list. Persisted for the life of the dialog chain.
    font_highlight: Option<String>,
    font_scroll: UniformListScrollHandle,
    resource_modal_height: Option<f32>,
    history_content_marker: Rc<std::cell::Cell<Option<Bounds<Pixels>>>>,
    window_restore: Option<crate::window_layout::Rect>,
    window_change: Option<Id>,
    notice: Option<String>,
    storage_warning: Option<String>,
    root_focus: FocusHandle,
    subscriptions: Vec<Subscription>,
    resize: Option<Resize>,
    resize_last_x: Option<f32>,
    resize_last_y: Option<f32>,
    body_bounds: Bounds<Pixels>,
    split_bounds: HashMap<Id, Bounds<Pixels>>,
    split_focus: HashMap<Id, FocusHandle>,
    tool_resize_focus: FocusHandle,
    preferences_dirty: Option<Instant>,
    close_after_save: Option<dialogs::CloseTarget>,
    allow_close: bool,
    qa: Option<qa::Controller>,
}

impl Workbench {
    /// Restore independent local processes and dormant SSH panes in the saved order.
    pub fn new(
        backend: Backend,
        receiver: async_channel::Receiver<Event>,
        snapshot: Snapshot,
        warning: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        #[cfg(target_os = "macos")]
        let (native_titlebar, titlebar_error) = match macos_titlebar::TitlebarRegions::new(window) {
            Ok(routing) => (Some(routing), None),
            Err(error) => (None, Some(format!("Title bar input setup failed: {error}"))),
        };
        #[cfg(not(target_os = "macos"))]
        let titlebar_error = None;
        let connection_search = Self::input(
            "",
            i18n::text(snapshot.preferences.language, "connection_search_hint"),
            false,
            window,
            cx,
        );
        let history_views = std::array::from_fn(|_| history::HistoryView {
            search: Self::input(
                "",
                i18n::text(snapshot.preferences.language, "search_history"),
                false,
                window,
                cx,
            ),
            scroll: ScrollHandle::new(),
            selected: HashSet::new(),
            anchor: None,
        });
        theme::apply(&snapshot.preferences, window, cx);
        let qa = qa::Controller::start(&backend);
        let mut this = Self {
            backend,
            receiver,
            prefs: snapshot.preferences,
            profiles: snapshot.profiles,
            tabs: vec![],
            tab_strip: TabStrip::default(),
            #[cfg(target_os = "macos")]
            native_titlebar,
            active: snapshot.workspace.active_tab,
            history: snapshot.history,
            transfers: snapshot.transfers,
            history_views,
            connection_search,
            connection_selected: None,
            connection_multi: Default::default(),
            connection_anchor: None,
            connection_rows_cache: Default::default(),
            connection_scroll: UniformListScrollHandle::new(),
            connection_drag: None,
            connection_suppress_click: None,
            connection_return: false,
            history_return: None,
            editor_return: None,
            transfer_selected: Default::default(),
            transfer_batches: HashMap::new(),
            transfer_anchor: None,
            modal_confirm_return: None,
            modal_stack: Vec::new(),
            tree_reveal: Default::default(),
            modal: None,
            modal_focus: cx.focus_handle(),
            return_focus: None,
            queued_trust: VecDeque::new(),
            queued_credentials: VecDeque::new(),
            compact_sidebar_open: false,
            modal_scroll: ScrollHandle::new(),
            font_highlight: None,
            font_scroll: UniformListScrollHandle::new(),
            resource_modal_height: None,
            history_content_marker: Rc::new(std::cell::Cell::new(None)),
            window_restore: None,
            window_change: None,
            notice: titlebar_error,
            storage_warning: warning,
            root_focus: cx.focus_handle(),
            subscriptions: vec![],
            resize: None,
            resize_last_x: None,
            resize_last_y: None,
            body_bounds: Bounds::default(),
            split_bounds: HashMap::new(),
            split_focus: HashMap::new(),
            tool_resize_focus: cx.focus_handle(),
            preferences_dirty: None,
            close_after_save: None,
            allow_close: false,
            qa,
        };
        for index in 0..this.history_views.len() {
            let search = this.history_views[index].search.clone();
            this.subscriptions.push(cx.subscribe(
                &search,
                move |this, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        let scope = if index == HistoryScope::Local.index() {
                            HistoryScope::Local
                        } else {
                            HistoryScope::Ssh
                        };
                        let visible = this
                            .visible_history(scope, cx)
                            .iter()
                            .map(|entry| entry.id)
                            .collect::<Vec<_>>();
                        let view = &mut this.history_views[index];
                        view.selected.retain(|id| visible.contains(id));
                        view.anchor = view.anchor.filter(|id| view.selected.contains(id));
                        view.scroll.set_offset(point(px(0.), px(0.)));
                        cx.notify();
                    }
                },
            ));
        }
        this.subscriptions
            .push(cx.observe_window_activation(window, |this, window, cx| {
                if !window.is_window_active() {
                    this.cancel_tab_drag(cx);
                }
            }));
        for saved in snapshot.workspace.tabs {
            let mut panes = Vec::new();
            for entry in saved.panes {
                let mut pane = this.create_pane_with_id(entry.spec, true, entry.id, window, cx);
                // Panels stay closed until the session actually connects; the
                // editor now lives in a dialog, so a legacy saved "editor"
                // tool falls back to the system monitor page.
                pane.tool = None;
                pane.last_tool = match entry.tool.unwrap_or(entry.last_tool) {
                    Tool::Editor => Tool::System,
                    tool => tool,
                };
                pane.monitor_page = SystemPage::Overview;
                panes.push(pane);
            }
            if panes.is_empty() {
                continue;
            }
            let active = panes
                .iter()
                .position(|p| p.owner.session == saved.active_pane)
                .unwrap_or(0);
            let layout = saved
                .layout
                .unwrap_or_else(|| PaneLayout::single(panes[0].owner.session));
            this.register_layout_focus(&layout, cx);
            this.tabs.push(Tab {
                id: saved.id,
                panes,
                active,
                layout,
                scroll: ScrollHandle::new(),
            });
        }
        if this.tabs.is_empty() {
            this.new_local(window, cx);
        }
        this.active = this.active.min(this.tabs.len().saturating_sub(1));
        this.focus_active(window, cx);
        this.subscriptions.push(cx.subscribe(
            &this.connection_search,
            |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let query = this.connection_search.read(cx).value();
                    this.connection_selected = this
                        .profiles
                        .iter()
                        .find(|profile| {
                            Some(profile.id) == this.connection_selected
                                && crate::connections::matches_query(profile, &query)
                        })
                        .map(|profile| profile.id);
                    // Rows hidden by the query cannot remain batch-delete targets.
                    this.connection_multi.retain(|id| {
                        this.profiles.iter().any(|profile| {
                            profile.id == *id && crate::connections::matches_query(profile, &query)
                        })
                    });
                    this.connection_anchor = this
                        .connection_anchor
                        .filter(|id| this.connection_multi.contains(id));
                    this.connection_scroll
                        .0
                        .borrow()
                        .base_handle
                        .set_offset(point(px(0.), px(0.)));
                }
                cx.notify();
            },
        ));
        // Receive terminal/service events immediately; native redraw requests are coalesced by GPUI.
        let receiver = this.receiver.clone();
        cx.spawn_in(window, async move |entity, cx| {
            let mut batch = 0;
            while let Ok(event) = receiver.recv().await {
                if entity
                    .update_in(cx, |this, window, cx| this.event(event, window, cx))
                    .is_err()
                {
                    break;
                }
                batch += 1;
                if batch == 64 {
                    batch = 0;
                    cx.background_executor()
                        .timer(Duration::from_millis(1))
                        .await;
                }
            }
        })
        .detach();
        // Persistence and monitoring do not dictate terminal input latency.
        cx.spawn_in(window, async move |entity, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                if entity
                    .update_in(cx, |this, window, cx| this.tick(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let weak = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            weak.update(cx, |this, cx| {
                if this.allow_close {
                    true
                } else {
                    this.request_close(dialogs::CloseTarget::Window, window, cx);
                    false
                }
            })
            .unwrap_or(true)
        });
        this
    }
    pub(super) fn input(
        value: &str,
        placeholder: &str,
        masked: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let input = cx.new(|cx| {
            let mut state = InputState::new(window, cx)
                .placeholder(placeholder.to_string())
                .masked(masked);
            state.set_value(value.to_string(), window, cx);
            state
        });
        cx.observe(&input, |_, _, cx| cx.notify()).detach();
        input
    }
    /// Render compact actions with a visible focus target and localized tooltip.
    fn icon_button(
        &self,
        id: impl Into<ElementId>,
        key: &str,
        icon: gpui_component::IconName,
    ) -> controls::Button {
        self.button(id, "").icon(icon).ghost().tooltip(self.t(key))
    }
    /// Spinner matching the button icon size, for swapping a button's glyph in
    /// place via `icon_element` while its own box keeps the width.
    fn loading_spinner(&self) -> gpui_component::spinner::Spinner {
        gpui_component::spinner::Spinner::new()
            .with_size(px((self.prefs.ui_size + 2.).min(20.)))
            .color(theme::Palette::new(self.prefs.theme).accent)
    }
    pub(super) fn t(&self, key: &str) -> &'static str {
        i18n::text(self.prefs.language, key)
    }
    pub(super) fn active_pane(&self) -> Option<&Pane> {
        let tab = self.tabs.get(self.active)?;
        tab.panes.get(tab.active)
    }
    pub(super) fn active_owner(&self) -> Option<Owner> {
        self.active_pane().map(|p| p.owner)
    }
    pub(super) fn pane(&self, owner: Owner) -> Option<&Pane> {
        self.tabs
            .iter()
            .flat_map(|t| &t.panes)
            .find(|p| p.owner == owner)
    }
    pub(super) fn active_pane_mut(&mut self) -> Option<&mut Pane> {
        let active = self.active;
        let owner = self
            .tabs
            .get(active)?
            .panes
            .get(self.tabs[active].active)?
            .owner;
        self.pane_mut(owner)
    }
    pub(super) fn pane_mut(&mut self, owner: Owner) -> Option<&mut Pane> {
        self.tabs
            .iter_mut()
            .flat_map(|t| &mut t.panes)
            .find(|p| p.owner == owner)
    }
    pub(super) fn document_mut(&mut self, owner: Owner, id: Id) -> Option<&mut Document> {
        self.tabs
            .iter_mut()
            .flat_map(|t| &mut t.panes)
            .flat_map(|p| &mut p.documents)
            .find(|d| d.id == id && d.owner == owner)
    }
    pub(super) fn changed(&mut self, cx: &mut Context<Self>) {
        self.preferences_dirty = Some(Instant::now());
        cx.notify();
    }
    fn workspace(&self) -> Workspace {
        Workspace {
            active_tab: self.active,
            tabs: self
                .tabs
                .iter()
                .map(|tab| SavedTab {
                    id: tab.id,
                    active_pane: tab.panes[tab.active].owner.session,
                    layout: Some(tab.layout.clone()),
                    panes: tab
                        .panes
                        .iter()
                        .map(|pane| {
                            let mut spec = pane.spec.clone();
                            if let SessionSpec::Local { directory, .. } = &mut spec {
                                if !pane.directory.is_empty() {
                                    *directory = pane.directory.clone();
                                }
                            }
                            SavedPane {
                                id: pane.owner.session,
                                spec,
                                tool: pane.tool,
                                last_tool: pane.last_tool,
                                system_page: pane.monitor_page,
                            }
                        })
                        .collect(),
                })
                .collect(),
        }
    }
    fn register_layout_focus(&mut self, layout: &PaneLayout, cx: &mut Context<Self>) {
        if let PaneLayout::Split {
            id, first, second, ..
        } = layout
        {
            self.split_focus
                .entry(*id)
                .or_insert_with(|| cx.focus_handle());
            self.register_layout_focus(first, cx);
            self.register_layout_focus(second, cx);
        }
    }
    pub(super) fn active_tool(&self) -> Option<Tool> {
        self.active_pane()
            .filter(|p| matches!(p.spec, SessionSpec::Ssh { .. }))
            .and_then(|p| p.tool)
    }
    pub(super) fn set_active_tool(&mut self, tool: Option<Tool>, cx: &mut Context<Self>) {
        if let Some(pane) = self
            .tabs
            .get_mut(self.active)
            .and_then(|t| t.panes.get_mut(t.active))
        {
            if matches!(pane.spec, SessionSpec::Ssh { .. }) {
                pane.tool = tool;
                if let Some(tool) = tool {
                    pane.last_tool = tool;
                }
            }
        }
        self.changed(cx);
    }
    /// Open the bottom SSH file panel and lazily load its first directory.
    pub(super) fn open_files_panel(&mut self, cx: &mut Context<Self>) {
        let Some((owner, loaded, loading)) = self.active_pane().and_then(|pane| {
            (matches!(pane.spec, SessionSpec::Ssh { .. })
                && pane.state == ConnectionState::Connected)
                .then_some((pane.owner, pane.files.loaded, pane.files.loading))
        }) else {
            return;
        };
        let opened = self
            .pane_mut(owner)
            .map(|pane| {
                let opened = !pane.files_open;
                pane.files_open = true;
                opened
            })
            .unwrap_or(false);
        if !loaded && !loading {
            self.navigate(owner, None, cx);
        } else if opened {
            self.changed(cx);
        }
    }
    /// Toggle the bottom SSH file panel; local panes and unconnected sessions ignore it.
    pub(super) fn toggle_files_panel(&mut self, cx: &mut Context<Self>) {
        let Some((owner, is_open)) = self.active_pane().and_then(|pane| {
            (matches!(pane.spec, SessionSpec::Ssh { .. })
                && pane.state == ConnectionState::Connected)
                .then_some((pane.owner, pane.files_open))
        }) else {
            return;
        };
        if is_open {
            if let Some(pane) = self.pane_mut(owner) {
                pane.files_open = false;
            }
            self.changed(cx);
        } else {
            self.open_files_panel(cx);
        }
    }
    /// Reopen the last SSH work page while keeping files in the bottom panel.
    pub(super) fn reopen_tool(&mut self, cx: &mut Context<Self>) {
        match self.active_pane().map(|pane| pane.last_tool) {
            Some(Tool::Files) => self.open_files_panel(cx),
            Some(tool) => self.set_active_tool(Some(tool), cx),
            None => {}
        }
    }
    pub(super) fn tree_width(&self, _window: &Window) -> f32 {
        self.active_pane()
            .map(|p| p.files.tree_width)
            .unwrap_or(180.)
    }
    pub(super) fn files_height(&self, window: &Window) -> f32 {
        let available = f32::from(window.viewport_size().height)
            - (self.prefs.ui_size * 1.45 + 14.).max(36.)
            - 28.;
        self.prefs
            .files_preferred_height
            .filter(|v| v.is_finite())
            .unwrap_or(240.)
            .clamp(120., (available * 0.7).max(120.))
    }
    pub(super) fn tool_width(&self, window: &Window) -> f32 {
        crate::layout::tool_width(
            self.prefs.tool_preferred_width,
            window.viewport_size().width.into(),
            window.viewport_size().width.into(),
        )
    }
    pub(super) fn persist(&mut self) {
        self.backend
            .save_workspace(self.prefs.clone(), self.workspace());
        self.preferences_dirty = None;
    }
    pub(super) fn focus_active(&self, window: &mut Window, cx: &mut App) {
        // A modal owns keyboard focus for its entire lifetime. Background
        // connection events may still request a redraw, but must never move
        // focus back to a terminal while the overlay is visible.
        if self.modal.is_some() {
            return;
        }
        self.tab_strip.scroll.scroll_to_item(self.active);
        if let Some(pane) = self.active_pane() {
            if let Some(document) = pane
                .active_document
                .filter(|_| {
                    matches!(&self.modal, Some(Modal::Editor { owner }) if *owner == pane.owner)
                })
                .and_then(|id| pane.documents.iter().find(|d| d.id == id))
            {
                document.input.focus_handle(cx).focus(window);
            } else if let Some(terminal) = &pane.terminal {
                terminal.focus_handle(cx).focus(window);
            } else {
                self.root_focus.focus(window);
            }
        }
    }
    fn create_pane(
        &mut self,
        spec: SessionSpec,
        restore: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Pane {
        self.create_pane_with_id(spec, restore, Id::new_v4(), window, cx)
    }
    fn create_pane_with_id(
        &mut self,
        spec: SessionSpec,
        restore: bool,
        id: Id,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Pane {
        let owner = Owner {
            session: id,
            attempt: Id::new_v4(),
        };
        let local = matches!(spec, SessionSpec::Local { .. });
        let terminal = if local && restore {
            Some(self.start_terminal(owner, spec.clone(), String::new(), false, cx))
        } else {
            None
        };
        let directory = match &spec {
            SessionSpec::Local { directory, .. } => directory.clone(),
            _ => String::new(),
        };
        let process_filter = Self::input("", self.t("filter"), false, window, cx);
        let port_filter = Self::input("", self.t("filter"), false, window, cx);
        self.subscriptions
            .push(cx.subscribe(&process_filter, |_, _, _: &InputEvent, cx| cx.notify()));
        self.subscriptions.push(
            cx.subscribe(&port_filter, move |this, _, _: &InputEvent, cx| {
                this.prune_port_expansion(owner, cx);
                cx.notify();
            }),
        );
        Pane {
            owner,
            tool: None,
            last_tool: Tool::System,
            files_open: false,
            monitor_page: SystemPage::Overview,
            monitor_history: Default::default(),
            monitor_resource_focus: std::array::from_fn(|_| cx.focus_handle().tab_stop(true)),
            process_filter,
            port_filter,
            port_protocol: ProtocolFilter::All,
            port_sort: PortSort::Ascending,
            port_expanded: HashSet::new(),
            monitor_scroll: [
                ScrollHandle::new(),
                ScrollHandle::new(),
                ScrollHandle::new(),
            ],
            process_sort: system::ProcessSort::Cpu,
            process_descending: true,
            process_attempts: vec![],
            title: String::new(),
            program: None,
            encoding_warning: false,
            spec,
            state: if terminal.is_some() {
                ConnectionState::Connecting
            } else {
                ConnectionState::Restored
            },
            terminal,
            pending_terminal: None,
            directory,
            files: FileTool {
                path: ".".into(),
                input: Self::input(".", "", false, window, cx),
                entries: vec![],
                selected: Default::default(),
                focus: cx.focus_handle(),
                request: None,
                loading: false,
                loaded: false,
                link_target: None,
                error: None,
                show_hidden: false,
                scroll: ScrollHandle::new(),
                scroll_x: ScrollHandle::new(),
                name_extra: None,
                size_extra: 0.,
                tree_children: Default::default(),
                tree_expanded: Default::default(),
                tree_rows: Rc::new(Vec::new()),
                tree_content_w: 0.,
                tree_request: None,
                tree_auto_load: false,
                tree_scroll: UniformListScrollHandle::new(),
                tree_width: 180.,
                tree_loading_path: None,
                encoding: None,
                clipboard: None,
            },
            documents: vec![],
            active_document: None,
            monitor: None,
            monitor_request: None,
            monitor_error: None,
            last_sample: Instant::now() - Duration::from_secs(30),
            opened_requests: HashMap::new(),
            open_latest: HashMap::new(),
        }
    }
    fn start_terminal(
        &mut self,
        owner: Owner,
        spec: SessionSpec,
        secret: String,
        remember: bool,
        cx: &mut Context<Self>,
    ) -> Entity<TerminalView> {
        let session = self
            .backend
            .start(owner, spec, zeroize::Zeroizing::new(secret), remember);
        let terminal = cx.new(|cx| TerminalView::new(session, self.prefs.clone(), cx));
        self.subscriptions
            .push(cx.subscribe(&terminal, |this, _, event: &PaneFocused, cx| {
                for (index, tab) in this.tabs.iter_mut().enumerate() {
                    if let Some(pane_index) = tab.panes.iter().position(|p| p.owner == event.0) {
                        this.active = index;
                        tab.active = pane_index;
                        this.changed(cx);
                        break;
                    }
                }
            }));
        terminal
    }
    pub(super) fn local_spec(&self) -> SessionSpec {
        // Local sessions always run the platform-default shell in the user's
        // home directory; the settings dialog no longer offers overrides.
        SessionSpec::Local {
            shell: crate::platform::default_shell(),
            directory: crate::platform::home_directory(),
            encoding: self.prefs.local_encoding,
        }
    }
    pub(super) fn new_local(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.compact_sidebar_open = false;
        let pane = self.create_pane(self.local_spec(), true, window, cx);
        self.tabs.push(Tab {
            id: Id::new_v4(),
            layout: PaneLayout::single(pane.owner.session),
            panes: vec![pane],
            active: 0,
            scroll: ScrollHandle::new(),
        });
        self.active = self.tabs.len() - 1;
        self.focus_active(window, cx);
        self.changed(cx);
    }
    pub(super) fn new_ssh(
        &mut self,
        profile: Profile,
        secret: String,
        remember: bool,
        force_new: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.compact_sidebar_open = false;
        if !force_new {
            if let Some((tab, pane)) = self.tabs.iter().enumerate().find_map(|(i, t)| t.panes.iter().position(|p| matches!(&p.spec, SessionSpec::Ssh { profile: saved, .. } if saved.id == profile.id)).map(|j| (i, j))) {
                self.active = tab; self.tabs[tab].active = pane;
                let target=&mut self.tabs[tab].panes[pane];
                if matches!(target.state,ConnectionState::Restored|ConnectionState::Disconnected|ConnectionState::Cancelled|ConnectionState::Failed(_)) {
                    let owner=target.owner;let encoding=target.spec.encoding();target.spec=SessionSpec::Ssh{profile,encoding};self.reconnect(owner,secret,remember,window,cx);
                } else {self.focus_active(window,cx);self.changed(cx);}
                return;
            }
        }
        // New sessions start in UTF-8; encoding is changed per session on the SSH page.
        let spec = SessionSpec::Ssh {
            profile,
            encoding: crate::encoding::Encoding::default(),
        };
        let mut pane = self.create_pane(spec.clone(), false, window, cx);
        pane.terminal = Some(self.start_terminal(pane.owner, spec, secret, remember, cx));
        pane.state = ConnectionState::Connecting;
        self.tabs.push(Tab {
            id: Id::new_v4(),
            layout: PaneLayout::single(pane.owner.session),
            panes: vec![pane],
            active: 0,
            scroll: ScrollHandle::new(),
        });
        self.active = self.tabs.len() - 1;
        self.focus_active(window, cx);
        self.changed(cx);
    }
    pub(super) fn reconnect(
        &mut self,
        old: Owner,
        secret: String,
        remember: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let prompt = self
            .pane(old)
            .is_some_and(|pane| pane.state == ConnectionState::CredentialsRequired);
        if prompt {
            // A connection form opened from the password prompt can replace
            // the in-flight attempt. Drop the old sender and invalidate the
            // old queued event before creating the replacement attempt.
            self.cancel_credential_prompt(old);
        }
        let Some(mut spec) = self
            .pane(old)
            .filter(|p| {
                !matches!(
                    p.state,
                    ConnectionState::Connecting
                        | ConnectionState::Authenticating
                        | ConnectionState::HostVerification
                )
            })
            .map(|p| p.spec.clone())
        else {
            return;
        };
        if let SessionSpec::Ssh { profile, .. } = &mut spec {
            if let Some(saved) = self.profiles.iter().find(|saved| saved.id == profile.id) {
                *profile = saved.clone();
            }
        }
        let prompt_modal = matches!(
            &self.modal,
            Some(Modal::Trust { owner, .. }) | Some(Modal::Credentials { owner, .. })
                if *owner == old
        );
        if prompt_modal {
            self.dismiss(window, cx);
        }
        self.backend.close(old);
        let owner = Owner {
            session: old.session,
            attempt: Id::new_v4(),
        };
        let terminal = self.start_terminal(owner, spec.clone(), secret, remember, cx);
        if let Some(pane) = self.pane_mut(old) {
            pane.spec = spec;
            pane.owner = owner;
            pane.state = ConnectionState::Connecting;
            if let Some(previous) = &pane.terminal {
                previous.update(cx, |view, cx| {
                    view.connected = false;
                    cx.notify();
                });
            }
            pane.pending_terminal = Some(terminal);
            pane.files.error = None;
            pane.files.request = None;
            pane.files.loading = false;
            pane.files.loaded = false;
            pane.monitor_request = None;
            pane.monitor_error = None;
            pane.port_expanded.clear();
            pane.process_attempts.clear();
        }
        // The editor dialog points at the pre-reconnect owner; its stale
        // documents stay read-only, so close it instead of keeping a
        // dialog that can no longer find its pane.
        if matches!(&self.modal, Some(Modal::Editor { owner }) if *owner == old) {
            self.editor_return = None;
            self.dismiss(window, cx);
        }
        self.focus_active(window, cx);
        self.changed(cx);
    }
    pub(super) fn split(
        &mut self,
        direction: Split,
        chosen: Option<SessionSpec>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        if tab.panes.len() >= MAX_LOCAL_PANES
            || !tab
                .panes
                .iter()
                .all(|p| matches!(p.spec, SessionSpec::Local { .. }))
        {
            return;
        }
        if chosen
            .as_ref()
            .is_some_and(|s| !matches!(s, SessionSpec::Local { .. }))
        {
            return;
        }
        let source = &tab.panes[tab.active];
        let target = source.owner.session;
        let mut spec = chosen.unwrap_or_else(|| source.spec.clone());
        if let SessionSpec::Local { directory, .. } = &mut spec {
            if !source.directory.is_empty() {
                *directory = source.directory.clone();
            }
        }
        let pane = self.create_pane(spec, true, window, cx);
        let added = pane.owner.session;
        let tab = &mut self.tabs[self.active];
        if tab.layout.split(target, added, direction) {
            tab.panes.push(pane);
            tab.active = tab.panes.len() - 1;
            let layout = tab.layout.clone();
            self.register_layout_focus(&layout, cx);
        } else {
            self.backend.close(pane.owner);
        }
        self.focus_active(window, cx);
        self.changed(cx);
    }
    pub(super) fn set_tool(&mut self, tool: Tool, cx: &mut Context<Self>) {
        if tool == Tool::Files {
            self.open_files_panel(cx);
            return;
        }
        let next = if self.active_tool() == Some(tool) {
            None
        } else {
            Some(tool)
        };
        self.set_active_tool(next, cx);
    }
    pub(super) fn apply_preferences(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.prefs.normalize();
        theme::apply(&self.prefs, window, cx);
        for (input, key) in [(&self.connection_search, "connection_search_hint")] {
            let placeholder = self.t(key);
            input.update(cx, |state, cx| {
                state.set_placeholder(placeholder, window, cx)
            });
        }
        for history in &self.history_views {
            let placeholder = self.t("search_history");
            history.search.update(cx, |state, cx| {
                state.set_placeholder(placeholder, window, cx)
            });
        }
        for pane in self.tabs.iter().flat_map(|t| &t.panes) {
            for (input, key) in [
                (&pane.process_filter, "filter"),
                (&pane.port_filter, "filter"),
            ] {
                let text = self.t(key);
                input.update(cx, |state, cx| state.set_placeholder(text, window, cx));
            }
            for terminal in pane.terminal.iter().chain(pane.pending_terminal.iter()) {
                terminal.update(cx, |terminal, cx| {
                    terminal.preferences = self.prefs.clone();
                    cx.notify();
                });
            }
        }
        self.changed(cx);
    }
    /// Open the on-demand connection picker with search ready for typing.
    pub(super) fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.connection_return = false;
        self.show_modal(Modal::Connections, window, cx);
        self.connection_search.focus_handle(cx).focus(window);
    }
    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.allow_close {
            return;
        }
        if !self.queued_trust.is_empty() {
            let valid: HashSet<Owner> = self
                .tabs
                .iter()
                .flat_map(|t| &t.panes)
                .filter(|p| p.state == ConnectionState::HostVerification)
                .map(|p| p.owner)
                .collect();
            self.queued_trust.retain(
                |event| matches!(event, Event::HostKey { owner, .. } if valid.contains(owner)),
            );
        }
        if self.modal.is_none() {
            let owner = self.active_owner();
            if let Some(index) = self.queued_trust.iter().position(|event| matches!(event, Event::HostKey { owner: pending, .. } if Some(*pending) == owner)) {
                if let Some(event) = self.queued_trust.remove(index) { self.event(event, window, cx); }
            }
        }
        let waiting: HashSet<_> = self
            .tabs
            .iter()
            .flat_map(|t| &t.panes)
            .filter(|p| p.state == ConnectionState::CredentialsRequired)
            .map(|p| p.owner)
            .collect();
        self.queued_credentials.retain(
            |event| matches!(event, Event::Credentials { owner, .. } if waiting.contains(owner)),
        );
        if self.modal.is_none() {
            let owner = self.active_owner();
            if let Some(index) = self.queued_credentials.iter().position(|event| matches!(event, Event::Credentials { owner: pending, .. } if Some(*pending) == owner)) {
                if let Some(event) = self.queued_credentials.remove(index) { self.event(event, window, cx); }
            }
        }
        // Resource detail and process/port dialogs stay open across samples; keep
        // sampling while they are visible so their data refreshes live.
        if self.active_tool() == Some(Tool::System)
            && self.modal.as_ref().is_none_or(|modal| {
                matches!(
                    modal,
                    Modal::ResourceDetails { .. } | Modal::SystemTools { .. }
                )
            })
        {
            if let Some(pane) = self.active_pane() {
                if pane.state == ConnectionState::Connected
                    && matches!(pane.spec, SessionSpec::Ssh { .. })
                    && pane
                        .monitor
                        .as_ref()
                        .is_none_or(|sample| sample.system == "Linux")
                    && pane.monitor_request.is_none()
                    && pane.last_sample.elapsed() > Duration::from_secs(3)
                {
                    self.refresh_monitor(pane.owner, cx);
                }
            }
        }
        if self.active_tool() == Some(Tool::Files) {
            if let Some(pane) = self.active_pane() {
                if !pane.files.loaded
                    && !pane.files.loading
                    && pane.state == ConnectionState::Connected
                {
                    self.navigate(pane.owner, None, cx);
                }
            }
        }
        self.tick_processes(cx);
        let bounds = window.bounds();
        let viewport = window.viewport_size();
        if !window.is_fullscreen()
            && (self.prefs.window_width != f32::from(viewport.width)
                || self.prefs.window_height != f32::from(viewport.height)
                || self.prefs.window_x != Some(bounds.origin.x.into())
                || self.prefs.window_y != Some(bounds.origin.y.into()))
        {
            self.prefs.window_width = viewport.width.into();
            self.prefs.window_height = viewport.height.into();
            self.prefs.window_x = Some(bounds.origin.x.into());
            self.prefs.window_y = Some(bounds.origin.y.into());
            self.preferences_dirty = Some(Instant::now());
        }
        if self
            .preferences_dirty
            .is_some_and(|t| t.elapsed() > Duration::from_millis(400))
        {
            self.persist();
        }
        if let Some(target) = self.close_after_save.clone() {
            let owners = self.close_owners(&target);
            let documents: Vec<_> = self
                .tabs
                .iter()
                .flat_map(|t| &t.panes)
                .filter(|p| owners.contains(&p.owner))
                .flat_map(|p| &p.documents)
                .filter(|d| match target {
                    dialogs::CloseTarget::Document(_, id) => d.id == id,
                    _ => true,
                })
                .collect();
            if !documents.iter().any(|d| d.saving) {
                let dirty = documents.iter().any(|d| d.dirty);
                self.close_after_save = None;
                if dirty {
                    self.request_close(target, window, cx);
                } else {
                    self.finish_close(target, window, cx);
                }
            }
        }
        #[cfg(debug_assertions)]
        self.qa_tick(window, cx);
    }
    fn event(&mut self, event: Event, window: &mut Window, cx: &mut Context<Self>) {
        if self.allow_close {
            return;
        }
        match event {
            #[cfg(debug_assertions)]
            Event::Qa(request) => self.qa_request(request, window, cx),
            Event::State(owner, state) => {
                if let Some(pane) = self.pane_mut(owner) {
                    if pane.state == ConnectionState::Cancelled {
                        return;
                    }
                    if state == ConnectionState::Connected {
                        pane.tool = Some(Tool::System);
                        pane.files_open = true;
                        if let Some(terminal) = pane.pending_terminal.take() {
                            let restore_focus = pane
                                .terminal
                                .as_ref()
                                .is_some_and(|t| t.focus_handle(cx).is_focused(window));
                            terminal.update(cx, |view, cx| {
                                view.connected = true;
                                cx.notify();
                            });
                            if restore_focus {
                                terminal.focus_handle(cx).focus(window);
                            }
                            pane.terminal = Some(terminal);
                            pane.monitor = None;
                            pane.monitor_history = Default::default();
                            pane.last_sample = Instant::now() - Duration::from_secs(30);
                        }
                    } else if matches!(
                        state,
                        ConnectionState::Disconnected
                            | ConnectionState::Failed(_)
                            | ConnectionState::Cancelled
                    ) {
                        pane.pending_terminal = None;
                    }
                    pane.state = state.clone();
                    if matches!(
                        state,
                        ConnectionState::Disconnected
                            | ConnectionState::Failed(_)
                            | ConnectionState::Cancelled
                    ) {
                        pane.program = None;
                        pane.files.loading = false;
                        pane.files.request = None;
                        pane.monitor_request = None;
                        // Disconnected sessions show no stale CPU/memory/swap/disk/network state
                        // and close the side panels until the next successful connection.
                        pane.monitor = None;
                        pane.monitor_history = Default::default();
                        pane.port_expanded.clear();
                        pane.tool = None;
                        pane.files_open = false;
                        for attempt in &mut pane.process_attempts {
                            if attempt.phase.pending() {
                                attempt.phase = system::ProcessPhase::Unknown;
                            }
                        }
                    }
                    if let Some(terminal) = &pane.terminal {
                        terminal.update(cx, |t, cx| {
                            t.connected = state == ConnectionState::Connected;
                            cx.notify();
                        });
                    }
                    if matches!(
                        state,
                        ConnectionState::Cancelled
                            | ConnectionState::Disconnected
                            | ConnectionState::Failed(_)
                    ) {
                        if matches!(&self.modal, Some(Modal::Trust { owner: o, .. }) | Some(Modal::Credentials { owner: o, .. }) if *o == owner)
                        {
                            self.dismiss(window, cx);
                        }
                    }
                    if state == ConnectionState::Connected {
                        // Auto-load files and system info for every SSH pane
                        // on connection, regardless of which tab is active.
                        self.navigate(owner, None, cx);
                        self.refresh_monitor(owner, cx);
                    }
                }
            }
            Event::Credentials {
                owner,
                reason,
                remember,
                reply,
            } => {
                if !self
                    .pane(owner)
                    .is_some_and(|p| p.state == ConnectionState::CredentialsRequired)
                {
                    return;
                }
                if self.modal.is_some() || self.active_owner() != Some(owner) {
                    self.queued_credentials.push_back(Event::Credentials {
                        owner,
                        reason,
                        remember,
                        reply,
                    });
                } else {
                    let secret = Self::input("", "", true, window, cx);
                    let focus = secret.focus_handle(cx);
                    self.show_modal(
                        Modal::Credentials {
                            owner,
                            reason,
                            secret,
                            show_secret: false,
                            reply: Some(reply),
                        },
                        window,
                        cx,
                    );
                    focus.focus(window);
                }
            }
            Event::CredentialStorage { owner, saved } => {
                if self.pane(owner).is_some_and(|p| {
                    !matches!(
                        p.state,
                        ConnectionState::Cancelled
                            | ConnectionState::Failed(_)
                            | ConnectionState::Disconnected
                    )
                }) {
                    let label = self
                        .pane(owner)
                        .map(|p| p.spec.context_label())
                        .unwrap_or_default();
                    self.notice = Some(format!(
                        "{} · {}",
                        label,
                        self.t(if saved {
                            "credential_saved"
                        } else {
                            "credential_save_failed"
                        })
                    ));
                }
            }
            Event::Program(owner, program) => {
                if let Some(pane) = self.pane_mut(owner) {
                    pane.program = program;
                }
            }
            Event::EncodingWarning(owner) => {
                if let Some(pane) = self.pane_mut(owner) {
                    pane.encoding_warning = true;
                }
            }
            Event::Directory(owner, directory) => {
                let changed = self.pane_mut(owner).is_some_and(|pane| {
                    let changed = pane.directory != directory;
                    pane.directory = directory.clone();
                    changed
                });
                if changed {
                    self.preferences_dirty = Some(Instant::now());
                    // The SSH file panel follows the terminal's working directory.
                    if let Some(pane) = self.pane(owner) {
                        if matches!(pane.spec, SessionSpec::Ssh { .. })
                            && pane.files.path != directory
                        {
                            self.navigate(owner, Some(directory), cx);
                        }
                    }
                }
            }
            Event::Title(owner, title) => {
                if let Some(pane) = self.pane_mut(owner) {
                    pane.title = crate::titles::clean(&title, 256);
                }
            }
            Event::History(entry) => {
                // One command per logical list: reruns refresh the newest copy in place.
                let group = crate::model::HistoryScope::group_of(&entry.scope);
                self.history.retain(|existing| {
                    crate::model::HistoryScope::group_of(&existing.scope) != group
                        || existing.command != entry.command
                });
                self.history.insert(0, entry);
                self.history.truncate(5000);
            }
            Event::HostKey {
                owner,
                host,
                port,
                previous,
                fingerprint,
                reply,
            } => {
                if !self
                    .pane(owner)
                    .is_some_and(|p| p.state == ConnectionState::HostVerification)
                {
                    let _ = reply.send(false);
                } else if self.modal.is_some() || self.active_owner() != Some(owner) {
                    self.queued_trust.push_back(Event::HostKey {
                        owner,
                        host,
                        port,
                        previous,
                        fingerprint,
                        reply,
                    });
                } else {
                    self.show_modal(
                        Modal::Trust {
                            owner,
                            host,
                            port,
                            previous,
                            fingerprint,
                            reply: Some(reply),
                        },
                        window,
                        cx,
                    );
                }
            }
            Event::Files {
                owner,
                request,
                result,
            } => {
                let ui_size = self.prefs.ui_size;
                if let Some(pane) = self.pane_mut(owner) {
                    // Tree child listings arrive with a separate request identity.
                    // Matching on the id alone keeps a failed or redirected
                    // fetch from leaving the pending marker stuck forever.
                    if pane
                        .files
                        .tree_request
                        .as_ref()
                        .is_some_and(|(id, _)| *id == request)
                    {
                        let pending_path = pane
                            .files
                            .tree_request
                            .as_ref()
                            .map(|(_, path)| path.clone());
                        pane.files.tree_request = None;
                        match result {
                            Ok((path, entries)) if Some(&path) == pending_path.as_ref() => {
                                pane.files.tree_children.insert(
                                    path,
                                    entries
                                        .into_iter()
                                        .filter(|e| e.directory)
                                        .map(|e| (e.name, e.directory))
                                        .collect(),
                                );
                                pane.files.rebuild_tree_rows(ui_size);
                            }
                            Ok(_) => {
                                // Resolved elsewhere (e.g. through a link);
                                // nothing attaches to the requested row.
                            }
                            Err(error) => {
                                // Tree fetch failures surface through the files
                                // panel error line instead of disappearing.
                                pane.files.error = Some(error);
                            }
                        }
                        cx.notify();
                        return;
                    }
                    if pane.files.request == Some(request) {
                        pane.files.loading = false;
                        pane.files.tree_loading_path = None;
                        pane.files.loaded = true;
                        let mut loaded_ok = false;
                        match result {
                            Ok((path, entries)) => {
                                pane.files.path = path.clone();
                                // Keep the toolbar input's value in sync with
                                // the resolved directory (see
                                // render_files_toolbar for the display).
                                pane.files
                                    .input
                                    .update(cx, |s, cx| s.set_value(path.clone(), window, cx));
                                // The listing is authoritative for the tree node
                                // of this directory as well: deletions, renames
                                // and out-of-band changes must not leave stale
                                // children behind in the sidebar.
                                let directories: std::collections::HashSet<&str> = entries
                                    .iter()
                                    .filter(|e| e.directory)
                                    .map(|e| e.name.as_str())
                                    .collect();
                                let stale: Vec<String> = pane
                                    .files
                                    .tree_children
                                    .get(&path)
                                    .map(|old| {
                                        old.iter()
                                            .filter(|(name, _)| {
                                                !directories.contains(name.as_str())
                                            })
                                            .map(|(name, _)| {
                                                if path == "/" {
                                                    format!("/{}", name)
                                                } else {
                                                    format!("{}/{}", path, name)
                                                }
                                            })
                                            .collect()
                                    })
                                    .unwrap_or_default();
                                if !stale.is_empty() {
                                    // Nested caches and expanded marks under a
                                    // removed directory are unreachable; drop
                                    // them so a recreated name starts fresh.
                                    pane.files.tree_children.retain(|key, _| {
                                        stale.iter().all(|child| {
                                            key != child && !key.starts_with(&format!("{}/", child))
                                        })
                                    });
                                    for child in &stale {
                                        pane.files.tree_expanded.remove(child);
                                    }
                                }
                                pane.files.tree_children.insert(
                                    path.clone(),
                                    entries
                                        .iter()
                                        .filter(|e| e.directory)
                                        .map(|e| (e.name.clone(), true))
                                        .collect(),
                                );
                                // Entering a directory — from the list, the
                                // tree, the parent button or the path input —
                                // also reveals it in the tree: the node and
                                // every cached ancestor expand, so the tree
                                // tracks the browsing location.
                                pane.files.tree_expanded.insert(path.clone());
                                let mut ancestor = path.as_str();
                                while let Some(cut) = ancestor.rfind('/') {
                                    ancestor = if cut == 0 { "/" } else { &ancestor[..cut] };
                                    if pane.files.tree_children.contains_key(ancestor)
                                        || ancestor == "/"
                                    {
                                        pane.files.tree_expanded.insert(ancestor.to_string());
                                    }
                                    if ancestor == "/" {
                                        break;
                                    }
                                }
                                pane.files.rebuild_tree_rows(ui_size);
                                pane.files.entries = entries;
                                pane.files.selected.clear();
                                pane.files.error = None;
                                loaded_ok = true;
                            }
                            Err(error) => {
                                pane.files.error = Some(error);
                                // The current directory survives a failed listing;
                                // put the path input back in sync with it so a
                                // typed-but-missing path does not linger in the
                                // toolbar while the tree and list stay unchanged.
                                let current = pane.files.path.clone();
                                pane.files
                                    .input
                                    .update(cx, |s, cx| s.set_value(current, window, cx));
                            }
                        }
                        pane.files.tree_auto_load = loaded_ok;
                        // The listing changed on screen; make the redraw
                        // explicit instead of relying on the path input's
                        // internal notification.
                        cx.notify();
                    }
                }
                // Auto-load the tree root once the first directory listing
                // lands. The first listing may be a deep path (e.g. the home
                // directory) whose cache entry now syncs here as well, so the
                // gate is "the root has no children yet", not "the cache is
                // empty" — otherwise the very first sync suppresses the root
                // request and the tree stays blank.
                if self.pane(owner).is_some_and(|p| p.files.tree_auto_load)
                    && self.pane(owner).is_some_and(|p| {
                        !p.files.tree_children.contains_key("/") && p.files.tree_request.is_none()
                    })
                {
                    // tree_expand (not tree_toggle): the listing above may
                    // already have marked "/" expanded, and toggling would then
                    // collapse instead of fetching — leaving the tree blank
                    // until the next navigation.
                    self.tree_expand(owner, "/", cx);
                }
                // Once the tree re-renders with the expanded chain, scroll
                // the current directory's row into the visible window.
                if self.pane(owner).is_some_and(|p| p.files.loaded) {
                    if let Some(path) = self.pane(owner).map(|p| p.files.path.clone()) {
                        self.tree_reveal.borrow_mut().replace((owner, path));
                    }
                }
            }
            Event::FileLink {
                owner,
                request,
                path,
                target,
                directory,
            } => {
                let Some((pending_path, _target_id)) = self
                    .pane_mut(owner)
                    .and_then(|pane| pane.opened_requests.remove(&request))
                else {
                    return;
                };
                if self
                    .pane(owner)
                    .and_then(|pane| pane.open_latest.get(&pending_path))
                    != Some(&request)
                {
                    return;
                }
                if let Some(pane) = self.pane_mut(owner) {
                    pane.open_latest.remove(&pending_path);
                    pane.files.link_target = Some((path, target, directory));
                }
            }
            Event::FileOpened {
                owner,
                request,
                result,
            } => {
                let Some((pending_path, target_id)) = self
                    .pane_mut(owner)
                    .and_then(|pane| pane.opened_requests.remove(&request))
                else {
                    return;
                };
                if self
                    .pane(owner)
                    .and_then(|pane| pane.open_latest.get(&pending_path))
                    != Some(&request)
                {
                    return;
                }
                if let Some(pane) = self.pane_mut(owner) {
                    pane.open_latest.remove(&pending_path);
                }
                match result {
                    Ok(file) => {
                        let existing_id = target_id.or_else(|| {
                            self.pane(owner).and_then(|pane| {
                                pane.documents
                                    .iter()
                                    .find(|document| document.original.path == pending_path)
                                    .map(|document| document.id)
                            })
                        });
                        if let Some(id) = existing_id {
                            if target_id.is_none()
                                && let Some(document) = self.document_mut(owner, id)
                            {
                                document.open_request = Some(request);
                            }
                            if !self.refresh_document(owner, id, request, file.clone(), window, cx)
                                && target_id.is_none()
                            {
                                self.add_document(owner, file, window, cx);
                            }
                        } else {
                            self.add_document(owner, file, window, cx);
                        }
                    }
                    Err(error) => {
                        if let Some(id) = target_id {
                            if let Some(document) = self.document_mut(owner, id)
                                && document.open_request == Some(request)
                            {
                                document.open_request = None;
                                document.error = Some(error);
                            }
                        } else if let Some(pane) = self.pane_mut(owner) {
                            pane.files.error = Some(error);
                        }
                    }
                }
            }
            Event::FileSaved {
                owner,
                document,
                revision,
                result,
            } => {
                if let Some(doc) = self.document_mut(owner, document) {
                    doc.saving = false;
                    match result {
                        Ok(SaveResult::Saved(stamp)) => {
                            if doc.revision == revision {
                                doc.original.stamp = stamp;
                                doc.dirty = false;
                                doc.original.text = doc.input.read(cx).value().to_string();
                                doc.original.encoding = doc.encoding;
                                doc.error = None;
                            }
                        }
                        Ok(SaveResult::Conflict) => {
                            doc.error = Some("Remote file changed; draft retained".into());
                            self.close_after_save = None;
                            if self.modal.is_none()
                                && self.active_pane().is_some_and(|p| {
                                    p.owner == owner && p.active_document == Some(document)
                                })
                            {
                                self.show_modal(Modal::Conflict { owner, document }, window, cx);
                            }
                        }
                        Err(error) => {
                            doc.error = Some(error);
                            self.close_after_save = None;
                        }
                    }
                }
            }
            Event::FileOperation { owner, result, .. } => match result {
                Ok(()) => self.navigate(owner, None, cx),
                Err(error) => {
                    if let Some(p) = self.pane_mut(owner) {
                        p.files.error = Some(error);
                    }
                }
            },
            Event::Transfer(record) => {
                let record_id = record.id;
                if let Some(task) = self.transfers.iter_mut().find(|task| task.id == record_id) {
                    if task.state.active() || task.state == record.state {
                        *task = record;
                    }
                } else {
                    self.transfers.push(record);
                }
                let settled = self
                    .transfer_batches
                    .iter()
                    .filter_map(|(id, batch)| {
                        if !batch.ids.contains(&record_id)
                            || !batch.ids.iter().all(|id| {
                                self.transfers
                                    .iter()
                                    .any(|task| task.id == *id && !task.state.active())
                            })
                        {
                            return None;
                        }
                        Some(*id)
                    })
                    .collect::<Vec<_>>();
                for batch_id in settled {
                    let Some(batch) = self.transfer_batches.remove(&batch_id) else {
                        continue;
                    };
                    let all_succeeded = batch.ids.iter().all(|id| {
                        self.transfers
                            .iter()
                            .any(|task| task.id == *id && task.state == TransferState::Completed)
                    });
                    let viewing_batch = matches!(&self.modal,
                        Some(Modal::Transfer { batch, .. } | Modal::CancelTransfers { batch, .. }) if *batch == batch_id);
                    if viewing_batch {
                        if all_succeeded {
                            self.dismiss(window, cx);
                        } else if let Some(Modal::Transfer { phase, .. }) = &mut self.modal {
                            *phase = dialogs::TransferPhase::Result;
                        } else if let Some(Modal::CancelTransfers {
                            owner,
                            records,
                            overwrite,
                            scroll,
                            ..
                        }) = &self.modal
                        {
                            let scroll = scroll.clone();
                            self.modal = Some(Modal::Transfer {
                                owner: *owner,
                                batch: batch_id,
                                records: records.clone(),
                                overwrite: *overwrite,
                                phase: dialogs::TransferPhase::Result,
                                review_origin: None,
                            });
                            self.modal_scroll = scroll;
                        }
                    }
                    if self.pane(batch.owner).is_some_and(|pane| {
                        pane.state == ConnectionState::Connected
                            && pane.files.path == batch.file_path
                            && pane.files.request == batch.file_request
                    }) {
                        self.navigate(batch.owner, None, cx);
                    }
                    cx.notify();
                }
            }
            Event::Monitor {
                owner,
                request,
                result,
            } => {
                if let Some(pane) = self.pane_mut(owner) {
                    if pane.monitor_request == Some(request) {
                        pane.monitor_request = None;
                        pane.last_sample = Instant::now();
                        match result {
                            Ok(sample) => {
                                system::observe_processes(pane, &sample);
                                pane.monitor_history.push(&sample);
                                if sample.errors.contains_key("ports") {
                                    pane.port_expanded.clear();
                                } else {
                                    let query = pane.port_filter.read(cx).value();
                                    let visible = crate::port_view::visible(
                                        &sample.ports,
                                        &query,
                                        pane.port_protocol,
                                        pane.port_sort,
                                    );
                                    pane.port_expanded
                                        .retain(|key| visible.iter().any(|row| &row.key == key));
                                }
                                pane.monitor = Some(sample);
                                pane.monitor_error = None;
                            }
                            Err(error) => pane.monitor_error = Some(error),
                        }
                    }
                }
            }
            Event::ProcessDetails {
                owner,
                request,
                result,
            } => {
                if self.pane(owner).is_some() {
                    self.apply_process_details_result(owner, request, result);
                }
            }
            Event::ProcessAction {
                owner,
                request,
                result,
            } => {
                if let Some(pane) = self.pane_mut(owner) {
                    if let Some(attempt) = pane
                        .process_attempts
                        .iter_mut()
                        .find(|p| p.request == request && p.phase == system::ProcessPhase::Sending)
                    {
                        attempt.changed = Instant::now();
                        match result {
                            Ok(outcome) => attempt.phase = system::ProcessPhase::from(outcome),
                            Err(error) => {
                                attempt.phase = system::ProcessPhase::Unknown;
                                attempt.error = Some(error);
                            }
                        }
                    }
                }
                self.refresh_monitor(owner, cx);
            }
            Event::Error(error) | Event::ProfilesSaved(Err(error)) => self.notice = Some(error),
            Event::ProfilesSaved(Ok(())) => {}
            Event::Output(owner) => {
                // Hidden tabs retain one pending wakeup until their next paint; their output is still parsed.
                if self
                    .tabs
                    .get(self.active)
                    .is_some_and(|tab| tab.panes.iter().any(|p| p.owner == owner))
                {
                    if let Some(terminal) = self.pane(owner).and_then(|p| p.terminal.clone()) {
                        let pending = terminal.update(cx, |t, cx| {
                            let pending = t.session.output_wakeup.pending();
                            if pending {
                                cx.notify();
                            }
                            pending
                        });
                        if pending {
                            window.refresh();
                        }
                    }
                }
                return;
            }
        }
        #[cfg(debug_assertions)]
        self.qa_tick(window, cx);
        cx.notify();
        // Async service updates must invalidate the window as well as the leased view.
        window.refresh();
    }
}
