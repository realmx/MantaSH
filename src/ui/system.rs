//! Per-session Linux tools and fixed-target process confirmations.
use super::*;
use crate::{
    monitor::{Process, Sample, bytes},
    processes::{Action, Identity, Outcome},
};
use gpui_component::IconName;

/// CPU and disk open a snapshot; memory and Swap are always visible in overview.
#[derive(Clone, Copy)]
pub(super) enum ResourceKind {
    Cpu,
    Disk,
}
impl ResourceKind {
    /// Use one stable focus handle per resource and session across background samples.
    pub(super) fn index(self) -> usize {
        match self {
            Self::Cpu => 0,
            Self::Disk => 1,
        }
    }
    /// Stable keys identify summary buttons and QA actions independently from translations.
    pub(super) fn key(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Disk => "disk",
        }
    }
    /// Use explicit localized titles in the details dialog.
    pub(super) fn title_key(self) -> &'static str {
        match self {
            Self::Cpu => "cpu_details",
            Self::Disk => "disk_details",
        }
    }
}
/// Copy only the requested resource, excluding unrelated process and port data.
pub(super) enum ResourceSnapshot {
    Cpu(Vec<crate::monitor::Cpu>),
    Disk(Vec<crate::monitor::Disk>),
}
impl ResourceSnapshot {
    /// Describe the frozen payload without consulting the currently active session.
    pub(super) fn kind(&self) -> ResourceKind {
        match self {
            Self::Cpu(_) => ResourceKind::Cpu,
            Self::Disk(_) => ResourceKind::Disk,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ProcessSort {
    Pid,
    Name,
    User,
    Cpu,
    Memory,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ProcessPhase {
    Sending,
    Sent,
    Gone,
    Changed,
    Denied,
    Unsupported,
    Unknown,
    StillRunning,
}

impl From<Outcome> for ProcessPhase {
    fn from(value: Outcome) -> Self {
        match value {
            Outcome::Sent => Self::Sent,
            Outcome::Gone => Self::Gone,
            Outcome::Changed => Self::Changed,
            Outcome::Denied => Self::Denied,
            Outcome::Unsupported => Self::Unsupported,
            Outcome::Unknown => Self::Unknown,
        }
    }
}
impl ProcessPhase {
    pub fn key(self) -> &'static str {
        match self {
            Self::Sending => "process_sending",
            Self::Sent => "process_sent",
            Self::Gone => "process_gone",
            Self::Changed => "process_changed",
            Self::Denied => "process_denied",
            Self::Unsupported => "process_unsupported",
            Self::Unknown => "process_unknown",
            Self::StillRunning => "process_running",
        }
    }
    pub fn pending(self) -> bool {
        matches!(self, Self::Sending | Self::Sent)
    }
}
pub(super) struct ProcessAttempt {
    pub request: Id,
    pub identity: Identity,
    pub phase: ProcessPhase,
    pub changed: Instant,
    pub error: Option<String>,
}

/// Never remove a row on acknowledgement; only the server's next valid snapshot replaces it.
pub(super) fn observe_processes(pane: &mut Pane, sample: &Sample) {
    for attempt in &mut pane.process_attempts {
        if matches!(
            attempt.phase,
            ProcessPhase::Sent | ProcessPhase::StillRunning
        ) && crate::processes::instance_gone(&attempt.identity, sample)
        {
            attempt.phase = ProcessPhase::Gone;
        }
    }
}

impl Workbench {
    /// Return only the still-live original process from a recent, valid Linux sample.
    pub(super) fn process_target<'a>(
        &'a self,
        owner: Owner,
        process: &Process,
    ) -> Result<&'a Process, &'static str> {
        let identity = process
            .identity
            .as_ref()
            .ok_or("process_identity_missing")?;
        identity
            .validate()
            .map_err(|_| "process_identity_missing")?;
        let pane = self.pane(owner).ok_or("process_offline")?;
        if pane.state != ConnectionState::Connected {
            return Err("process_offline");
        }
        if pane.monitor_error.is_some() || pane.last_sample.elapsed() > Duration::from_secs(10) {
            return Err("process_sample_unavailable");
        }
        let sample = pane.monitor.as_ref().ok_or("process_sample_unavailable")?;
        crate::processes::current_process(identity, sample).map_err(|issue| match issue {
            crate::processes::TargetIssue::IdentityMissing => "process_identity_missing",
            crate::processes::TargetIssue::InvalidSample => "process_sample_unavailable",
            crate::processes::TargetIssue::HostChanged | crate::processes::TargetIssue::Changed => {
                "process_changed"
            }
            crate::processes::TargetIssue::Gone => "process_gone",
        })
    }

    /// A pending signal or a confirmed exit cannot be treated as a new actionable target.
    pub(super) fn process_action_reason(
        &self,
        owner: Owner,
        process: &Process,
    ) -> Option<&'static str> {
        if matches!(&self.modal, Some(Modal::ProcessDetails { owner: current, process: selected, preview: true, .. })
            if *current == owner && selected.identity == process.identity)
        {
            return Some("process_preview_only");
        }
        if self.modal_confirm_return.as_ref().is_some_and(|frame|
            matches!(&frame.modal, Modal::ProcessDetails { owner: current, process: selected, preview: true, .. }
                if *current == owner && selected.identity == process.identity)) {
            return Some("process_preview_only");
        }
        let identity = process.identity.as_ref();
        let attempt = self.pane(owner).and_then(|pane| {
            pane.process_attempts
                .iter()
                .rev()
                .find(|attempt| Some(&attempt.identity) == identity)
        });
        if let Some(attempt) = attempt {
            if attempt.phase.pending() {
                return Some(attempt.phase.key());
            }
            if attempt.phase == ProcessPhase::Gone {
                return Some("process_gone");
            }
        }
        self.process_target(owner, process).err()
    }

    /// Revalidate cached identity, connectivity and the sampled original before any signal.
    pub(super) fn can_signal(&self, owner: Owner, process: &Process) -> bool {
        self.process_action_reason(owner, process).is_none()
    }
    pub(super) fn show_process_details(
        &mut self,
        owner: Owner,
        process: Process,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pane(owner).is_none() {
            return;
        }
        let request = Id::new_v4();
        let reason = self.process_target(owner, &process).err();
        let result = reason.map(|reason| Err(self.t(reason).to_string()));
        let refreshing = reason.is_none();
        if refreshing {
            if let Some(identity) = &process.identity {
                self.backend
                    .process_details(owner, request, identity.clone());
            }
        }
        self.show_modal(
            Modal::ProcessDetails {
                owner,
                process,
                request,
                result,
                refreshing,
                refresh_error: None,
                raw_expanded: false,
                command_expanded: false,
                preview: false,
            },
            window,
            cx,
        );
    }

    /// A refresh keeps the prior detail text and only accepts the latest request.
    pub(super) fn refresh_process_details(&mut self, cx: &mut Context<Self>) {
        let Some(Modal::ProcessDetails {
            owner,
            process,
            refreshing: false,
            ..
        }) = &self.modal
        else {
            return;
        };
        let owner = *owner;
        let process = process.clone();
        let reason = self
            .process_target(owner, &process)
            .err()
            .map(|reason| self.t(reason).to_string());
        let preview = matches!(
            &self.modal,
            Some(Modal::ProcessDetails { preview: true, .. })
        );
        let Some(Modal::ProcessDetails {
            request,
            refreshing,
            refresh_error,
            ..
        }) = &mut self.modal
        else {
            return;
        };
        if let Some(reason) = reason {
            *refresh_error = Some(reason);
            cx.notify();
            return;
        }
        *request = Id::new_v4();
        *refreshing = true;
        *refresh_error = None;
        if !preview {
            if let Some(identity) = process.identity {
                self.backend.process_details(owner, *request, identity);
            }
        }
        cx.notify();
    }

    /// Apply a bounded reply to the latest request, including a suspended detail view.
    pub(super) fn apply_process_details_result(
        &mut self,
        owner: Owner,
        request: Id,
        result: std::result::Result<crate::processes::Details, String>,
    ) -> bool {
        fn update(
            modal: &mut Modal,
            owner: Owner,
            request: Id,
            result: std::result::Result<crate::processes::Details, String>,
        ) -> bool {
            let Modal::ProcessDetails {
                owner: current_owner,
                request: current_request,
                refreshing,
                refresh_error,
                result: slot,
                ..
            } = modal
            else {
                return false;
            };
            if *current_owner != owner || *current_request != request || !*refreshing {
                return false;
            }
            *refreshing = false;
            if let Err(error) = &result {
                if matches!(slot, Some(Ok(_))) {
                    *refresh_error = Some(error.clone());
                    return true;
                }
            }
            *slot = Some(result);
            *refresh_error = None;
            true
        }
        if let Some(modal) = &mut self.modal {
            if update(modal, owner, request, result.clone()) {
                return true;
            }
        }
        for frame in &mut self.modal_stack {
            if update(&mut frame.modal, owner, request, result.clone()) {
                return true;
            }
        }
        self.modal_confirm_return
            .as_mut()
            .is_some_and(|frame| update(&mut frame.modal, owner, request, result))
    }

    /// Capture the source tab and target before either signal confirmation.
    pub(super) fn review_process_action(
        &mut self,
        owner: Owner,
        process: Process,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(&self.modal, Some(Modal::ProcessDetails { owner: current, process: original, .. })
            if *current == owner && original.identity == process.identity)
        {
            return;
        }
        if matches!(
            &self.modal,
            Some(Modal::ProcessDetails { preview: true, .. })
        ) {
            return;
        }
        if self.process_action_reason(owner, &process).is_some() {
            return;
        }
        let Some(pane) = self.pane(owner) else {
            return;
        };
        let SessionSpec::Ssh { profile, .. } = &pane.spec else {
            return;
        };
        let host = format!("{}:{}", profile.host, profile.port);
        self.show_modal(
            Modal::ProcessConfirm {
                owner,
                process,
                action,
                host,
            },
            window,
            cx,
        );
    }
    pub(super) fn submit_process(
        &mut self,
        owner: Owner,
        process: Process,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.modal_confirm_return.as_ref().is_some_and(|frame| {
            matches!(&frame.modal, Modal::ProcessDetails { preview: true, .. })
        }) {
            return;
        }
        if !matches!(&self.modal, Some(Modal::ProcessConfirm { owner: current, process: target, action: selected, host })
            if *current == owner && target.identity == process.identity && *selected == action
                && self.pane(owner).is_some_and(|pane| matches!(&pane.spec, SessionSpec::Ssh { profile, .. }
                    if host == &format!("{}:{}", profile.host, profile.port))))
        {
            return;
        }
        if let Some(reason) = self.process_action_reason(owner, &process) {
            let message = self.t(reason).to_string();
            if self.restore_confirmation_parent(window, cx) {
                if let Some(Modal::ProcessDetails { refresh_error, .. }) = &mut self.modal {
                    *refresh_error = Some(message);
                }
            }
            cx.notify();
            return;
        }
        let identity = process.identity.unwrap();
        let request = Id::new_v4();
        if let Some(pane) = self.pane_mut(owner) {
            pane.process_attempts
                .retain(|a| a.identity != identity || a.phase.pending());
            if pane.process_attempts.len() >= 32 {
                pane.process_attempts.retain(|a| a.phase.pending());
            }
            pane.process_attempts.push(ProcessAttempt {
                request,
                identity: identity.clone(),
                phase: ProcessPhase::Sending,
                changed: Instant::now(),
                error: None,
            });
        }
        self.backend
            .process_action(owner, request, identity, action);
        self.restore_confirmation_parent(window, cx);
        cx.notify();
    }
    /// An explicit signal gets bounded follow-up even with automatic monitoring disabled.
    pub(super) fn tick_processes(&mut self, cx: &mut Context<Self>) {
        let mut owners = vec![];
        for pane in self.tabs.iter_mut().flat_map(|tab| &mut tab.panes) {
            let mut pending = false;
            for attempt in &mut pane.process_attempts {
                if attempt.phase == ProcessPhase::Sent {
                    if attempt.changed.elapsed() > Duration::from_secs(10) {
                        attempt.phase = ProcessPhase::StillRunning;
                        cx.notify();
                    } else {
                        pending = true;
                    }
                }
            }
            #[cfg(debug_assertions)]
            if self
                .qa
                .as_ref()
                .is_some_and(|qa| qa.process_fixture_owner == Some(pane.owner))
            {
                continue;
            }
            if pending
                && pane.monitor_request.is_none()
                && pane.last_sample.elapsed() > Duration::from_secs(1)
            {
                owners.push(pane.owner);
            }
        }
        for owner in owners {
            self.refresh_monitor(owner, cx);
        }
    }
    /// Draw measured samples in a bounded time window; missing intervals remain separate paths.
    fn trend_chart(
        &self,
        series: Vec<(Vec<Vec<(f32, f32)>>, Hsla)>,
        maximum: f32,
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        let border = theme::Palette::new(self.prefs.theme).border.opacity(0.55);
        #[cfg(debug_assertions)]
        let view = _cx.entity();
        canvas(
            move |_bounds, _, _cx| {
                #[cfg(debug_assertions)]
                view.update(_cx, |this, _| {
                    if let Some(qa) = &mut this.qa {
                        qa.network_plot_bounds = Some(_bounds);
                    }
                });
            },
            move |bounds, _, window, _| {
                let width = f32::from(bounds.size.width).max(1.);
                let height = f32::from(bounds.size.height).max(1.);
                for fraction in [0., 0.5, 1.] {
                    let mut grid = PathBuilder::stroke(px(1.));
                    grid.move_to(bounds.origin + point(px(0.), px(fraction * (height - 1.))));
                    grid.line_to(bounds.origin + point(px(width), px(fraction * (height - 1.))));
                    if let Ok(path) = grid.build() {
                        window.paint_path(path, border);
                    }
                }
                for (series_index, (segments, color)) in series.iter().enumerate() {
                    for segment in segments {
                        // Fill only each observed receive segment; missing intervals remain empty.
                        if series_index == 0 && segment.len() > 1 {
                            let mut area = PathBuilder::fill();
                            area.move_to(
                                bounds.origin + point(px(segment[0].0 * width), px(height - 1.)),
                            );
                            for (x, y) in segment {
                                area.line_to(
                                    bounds.origin
                                        + point(
                                            px(x.clamp(0., 1.) * width),
                                            px((1. - (y / maximum.max(1.)).clamp(0., 1.))
                                                * (height - 2.)
                                                + 1.),
                                        ),
                                );
                            }
                            area.line_to(
                                bounds.origin
                                    + point(px(segment.last().unwrap().0 * width), px(height - 1.)),
                            );
                            area.close();
                            if let Ok(path) = area.build() {
                                window.paint_path(path, color.opacity(0.09));
                            }
                        }
                        let mut line = if series_index == 1 {
                            PathBuilder::stroke(px(2.)).dash_array(&[px(4.), px(3.)])
                        } else {
                            PathBuilder::stroke(px(2.))
                        };
                        for (i, (x, y)) in segment.iter().enumerate() {
                            let point = bounds.origin
                                + point(
                                    px(x.clamp(0., 1.) * width),
                                    px((1. - (y / maximum.max(1.)).clamp(0., 1.)) * (height - 2.)
                                        + 1.),
                                );
                            if i == 0 {
                                line.move_to(point);
                            } else {
                                line.line_to(point);
                            }
                        }
                        if segment.len() == 1 {
                            let (x, y) = segment[0];
                            let position = bounds.origin
                                + point(
                                    px(x * width - 2.),
                                    px((1. - y / maximum.max(1.)) * height),
                                );
                            window.paint_quad(fill(
                                Bounds::new(position, size(px(3.), px(3.))),
                                *color,
                            ));
                        }
                        if let Ok(path) = line.build() {
                            window.paint_path(path, *color);
                        }
                    }
                }
            },
        )
        .w_full()
        .h(px(56.))
        .into_any_element()
    }
    /// Measure the real GPUI card boxes during isolated draw QA, without changing their layout.
    fn measure_overview_card(
        &self,
        _name: &'static str,
        card: impl IntoElement + ParentElement + Styled,
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        #[cfg(debug_assertions)]
        let card = card.when(self.qa.is_some(), |card| {
            let view = _cx.entity();
            card.relative().child(
                canvas(
                    move |bounds, _, cx| {
                        view.update(cx, |this, _| {
                            if let Some(qa) = &mut this.qa {
                                // Absolute bounds are the padding box; include this card's 1px border.
                                let outer = Bounds::new(
                                    bounds.origin - point(px(1.), px(1.)),
                                    bounds.size + size(px(2.), px(2.)),
                                );
                                qa.overview_bounds.insert(_name, outer);
                                qa.overview_revision += 1;
                            }
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
        });
        card.into_any_element()
    }
    /// Stack resource sections at their content height without stretching short bars into cards.
    fn overview_card(&self) -> gpui::Div {
        // Modules are flush full-width sections on the panel's uniform background;
        // hairline dividers separate them.
        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .min_w_0()
            .w_full()
            .gap(px(theme::SPACE_SMALL))
            .p(px(theme::SPACE_CONTROL))
    }
    /// Place the name, capacity and percentage immediately above a track with equal left/right insets.
    fn resource_meter(
        &self,
        label: &str,
        percent: Option<f64>,
        detail: Option<String>,
        action: bool,
        width: f32,
    ) -> AnyElement {
        let p = theme::Palette::new(self.prefs.theme);
        let percent = percent.filter(|value| value.is_finite() && (0. ..=100.).contains(value));
        let value = percent
            .map(|value| format!("{value:.1}%"))
            .unwrap_or_else(|| "—".into());
        // Give long capacities the full row at large fonts instead of wrapping inside the name column.
        let capacity_below = detail
            .as_ref()
            .is_some_and(|text| text.contains('/') && width < self.prefs.ui_size * 17.);
        let inline_detail = if capacity_below { None } else { detail.clone() };
        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w(px(width))
            .min_w_0()
            .line_height(relative(1.5))
            .gap(px(theme::SPACE_CONTROL))
            .child(
                div()
                    .flex()
                    .w(px(width))
                    .items_start()
                    .gap(px(theme::SPACE_CONTROL))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_wrap()
                            .items_baseline()
                            .gap(px(theme::SPACE_CONTROL))
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(label.to_owned()),
                            )
                            .when_some(inline_detail, |identity, detail| {
                                identity.child(
                                    div()
                                        .min_w_0()
                                        .whitespace_normal()
                                        .text_color(p.muted)
                                        .child(detail),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_shrink_0()
                            .items_center()
                            .gap(px(theme::SPACE_CONTROL))
                            .child(value)
                            .when(action, |reading| {
                                reading.child(
                                    gpui_component::Icon::new(IconName::ChevronRight)
                                        .size(px(self.prefs.ui_size))
                                        .text_color(p.muted),
                                )
                            }),
                    ),
            )
            .when(capacity_below, |meter| {
                meter.child(
                    div()
                        .w(px(width))
                        .min_w_0()
                        .whitespace_normal()
                        .text_color(p.muted)
                        .child(detail.unwrap_or_default()),
                )
            })
            .child(
                div()
                    .w(px(width))
                    .h(px(10.))
                    .flex_shrink_0()
                    .rounded(px(3.))
                    .bg(p.border)
                    .when_some(percent, |track, value| {
                        track.child(
                            div()
                                .w(px(width * value as f32 / 100.))
                                .h_full()
                                .rounded(px(3.))
                                .bg(p.meter_color(value)),
                        )
                    }),
            )
            .into_any_element()
    }
    /// One CPU core as a single left-center-right row: name left, usage bar
    /// stretching between, percent right. The bar fill is a fraction of the
    /// flex track because the row width is not known at build time.
    fn cpu_core_row(&self, label: &str, percent: Option<f64>) -> Div {
        let p = theme::Palette::new(self.prefs.theme);
        let percent = percent.filter(|value| value.is_finite() && (0. ..=100.).contains(value));
        let value = percent
            .map(|value| format!("{value:.1}%"))
            .unwrap_or_else(|| "—".into());
        div()
            .flex()
            .items_center()
            .gap(px(theme::SPACE_CONTROL))
            .line_height(relative(1.5))
            .child(
                div()
                    .flex_shrink_0()
                    .min_w(px(self.prefs.ui_size * 3.))
                    .child(label.to_owned()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(px(10.))
                    .rounded(px(3.))
                    .bg(p.border)
                    .when_some(percent, |track, value| {
                        track.child(
                            div()
                                .w(relative(value as f32 / 100.))
                                .h_full()
                                .rounded(px(3.))
                                .bg(p.meter_color(value)),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .justify_end()
                    .w(px(self.prefs.ui_size * 3.6))
                    .child(value),
            )
    }
    /// Show used/total capacity using the same byte formatter as the detailed mount list.
    fn capacity_summary(&self, used: u64, total: u64) -> String {
        format!("{} / {}", bytes(used), bytes(total))
    }
    /// Lay the meter out directly in the full-width row; native button label wrappers shrink flex tracks to zero.
    fn percentage_card(
        &self,
        owner: Owner,
        kind: ResourceKind,
        label: &str,
        percent: Option<f64>,
        detail: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = theme::Palette::new(self.prefs.theme);
        let Some(pane) = self.pane(owner) else {
            return div().into_any_element();
        };
        let focus = pane.monitor_resource_focus[kind.index()].clone();
        let click_focus = focus.clone();
        // Give GPUI a definite width before it measures the nested percentage track and wrapped labels.
        let available = self.tool_width(window);
        let card = self
            .overview_card()
            .w(px(available))
            .id(SharedString::from(format!(
                "resource-{}-{}",
                kind.key(),
                owner.session
            )))
            .track_focus(&focus)
            .key_context("MantaSHResource")
            .cursor_pointer()
            .px(px(theme::SPACE_PANEL))
            .py(px(theme::SPACE_CONTROL))
            .hover(|row| row.bg(p.background).border_color(p.muted.opacity(0.45)))
            .active(|row| row.bg(p.selected))
            .when(focus.is_focused(window), |row| {
                row.border_color(p.accent).bg(p.selected)
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    if this.active_owner() == Some(owner) {
                        click_focus.focus(window);
                        cx.stop_propagation();
                        cx.notify();
                    }
                }),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.show_resource_details(owner, kind, window, cx);
                cx.stop_propagation();
            }))
            .on_action(
                cx.listener(move |this, _: &OpenResourceDetails, window, cx| {
                    this.show_resource_details(owner, kind, window, cx);
                    cx.stop_propagation();
                }),
            )
            .child(self.resource_meter(
                label,
                percent,
                Some(detail),
                true,
                available - 2. * theme::SPACE_PANEL,
            ));
        self.measure_overview_card(kind.key(), card, cx)
    }
    /// Freeze the original host, attempt and sample so background changes cannot retarget details.
    pub(super) fn show_resource_details(
        &mut self,
        owner: Owner,
        kind: ResourceKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_owner() != Some(owner) || self.active_tool() != Some(Tool::System) {
            return;
        }
        let Some(pane) = self.pane(owner) else {
            return;
        };
        let Some(sample) = &pane.monitor else {
            return;
        };
        let return_focus = pane.monitor_resource_focus[kind.index()].clone();
        let data = match kind {
            ResourceKind::Cpu => ResourceSnapshot::Cpu(sample.cpu.clone()),
            ResourceKind::Disk => ResourceSnapshot::Disk(sample.disks.clone()),
        };
        let modal = Modal::ResourceDetails {
            owner,
            timestamp: sample.timestamp,
            data,
        };
        self.show_modal(modal, window, cx);
        self.return_focus = Some(return_focus);
    }
    /// A mount uses two information rows and one thin meter, with no repeated disk heading or nested card.
    fn disk_detail_row(
        &self,
        disk: &crate::monitor::Disk,
        width: f32,
        separated: bool,
    ) -> AnyElement {
        let p = theme::Palette::new(self.prefs.theme);
        let percent = (disk.total > 0)
            .then(|| 100. * disk.used as f64 / disk.total as f64)
            .filter(|value| value.is_finite() && (0. ..=100.).contains(value));
        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w(px(width))
            .min_w_0()
            .line_height(relative(1.5))
            .gap(px(theme::SPACE_SMALL))
            .py(px(theme::SPACE_CONTROL))
            .when(separated, |row| row.border_t_1().border_color(p.border))
            .child(
                div()
                    .flex()
                    .w(px(width))
                    .items_start()
                    .gap(px(theme::SPACE_CONTROL))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_wrap()
                            .gap(px(theme::SPACE_CONTROL))
                            .child(
                                div()
                                    .min_w_0()
                                    .whitespace_normal()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(disk.mount.clone()),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .whitespace_normal()
                                    .text_color(p.muted)
                                    .child(disk.filesystem.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_shrink_0()
                            .items_baseline()
                            .gap(px(theme::SPACE_CONTROL))
                            .child(
                                div()
                                    .min_w_0()
                                    .whitespace_normal()
                                    .text_color(p.muted)
                                    .child(format!("{} / {}", bytes(disk.used), bytes(disk.total))),
                            )
                            .child(
                                div().flex_shrink_0().text_right().child(
                                    percent
                                        .map(|value| format!("{value:.1}%"))
                                        .unwrap_or_else(|| "—".into()),
                                ),
                            ),
                    ),
            )
            .child(
                div()
                    .w(px(width))
                    .h(px(6.))
                    .flex_shrink_0()
                    .rounded(px(3.))
                    .bg(p.border)
                    .when_some(percent, |track, value| {
                        track.child(
                            div()
                                .w(px(width * value as f32 / 100.))
                                .h_full()
                                .rounded(px(3.))
                                .bg(p.meter_color(value)),
                        )
                    }),
            )
            .into_any_element()
    }
    /// One consistent section surface separates resource groups inside the scrollable dialog.
    fn resource_section(&self) -> gpui::Div {
        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .min_w_0()
            .gap(px(theme::SPACE_PANEL))
    }
    /// Render CPU cores and a compact mount list inside their fixed-host snapshot dialog.
    pub(super) fn render_resource_details(
        &self,
        data: &ResourceSnapshot,
        width: f32,
    ) -> AnyElement {
        let meter_width = width;
        let mut body = div()
            .flex()
            .flex_col()
            .min_w_0()
            .gap(px(theme::SPACE_PANEL));
        match data {
            ResourceSnapshot::Cpu(cpus) => {
                body = body
                    .child(self.resource_section().child(self.resource_meter(
                        "CPU",
                        cpus.first().and_then(|cpu| cpu.percent),
                        None,
                        false,
                        meter_width,
                    )))
                    .child(
                        // Cores are compact left-center-right rows without a
                        // group header; the aggregate meter above separates
                        // them. A single core keeps the whole row, larger
                        // counts flow through two columns.
                        self.resource_section()
                            .gap(px(theme::SPACE_SMALL))
                            .when(cpus.len() == 2, |section| {
                                section.children(
                                    cpus.iter()
                                        .skip(1)
                                        .map(|cpu| self.cpu_core_row(&cpu.name, cpu.percent)),
                                )
                            })
                            .when(cpus.len() > 2, |section| {
                                section.child(
                                    div()
                                        .grid()
                                        .grid_cols(2)
                                        .gap_x(px(theme::SPACE_PANEL))
                                        .gap_y(px(theme::SPACE_SMALL))
                                        .children(
                                            cpus.iter().skip(1).map(|cpu| {
                                                self.cpu_core_row(&cpu.name, cpu.percent)
                                            }),
                                        ),
                                )
                            })
                            .when(cpus.len() <= 1, |section| {
                                section.child(self.t("no_samples"))
                            }),
                    );
            }
            ResourceSnapshot::Disk(disks) => {
                body = body
                    .gap_0()
                    .when(disks.is_empty(), |body| body.child(self.t("no_samples")))
                    .children({
                        // Mount points read in a stable order, root filesystem first.
                        let mut sorted = disks.clone();
                        sorted.sort_by(|a, b| a.mount.cmp(&b.mount));
                        sorted
                            .iter()
                            .enumerate()
                            .map(|(index, disk)| self.disk_detail_row(disk, width, index > 0))
                            .collect::<Vec<_>>()
                    });
            }
        }
        body.into_any_element()
    }
    /// Keep physical memory and Swap in one non-interactive overview section.
    /// The merged memory/swap card opens the processes dialog on click.
    fn memory_card(
        &self,
        owner: Owner,
        sample: &Sample,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = theme::Palette::new(self.prefs.theme);
        let width = self.tool_width(window);
        let inner = width - 2. * theme::SPACE_PANEL;
        let mut card = self
            .overview_card()
            .w(px(width))
            .id(SharedString::from(format!("memory-card-{}", owner.session)))
            .cursor_pointer()
            .px(px(theme::SPACE_PANEL))
            .py(px(theme::SPACE_CONTROL))
            .gap(px(theme::SPACE_CONTROL))
            .hover(|row| row.bg(p.background).border_color(p.muted.opacity(0.45)))
            .active(|row| row.bg(p.selected))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.show_modal(
                    Modal::SystemTools {
                        owner,
                        page: SystemPage::Processes,
                    },
                    window,
                    cx,
                );
                cx.stop_propagation();
            }));
        if let Some(memory) = &sample.memory {
            let used = memory.total.saturating_sub(memory.available);
            let swap_used = memory.swap_total.saturating_sub(memory.swap_free);
            card = card
                .child(self.resource_meter(
                    self.t("memory"),
                    (memory.total > 0).then(|| 100. * used as f64 / memory.total as f64),
                    Some(self.capacity_summary(used, memory.total)),
                    true,
                    inner,
                ))
                .child(
                    div()
                        .pt(px(theme::SPACE_CONTROL))
                        .border_t_1()
                        .border_color(p.border)
                        .child(
                            self.resource_meter(
                                self.t("swap_label"),
                                (memory.swap_total > 0)
                                    .then(|| 100. * swap_used as f64 / memory.swap_total as f64),
                                Some(if memory.swap_total > 0 {
                                    self.capacity_summary(swap_used, memory.swap_total)
                                } else {
                                    self.t("swap_disabled").into()
                                }),
                                true,
                                inner,
                            ),
                        ),
                );
        } else {
            card = card
                .child(self.resource_meter(self.t("memory"), None, None, true, inner))
                .child(self.resource_meter(self.t("swap_label"), None, None, true, inner));
        }
        self.measure_overview_card("memory", card, cx)
    }
    /// Pair each direction label with its current rate, keeping units readable at the user's font size.
    fn network_speed(
        &self,
        key: &str,
        value: Option<f64>,
        color: Hsla,
        icon: IconName,
    ) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .min_w_0()
            .gap(px(theme::SPACE_SMALL))
            .text_color(color)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(theme::SPACE_SMALL))
                    .child(gpui_component::Icon::new(icon).size(px(self.prefs.ui_size)))
                    .child(self.t(key)),
            )
            .child(
                div()
                    .text_size(px(self.prefs.ui_size + 2.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .whitespace_normal()
                    .child(
                        value
                            .map(|v| format!("{}/s", bytes(v as u64)))
                            .unwrap_or_else(|| "—".into()),
                    ),
            )
            .into_any_element()
    }
    /// Separate current speeds, their shared trend, and per-interface cumulative traffic.
    /// The network card opens the ports dialog on click.
    fn network_card(
        &self,
        pane: &Pane,
        sample: &Sample,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = theme::Palette::new(self.prefs.theme);
        let owner = pane.owner;
        let history = &pane.monitor_history;
        let latest = history.points.back();
        let rx = history.series(|point| point.received);
        let tx = history.series(|point| point.sent);
        let maximum = rx
            .iter()
            .chain(&tx)
            .flatten()
            .map(|(_, value)| *value)
            .fold(1024., f32::max);
        let empty = rx.is_empty() && tx.is_empty();
        // Keep the two traffic columns aligned across every interface while
        // letting each column grow only to the widest value it must display.
        // GPUI's grid helper only creates equal `1fr` tracks, so the rows use
        // measured flex children instead of imposing three equal widths.
        let traffic_rows: Vec<_> = sample
            .network
            .iter()
            .map(|network| {
                (
                    network.name.clone(),
                    format!("↓ {}", bytes(network.received)),
                    format!("↑ {}", bytes(network.sent)),
                )
            })
            .collect();
        let received_width = traffic_rows
            .iter()
            .map(|(_, received, _)| super::tools::estimate_text_width(received, self.prefs.ui_size))
            .fold(0., f32::max);
        let sent_width = traffic_rows
            .iter()
            .map(|(_, _, sent)| super::tools::estimate_text_width(sent, self.prefs.ui_size))
            .fold(0., f32::max);
        let width = self.tool_width(window);
        let card = self
            .overview_card()
            .w(px(width))
            .id(SharedString::from(format!(
                "network-card-{}",
                owner.session
            )))
            .cursor_pointer()
            .px(px(theme::SPACE_PANEL))
            .py(px(theme::SPACE_CONTROL))
            .gap(px(theme::SPACE_CONTROL))
            .line_height(relative(1.5))
            .hover(|row| row.bg(p.background).border_color(p.muted.opacity(0.45)))
            .active(|row| row.bg(p.selected))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.show_modal(
                    Modal::SystemTools {
                        owner,
                        page: SystemPage::Ports,
                    },
                    window,
                    cx,
                );
                cx.stop_propagation();
            }))
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .w_full()
                    .gap(px(theme::SPACE_CONTROL))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.t("network")),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(theme::SPACE_CONTROL))
                            .text_color(p.muted)
                            .child(self.t("recent_three_minutes"))
                            .child(
                                gpui_component::Icon::new(IconName::ChevronRight)
                                    .size(px(self.prefs.ui_size))
                                    .text_color(p.muted),
                            ),
                    ),
            )
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .w_full()
                    .gap(px(theme::SPACE_PANEL))
                    .child(self.network_speed(
                        "download",
                        latest.and_then(|point| point.received),
                        p.accent,
                        IconName::ArrowDown,
                    ))
                    .child(self.network_speed(
                        "upload",
                        latest.and_then(|point| point.sent),
                        p.network_sent,
                        IconName::ArrowUp,
                    )),
            )
            .child(
                div().flex().flex_col().gap(px(theme::SPACE_SMALL)).child(
                    div()
                        .id(("network-trend", pane.owner.session.as_u128() as u64))
                        .relative()
                        .w_full()
                        .h(px(56.))
                        .flex_shrink_0()
                        .child(self.trend_chart(
                            vec![(rx, p.accent), (tx, p.network_sent)],
                            maximum,
                            cx,
                        ))
                        .when(empty, |chart| {
                            chart.child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_color(p.muted)
                                    .child(self.t("sampling")),
                            )
                        }),
                ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(theme::SPACE_CONTROL))
                    .pt(px(theme::SPACE_CONTROL))
                    .border_t_1()
                    .border_color(p.border)
                    .children(traffic_rows.into_iter().map(|(name, received, sent)| {
                        div()
                            .flex()
                            .w_full()
                            .min_w_0()
                            .items_start()
                            .gap(px(theme::SPACE_CONTROL))
                            .text_color(p.muted)
                            .child(div().flex_1().min_w_0().whitespace_normal().child(name))
                            .child(
                                div()
                                    .flex_none()
                                    .w(px(received_width))
                                    .text_right()
                                    .whitespace_nowrap()
                                    .child(received),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .w(px(sent_width))
                                    .text_right()
                                    .whitespace_nowrap()
                                    .child(sent),
                            )
                    })),
            );
        self.measure_overview_card("network", card, cx)
    }
    /// Percentages describe current CPU, memory and disk use; only network retains a time-series plot.
    fn render_overview(
        &self,
        pane: &Pane,
        sample: &Sample,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = theme::Palette::new(self.prefs.theme);
        let disk_percent = sample
            .disks
            .iter()
            .find(|disk| disk.mount == "/")
            .or(sample.disks.first())
            .filter(|disk| disk.total > 0)
            .map(|disk| 100. * disk.used as f64 / disk.total as f64);
        // A solid hairline so sections read clearly on the surface background.
        // Use the border mechanism like every other separator in this panel: a
        // percentage-width background div collapses to zero width inside the
        // panel's measured chain and paints nothing (verified pixel-level),
        // while border_t on a stretched child reliably draws the full width.
        // flex_shrink_0 keeps the 1px from collapsing when the overview is
        // taller than the panel viewport.
        let divider = || div().flex_shrink_0().border_t_1().border_color(p.border);
        div()
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .child(self.percentage_card(
                pane.owner,
                ResourceKind::Cpu,
                "CPU",
                sample.cpu.first().and_then(|cpu| cpu.percent),
                format!(
                    "{} {}",
                    sample.cpu.iter().filter(|cpu| cpu.name != "cpu").count(),
                    self.t("cores_unit")
                ),
                window,
                cx,
            ))
            .child(divider())
            .child(self.memory_card(pane.owner, sample, window, cx))
            .child(divider())
            .child(
                self.percentage_card(
                    pane.owner,
                    ResourceKind::Disk,
                    self.t("disk"),
                    disk_percent,
                    sample
                        .disks
                        .iter()
                        .find(|disk| disk.mount == "/")
                        .or(sample.disks.first())
                        .map(|disk| self.capacity_summary(disk.used, disk.total))
                        .unwrap_or_else(|| "—".into()),
                    window,
                    cx,
                ),
            )
            .child(divider())
            .child(self.network_card(pane, sample, window, cx))
            .into_any_element()
    }
    /// The panel shows overview only: nothing above the CPU module. Sampling is
    /// automatic on connect and while visible; a disconnect clears all modules.
    pub(super) fn render_monitor(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = theme::Palette::new(self.prefs.theme);
        let Some(pane) = self.active_pane() else {
            return div().into_any_element();
        };
        let owner = pane.owner;
        let body = div().flex().flex_col().flex_1().min_h_0();
        let Some(sample) = &pane.monitor else {
            return body.into_any_element();
        };
        let mut rows = vec![self.render_overview(pane, sample, window, cx)];
        if let Some(error) = &pane.monitor_error {
            rows.push(
                div()
                    .py_2()
                    .text_color(p.error)
                    .whitespace_normal()
                    .child(error.clone())
                    .into_any_element(),
            );
        }
        for (key, error) in &sample.errors {
            if key == "system" || (key != "ports" && key != "processes") {
                rows.push(
                    div()
                        .py_2()
                        .text_color(p.error)
                        .whitespace_normal()
                        .child(format!("{}: {error}", self.t("unavailable")))
                        .into_any_element(),
                );
            }
        }
        body.child(
            div()
                .id(("monitor-details", owner.session.as_u128() as u64))
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&pane.monitor_scroll[0])
                .children(rows),
        )
        .into_any_element()
    }
    /// QA-only bounds for the visible process table; no production layout cost.
    fn measure_process_region(&self, name: &'static str, _cx: &mut Context<Self>) -> AnyElement {
        #[cfg(debug_assertions)]
        if self.qa.is_some() {
            let view = _cx.entity();
            return canvas(
                move |bounds, _, cx| {
                    view.update(cx, |this, _| {
                        if let Some(qa) = &mut this.qa {
                            qa.process_geometry.insert(name, bounds);
                        }
                    });
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0()
            .into_any_element();
        }
        div().size_0().into_any_element()
    }

    /// Scan processes as aligned, sortable columns; only the result region scrolls.
    fn render_process_list(&self, owner: Owner, cx: &mut Context<Self>) -> AnyElement {
        let p = theme::Palette::new(self.prefs.theme);
        let Some(pane) = self.pane(owner) else {
            return div().into_any_element();
        };
        let sample = pane.monitor.as_ref();
        let section_error = sample.and_then(|s| s.errors.get("processes"));
        let query = pane.process_filter.read(cx).value();
        let mut processes: Vec<&Process> = sample
            .filter(|_| section_error.is_none())
            .map(|s| {
                s.processes
                    .iter()
                    .filter(|process| process.matches_command(&query))
                    .collect()
            })
            .unwrap_or_default();
        processes.sort_by(|a, b| {
            let order = match pane.process_sort {
                ProcessSort::Pid => a.pid.cmp(&b.pid),
                ProcessSort::Name => a.command.cmp(&b.command),
                ProcessSort::User => a.user.cmp(&b.user),
                ProcessSort::Cpu => a.cpu.total_cmp(&b.cpu),
                ProcessSort::Memory => a.rss.cmp(&b.rss),
            };
            (if pane.process_descending {
                order.reverse()
            } else {
                order
            })
            .then_with(|| a.pid.cmp(&b.pid))
        });
        let total = sample.map_or(0, |s| s.processes.len());
        let refreshing = pane.monitor_request.is_some();
        let status = if pane.state != ConnectionState::Connected || section_error.is_some() {
            self.t("unavailable").to_string()
        } else if pane.monitor_error.is_some() {
            self.t(if sample.is_some() {
                "process_old_data"
            } else {
                "unavailable"
            })
            .to_string()
        } else if sample.is_none() {
            self.t(if refreshing {
                "loading"
            } else {
                "refresh_to_sample"
            })
            .to_string()
        } else {
            format!(
                "{} / {} {}",
                processes.len(),
                total,
                self.t("process_count")
            )
        };
        let error_color = pane.state != ConnectionState::Connected
            || section_error.is_some()
            || pane.monitor_error.is_some();
        let columns = [
            (ProcessSort::Pid, "PID", 80.),
            (ProcessSort::Name, self.t("process_command"), 0.),
            (ProcessSort::User, self.t("username"), 80.),
            (ProcessSort::Cpu, "CPU", 72.),
            (ProcessSort::Memory, self.t("memory"), 88.),
        ];
        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .relative()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(theme::SPACE_CONTROL))
                    .px(px(theme::SPACE_PANEL))
                    .pt(px(theme::SPACE_PANEL))
                    .pb(px(theme::SPACE_CONTROL))
                    .child(self.measure_process_region("toolbar", cx))
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(theme::SPACE_CONTROL))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(self.input_box(&pane.process_filter)),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(theme::SPACE_CONTROL))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .text_color(if error_color { p.error } else { p.muted })
                                    .child(status),
                            ),
                    ),
            )
            .child(
                div()
                    .relative()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(theme::SPACE_CONTROL))
                    .px(px(theme::SPACE_PANEL))
                    .py(px(theme::SPACE_SMALL))
                    .border_t_1()
                    .border_b_1()
                    .border_color(p.border)
                    .bg(p.background)
                    .child(self.measure_process_region("columns", cx))
                    .children(columns.into_iter().enumerate().map(
                        |(index, (sort, label, width))| {
                            let active = pane.process_sort == sort;
                            let caption = if active {
                                format!(
                                    "{label} {}",
                                    if pane.process_descending {
                                        "↓"
                                    } else {
                                        "↑"
                                    }
                                )
                            } else {
                                label.to_string()
                            };
                            div()
                                .relative()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .when(index == 1, |cell| cell.flex_1())
                                .when(index != 1, |cell| cell.w(px(width)).flex_none())
                                .when(
                                    matches!(
                                        sort,
                                        ProcessSort::Pid | ProcessSort::Cpu | ProcessSort::Memory
                                    ),
                                    |cell| cell.justify_end(),
                                )
                                .child(self.measure_process_region(
                                    match index {
                                        0 => "header_pid",
                                        1 => "header_name",
                                        2 => "header_user",
                                        3 => "header_cpu",
                                        _ => "header_memory",
                                    },
                                    cx,
                                ))
                                .child(
                                    self.button(("sort-process", index), caption)
                                        .ghost()
                                        .chromeless()
                                        .h(px(24.))
                                        .selected(active)
                                        .tooltip(self.t("process_sort"))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(pane) = this.pane_mut(owner) {
                                                if pane.process_sort == sort {
                                                    pane.process_descending =
                                                        !pane.process_descending;
                                                } else {
                                                    pane.process_sort = sort;
                                                    pane.process_descending = matches!(
                                                        sort,
                                                        ProcessSort::Cpu | ProcessSort::Memory
                                                    );
                                                }
                                            }
                                            cx.notify();
                                        })),
                                )
                        },
                    ))
                    .child(
                        div()
                            .w(px(48.))
                            .flex_none()
                            .text_color(p.muted)
                            .child(self.t("process_state")),
                    )
                    .child(div().w(px(20.)).flex_none()),
            )
            .child(
                div()
                    .relative()
                    .w_full()
                    .min_w_0()
                    // Keep the list viewport and its position indicator in a
                    // non-scrolling flex shell. A bare block here lets GPUI
                    // resolve the percent-height child against the content,
                    // allowing the absolute scrollbar to move with rows.
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(
                        div()
                            .relative()
                            .id(("system-page-details", owner.session.as_u128() as u64))
                            .w_full()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .track_scroll(&pane.monitor_scroll[1])
                            .child(self.measure_process_region("viewport", cx))
                            .child(
                                div()
                                    .w_full()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .px(px(theme::SPACE_PANEL))
                                    .children(pane.process_attempts.iter().rev().take(3).map(
                                        |attempt| {
                                            div()
                                                .min_w_0()
                                                .whitespace_normal()
                                                .py(px(theme::SPACE_SMALL))
                                                .text_color(
                                                    if matches!(
                                                        attempt.phase,
                                                        ProcessPhase::Denied
                                                            | ProcessPhase::Changed
                                                            | ProcessPhase::Unknown
                                                    ) {
                                                        p.error
                                                    } else {
                                                        p.muted
                                                    },
                                                )
                                                .child(format!(
                                                    "PID {} · {}",
                                                    attempt.identity.pid,
                                                    self.t(attempt.phase.key())
                                                ))
                                                .when_some(attempt.error.as_ref(), |line, error| {
                                                    line.child(format!(": {error}"))
                                                })
                                        },
                                    ))
                                    .when(pane.state != ConnectionState::Connected, |list| {
                                        list.child(
                                            div()
                                                .min_w_0()
                                                .whitespace_normal()
                                                .py(px(theme::SPACE_CONTROL))
                                                .text_color(p.error)
                                                .child(self.t("process_offline")),
                                        )
                                    })
                                    .when_some(section_error, |list, error| {
                                        list.child(
                                            div()
                                                .min_w_0()
                                                .whitespace_normal()
                                                .py(px(theme::SPACE_CONTROL))
                                                .text_color(p.error)
                                                .child(format!(
                                                    "{}: {error}",
                                                    self.t("unavailable")
                                                )),
                                        )
                                    })
                                    .when(section_error.is_none(), |list| {
                                        list.when_some(
                                            pane.monitor_error.as_ref(),
                                            |list, error| {
                                                list.child(
                                                    div()
                                                        .min_w_0()
                                                        .whitespace_normal()
                                                        .py(px(theme::SPACE_CONTROL))
                                                        .text_color(p.error)
                                                        .child(error.clone()),
                                                )
                                            },
                                        )
                                    })
                                    .when(
                                        processes.is_empty() && sample.is_some() && !error_color,
                                        |list| {
                                            list.child(
                                                div()
                                                    .py(px(theme::SPACE_CONTROL))
                                                    .text_color(p.muted)
                                                    .child(self.t("no_matches")),
                                            )
                                        },
                                    )
                                    .children(processes.into_iter().enumerate().map(
                                        |(index, process)| {
                                            let details = process.clone();
                                            let command_tooltip = process.command.clone();
                                            div()
                                                .relative()
                                                .w_full()
                                                .min_w_0()
                                                .flex()
                                                .items_center()
                                                .gap(px(theme::SPACE_CONTROL))
                                                .py(px(theme::SPACE_SMALL))
                                                .border_b_1()
                                                .border_color(p.border)
                                                .when(index == 0, |row| {
                                                    row.child(
                                                        self.measure_process_region(
                                                            "first_row",
                                                            cx,
                                                        ),
                                                    )
                                                })
                                                .child(
                                                    div()
                                                        .relative()
                                                        .w(px(80.))
                                                        .flex_none()
                                                        .min_w_0()
                                                        .text_right()
                                                        .whitespace_nowrap()
                                                        .font_family(
                                                            self.prefs.terminal_font.clone(),
                                                        )
                                                        .child(
                                                            self.measure_process_region(
                                                                "row_pid", cx,
                                                            ),
                                                        )
                                                        .child(process.pid.to_string()),
                                                )
                                                .child(
                                                    div()
                                                        .id(("process-command", process.pid as u64))
                                                        .tooltip(move |window, cx| {
                                                            gpui_component::tooltip::Tooltip::new(
                                                                command_tooltip.clone(),
                                                            )
                                                            .build(window, cx)
                                                        })
                                                        .relative()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .overflow_hidden()
                                                        .text_ellipsis()
                                                        .whitespace_nowrap()
                                                        .child(
                                                            self.measure_process_region(
                                                                "row_name", cx,
                                                            ),
                                                        )
                                                        .child(process.command.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .relative()
                                                        .w(px(80.))
                                                        .flex_none()
                                                        .min_w_0()
                                                        .overflow_hidden()
                                                        .text_ellipsis()
                                                        .whitespace_nowrap()
                                                        .child(
                                                            self.measure_process_region(
                                                                "row_user", cx,
                                                            ),
                                                        )
                                                        .child(process.user.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .relative()
                                                        .w(px(72.))
                                                        .flex_none()
                                                        .text_right()
                                                        .whitespace_nowrap()
                                                        .child(
                                                            self.measure_process_region(
                                                                "row_cpu", cx,
                                                            ),
                                                        )
                                                        .child(if process.cpu.is_finite() {
                                                            format!("{:.1}%", process.cpu)
                                                        } else {
                                                            "—".into()
                                                        }),
                                                )
                                                .child(
                                                    div()
                                                        .relative()
                                                        .w(px(88.))
                                                        .flex_none()
                                                        .text_right()
                                                        .whitespace_nowrap()
                                                        .child(self.measure_process_region(
                                                            "row_memory",
                                                            cx,
                                                        ))
                                                        .child(bytes(process.rss)),
                                                )
                                                .child(
                                                    div()
                                                        .relative()
                                                        .w(px(48.))
                                                        .flex_none()
                                                        .overflow_hidden()
                                                        .text_ellipsis()
                                                        .whitespace_nowrap()
                                                        .text_color(p.muted)
                                                        .child(self.measure_process_region(
                                                            "row_state",
                                                            cx,
                                                        ))
                                                        .child(if process.state.is_empty() {
                                                            "—".into()
                                                        } else {
                                                            process.state.clone()
                                                        }),
                                                )
                                                .child(
                                                    self.button(("process-details", index), "")
                                                        .icon(IconName::ChevronRight)
                                                        .ghost()
                                                        .chromeless()
                                                        .w(px(20.))
                                                        .h(px(20.))
                                                        .p_0()
                                                        .tooltip(self.t("process_details"))
                                                        .on_click(cx.listener(
                                                            move |this, _, w, cx| {
                                                                this.show_process_details(
                                                                    owner,
                                                                    details.clone(),
                                                                    w,
                                                                    cx,
                                                                )
                                                            },
                                                        )),
                                                )
                                        },
                                    )),
                            ),
                    )
                    // Match GPUI's scrollable layer: the indicator lives in
                    // an absolute full-size sibling of the scrolling area.
                    .child(div().absolute().inset_0().child(self.overlay_scrollbar(
                        "process-list-scrollbar",
                        pane.monitor_scroll[1].clone(),
                        Resize::ModalScroll,
                        cx,
                    ))),
            )
            .into_any_element()
    }
    /// Processes and ports render inside their dialog from the same pane state.
    pub(super) fn render_system_page(
        &self,
        owner: Owner,
        page: SystemPage,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if page == SystemPage::Ports {
            return self.render_ports(owner, cx);
        }
        if page == SystemPage::Processes {
            return self.render_process_list(owner, cx);
        }
        let p = theme::Palette::new(self.prefs.theme);
        let Some(pane) = self.pane(owner) else {
            return div().into_any_element();
        };
        let page_index = match page {
            SystemPage::Overview => 0,
            SystemPage::Processes => 1,
            SystemPage::Ports => 2,
        };
        let mut body = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap(px(theme::SPACE_CONTROL))
            .p(px(theme::SPACE_PANEL))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(theme::SPACE_SMALL))
                    .child(div().flex_1())
                    .child({
                        // Keep the button in place while loading: only its icon
                        // swaps for the spinner, so the row width never shifts.
                        let refreshing = pane.monitor_request.is_some();
                        let refresh = self
                            .button("refresh-monitor-page", "")
                            .svg_icon("icons/refresh-cw.svg")
                            .ghost()
                            .h(px(self.controls_height()))
                            .tooltip(self.t("refresh"))
                            .disabled(pane.state != ConnectionState::Connected || refreshing);
                        let refresh = if refreshing {
                            refresh.icon_element(self.loading_spinner())
                        } else {
                            refresh
                        };
                        refresh
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.refresh_monitor(owner, cx)),
                            )
                            .into_any_element()
                    }),
            )
            .when_some(pane.monitor_error.clone(), |d, error| {
                d.child(div().text_color(p.error).child(error))
            })
            .child(self.input_box(if page == SystemPage::Processes {
                &pane.process_filter
            } else {
                &pane.port_filter
            }));
        let Some(sample) = &pane.monitor else {
            return body
                .child(div().py_2().text_color(p.muted).child(self.t(
                    if pane.monitor_request.is_some() {
                        "loading"
                    } else {
                        "refresh_to_sample"
                    },
                )))
                .into_any_element();
        };
        let updated = chrono::DateTime::from_timestamp(sample.timestamp, 0)
            .map(|date| {
                date.with_timezone(&chrono::Local)
                    .format("%H:%M:%S")
                    .to_string()
            })
            .unwrap_or_default();
        body = body.child(
            div()
                .text_color(p.muted)
                .child(format!("{} {updated}", self.t("updated"))),
        );
        let mut rows = Vec::new();
        match page {
            SystemPage::Overview => {}
            SystemPage::Processes => {}
            SystemPage::Ports => {
                let query = pane.port_filter.read(cx).value().to_lowercase();
                for port in sample.ports.iter().filter(|p| {
                    format!("{} {} {} {}", p.local, p.protocol, p.peer, p.process)
                        .to_lowercase()
                        .contains(&query)
                }) {
                    rows.push(
                        div()
                            .py(px(8.))
                            .border_b_1()
                            .border_color(p.border)
                            .child(format!("{} · {}", port.protocol, port.local))
                            .child(
                                div()
                                    .text_color(p.muted)
                                    .whitespace_normal()
                                    .child(format!("{} · {}", port.state, port.process)),
                            )
                            .into_any_element(),
                    );
                }
                if rows.is_empty() {
                    rows.push(
                        div()
                            .py_2()
                            .text_color(p.muted)
                            .child(self.t("no_matches"))
                            .into_any_element(),
                    );
                }
            }
        }
        for (key, error) in &sample.errors {
            if key == "system"
                || match page {
                    SystemPage::Overview => key != "ports" && key != "processes",
                    SystemPage::Processes => key == "processes",
                    SystemPage::Ports => key == "ports",
                }
            {
                rows.push(
                    div()
                        .py_2()
                        .text_color(p.error)
                        .whitespace_normal()
                        .child(format!("{}: {error}", self.t("unavailable")))
                        .into_any_element(),
                );
            }
        }
        body.child(
            div()
                .id(("system-page-details", owner.session.as_u128() as u64))
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&pane.monitor_scroll[page_index])
                .children(rows),
        )
        .into_any_element()
    }
}
