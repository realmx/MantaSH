//! Isolated, non-actionable process-detail fixtures for native GPUI checks.
use super::*;
use crate::{
    monitor::{Process, Sample},
    processes::{Details, Identity},
};

const BOOT: &str = "59372983-86ce-4bae-9f6a-555ef0db8dc8";

fn fixture_process() -> Process {
    Process {
        user: "qa-user".into(),
        started_at: Some(1_700_000_000),
        identity: Some(Identity {
            pid: 4242,
            start_ticks: 900,
            boot_id: BOOT.into(),
        }),
        pid: 4242,
        parent: 99,
        cpu: 12.5,
        memory: 2.4,
        rss: 2_097_152,
        state: "S".into(),
        command: "qa-worker".into(),
    }
}

fn fixture_details(long: bool) -> Details {
    Details {
        command: if long {
            format!(
                "/usr/bin/qa-worker {}",
                "--sample=中文-long-token".repeat(70)
            )
        } else {
            "/usr/bin/qa-worker --sample".into()
        },
        status: (0..100)
            .map(|index| format!("QaField{index}:\tvalue\n"))
            .collect(),
    }
}

impl Workbench {
    /// Preview fixture never establishes an SSH connection or enables signal controls.
    pub(super) fn qa_process_fixture(
        &mut self,
        mode: &str,
        long: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(
            mode,
            "loading"
                | "ready"
                | "error"
                | "missing"
                | "gone"
                | "reused"
                | "reboot"
                | "sample_error"
                | "offline"
        ) {
            return;
        }
        let existing = self
            .qa
            .as_ref()
            .and_then(|qa| qa.process_fixture_owner)
            .filter(|owner| self.pane(*owner).is_some());
        let owner = if let Some(owner) = existing {
            owner
        } else {
            let profile = Profile {
                id: Id::new_v4(),
                name: "Process preview (QA)".into(),
                host: "127.0.0.1".into(),
                port: 1,
                username: "qa".into(),
            };
            let pane = self.create_pane(
                SessionSpec::Ssh {
                    profile,
                    encoding: crate::encoding::Encoding::Utf8,
                },
                false,
                window,
                cx,
            );
            let owner = pane.owner;
            self.tabs.push(Tab {
                id: Id::new_v4(),
                layout: PaneLayout::single(owner.session),
                panes: vec![pane],
                active: 0,
                scroll: ScrollHandle::new(),
            });
            if let Some(qa) = &mut self.qa {
                qa.process_fixture_owner = Some(owner);
            }
            owner
        };
        if let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.panes.iter().any(|pane| pane.owner == owner))
        {
            self.active = index;
        }
        self.modal = None;
        self.modal_stack.clear();
        self.modal_confirm_return = None;
        let mut process = fixture_process();
        if mode == "missing" {
            process.identity = None;
        }
        self.qa_process_sample(mode);
        if let Some(pane) = self.pane_mut(owner) {
            pane.process_attempts.clear();
        }
        let result = match mode {
            "loading" => None,
            "error" => Some(Err("QA detail read failed".into())),
            "missing" => Some(Err(self.t("process_identity_missing").into())),
            _ => Some(Ok(fixture_details(long))),
        };
        self.show_modal(
            Modal::SystemTools {
                owner,
                page: SystemPage::Processes,
            },
            window,
            cx,
        );
        self.show_modal(
            Modal::ProcessDetails {
                owner,
                process,
                request: Id::new_v4(),
                result,
                refreshing: mode == "loading",
                refresh_error: None,
                raw_expanded: false,
                command_expanded: false,
                preview: true,
            },
            window,
            cx,
        );
    }

    /// Change only the synthetic sample, never a production process or transport.
    pub(super) fn qa_process_sample(&mut self, mode: &str) {
        let Some(owner) = self.qa.as_ref().and_then(|qa| qa.process_fixture_owner) else {
            return;
        };
        let Some(pane) = self.pane_mut(owner) else {
            return;
        };
        pane.state = if mode == "offline" {
            ConnectionState::Disconnected
        } else {
            ConnectionState::Connected
        };
        let mut process = fixture_process();
        let mut sample = Sample {
            boot_id: Some(BOOT.into()),
            system: "Linux".into(),
            timestamp: chrono::Utc::now().timestamp(),
            ..Default::default()
        };
        sample.processes.extend((0..40).map(|index| {
            let mut row = fixture_process();
            row.pid = 5000 + index;
            row.command = format!("qa-helper-{index}");
            row.identity = Some(Identity {
                pid: row.pid,
                start_ticks: 1000 + u64::from(index),
                boot_id: BOOT.into(),
            });
            row
        }));
        if mode != "gone" {
            sample.processes.push(process.clone());
        }
        match mode {
            "reused" => {
                process.identity.as_mut().unwrap().start_ticks += 1;
                sample.processes = vec![process];
            }
            "reboot" => sample.boot_id = Some("81283e7a-8d63-4097-81e0-c508c17235ef".into()),
            "sample_error" => {
                sample
                    .errors
                    .insert("processes".into(), "QA sampling failed".into());
            }
            _ => {}
        }
        pane.monitor = Some(sample);
        pane.monitor_error = None;
        pane.last_sample = if mode == "stale" {
            Instant::now() - Duration::from_secs(12)
        } else {
            Instant::now()
        };
    }

    /// Inject a bounded result using the same owner/request check as a real reply.
    pub(super) fn qa_process_reply(
        &mut self,
        stale: bool,
        error: bool,
        long: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(Modal::ProcessDetails {
            owner,
            request,
            preview: true,
            ..
        }) = &self.modal
        else {
            return;
        };
        let owner = *owner;
        let request = if stale { Id::new_v4() } else { *request };
        let result = if error {
            Err("QA refresh failed".into())
        } else {
            Ok(fixture_details(long))
        };
        if self.apply_process_details_result(owner, request, result) {
            cx.notify();
        }
    }

    pub(super) fn qa_process_attempt(&mut self, phase: &str, cx: &mut Context<Self>) {
        let Some(owner) = self.qa.as_ref().and_then(|qa| qa.process_fixture_owner) else {
            return;
        };
        let Some(Modal::ProcessDetails {
            process,
            preview: true,
            ..
        }) = &self.modal
        else {
            return;
        };
        let Some(identity) = process.identity.clone() else {
            return;
        };
        let phase = match phase {
            "sending" => system::ProcessPhase::Sending,
            "sent" => system::ProcessPhase::Sent,
            "gone" => system::ProcessPhase::Gone,
            "still_running" => system::ProcessPhase::StillRunning,
            "denied" => system::ProcessPhase::Denied,
            "changed" => system::ProcessPhase::Changed,
            "unknown" => system::ProcessPhase::Unknown,
            _ => return,
        };
        if let Some(pane) = self.pane_mut(owner) {
            pane.process_attempts.clear();
            pane.process_attempts.push(system::ProcessAttempt {
                request: Id::new_v4(),
                identity,
                phase,
                changed: Instant::now(),
                error: None,
            });
            cx.notify();
        }
    }

    /// Open the real confirmation renderer, but its preview parent blocks every signal route.
    pub(super) fn qa_process_confirm(
        &mut self,
        force: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Modal::ProcessDetails {
            owner,
            process,
            preview: true,
            ..
        }) = &self.modal
        else {
            return;
        };
        let owner = *owner;
        let process = process.clone();
        let action = if force {
            crate::processes::Action::Force
        } else {
            crate::processes::Action::Terminate
        };
        self.show_modal(
            Modal::ProcessConfirm {
                owner,
                process,
                action,
                host: "127.0.0.1:1".into(),
            },
            window,
            cx,
        );
    }

    /// Change only the fixture list's view state, leaving the selected identity untouched.
    pub(super) fn qa_process_list_state(
        &mut self,
        query: String,
        y: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(owner) = self.qa.as_ref().and_then(|qa| qa.process_fixture_owner) else {
            return;
        };
        if !matches!(&self.modal, Some(Modal::SystemTools { owner: current, page: SystemPage::Processes }) if *current == owner)
        {
            return;
        }
        let Some(pane) = self.pane(owner) else {
            return;
        };
        let filter = pane.process_filter.clone();
        filter.update(cx, |input, cx| input.set_value(query, window, cx));
        if let Some(pane) = self.pane_mut(owner) {
            pane.process_sort = system::ProcessSort::Cpu;
            pane.process_descending = true;
            pane.monitor_scroll[1].set_offset(point(px(0.), px(y)));
            cx.notify();
        }
    }

    pub(super) fn qa_reopen_process_preview(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(owner) = self.qa.as_ref().and_then(|qa| qa.process_fixture_owner) else {
            return;
        };
        if !matches!(&self.modal, Some(Modal::SystemTools { owner: current, page: SystemPage::Processes }) if *current == owner)
        {
            return;
        }
        self.show_modal(
            Modal::ProcessDetails {
                owner,
                process: fixture_process(),
                request: Id::new_v4(),
                result: Some(Ok(fixture_details(false))),
                refreshing: false,
                refresh_error: None,
                raw_expanded: false,
                command_expanded: false,
                preview: true,
            },
            window,
            cx,
        );
    }

    /// Resize only an isolated synthetic Linux sample while the process list is open.
    pub(super) fn qa_process_list_fixture(&mut self, count: usize, cx: &mut Context<Self>) {
        if count > 120 {
            return;
        }
        let Some(owner) = self.qa.as_ref().and_then(|qa| qa.process_fixture_owner) else {
            return;
        };
        if !matches!(&self.modal, Some(Modal::SystemTools { owner: current, page: SystemPage::Processes }) if *current == owner)
        {
            return;
        }
        let Some(pane) = self.pane_mut(owner) else {
            return;
        };
        let Some(sample) = pane.monitor.as_mut() else {
            return;
        };
        sample.processes = (0..count)
            .map(|index| {
                let mut process = fixture_process();
                process.pid = 50_000 + index as u32;
                process.command = format!("qa-helper-{index}");
                process.cpu = (count - index) as f64 / 10.;
                process.rss = 1_048_576 * (index as u64 + 1);
                process.identity = Some(Identity {
                    pid: process.pid,
                    start_ticks: 1000 + index as u64,
                    boot_id: BOOT.into(),
                });
                process
            })
            .collect();
        pane.monitor_error = None;
        pane.last_sample = Instant::now();
        cx.notify();
    }
    pub(super) fn qa_process_list_snapshot(&self, cx: &App) -> serde_json::Value {
        let Some(owner) = self.qa.as_ref().and_then(|qa| qa.process_fixture_owner) else {
            return serde_json::Value::Null;
        };
        let Some(pane) = self.pane(owner) else {
            return serde_json::Value::Null;
        };
        serde_json::json!({ "query":pane.process_filter.read(cx).value(),
            "sort":match pane.process_sort {
                system::ProcessSort::Pid => "pid", system::ProcessSort::Name => "name",
                system::ProcessSort::User => "user", system::ProcessSort::Cpu => "cpu",
                system::ProcessSort::Memory => "memory",
            }, "descending":pane.process_descending,
            "scroll_y":f32::from(pane.monitor_scroll[1].offset().y),
            "max_y":f32::from(pane.monitor_scroll[1].max_offset().height),
            "max_x":f32::from(pane.monitor_scroll[1].max_offset().width),
            "total":pane.monitor.as_ref().map_or(0, |sample| sample.processes.len()),
            "bounds":Self::qa_bounds(pane.monitor_scroll[1].bounds()),
            "geometry":self.qa.as_ref().map(|qa| qa.process_geometry.iter()
                .map(|(name, bounds)| (*name, Self::qa_bounds(*bounds)))
                .collect::<std::collections::HashMap<_, _>>()),
        })
    }

    pub(super) fn qa_process_snapshot(&self) -> serde_json::Value {
        let details = match &self.modal {
            Some(modal @ Modal::ProcessDetails { .. }) => Some(modal),
            Some(Modal::ProcessConfirm { .. }) => {
                self.modal_confirm_return.as_ref().map(|frame| &frame.modal)
            }
            _ => None,
        };
        let Some(Modal::ProcessDetails {
            owner,
            process,
            request,
            result,
            refreshing,
            refresh_error,
            raw_expanded,
            command_expanded,
            preview,
        }) = details
        else {
            return serde_json::Value::Null;
        };
        let attempt = process.identity.as_ref().and_then(|identity| {
            self.pane(*owner).and_then(|pane| {
                pane.process_attempts
                    .iter()
                    .rev()
                    .find(|attempt| &attempt.identity == identity)
            })
        });
        serde_json::json!({
            "owner": owner.session, "request": request, "pid": process.pid,
            "preview": preview, "refreshing": refreshing, "raw_expanded": raw_expanded,
            "command_expanded": command_expanded,
            "long_command": result.as_ref().and_then(|r| r.as_ref().ok())
                .is_some_and(|d| d.command.chars().count() > 160),
            "target_reason": self.process_target(*owner, process).err(),
            "action_reason": self.process_action_reason(*owner, process),
            "signal_enabled": self.can_signal(*owner, process),
            "command_len": result.as_ref().and_then(|r| r.as_ref().ok()).map(|d| d.command.len()),
            "raw_len": result.as_ref().and_then(|r| r.as_ref().ok()).map(|d| d.status.len()),
            "read_error": result.as_ref().and_then(|r| r.as_ref().err()),
            "refresh_error": refresh_error,
            "attempt": attempt.map(|attempt| attempt.phase.key()),
            "metrics": self.process_target(*owner, process).ok().map(|p| serde_json::json!({
                "user":p.user,"parent":p.parent,"state":p.state,"cpu":p.cpu,"rss":p.rss,
            })),
            "footer_bounds": self.qa.as_ref().and_then(|qa| qa.process_footer_bounds.map(Self::qa_bounds)),
            "confirm": match &self.modal { Some(Modal::ProcessConfirm { action, host, .. }) =>
                Some(serde_json::json!({"host":host,"action":action})), _ => None },
        })
    }
}
