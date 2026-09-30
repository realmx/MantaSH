//! Application update UI. Every background result is tied to one request/consent.
use super::*;
use crate::update::{PreparedUpdate, Release, Updater};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Idle,
    Checking,
    NoUpdate,
    CheckFailed,
    Available,
    Downloading,
    Preparing,
    Ready,
    Closing,
    Failed,
}

pub(super) struct Updates {
    phase: Phase,
    request: Option<Id>,
    release: Option<Release>,
    prepared: Option<PreparedUpdate>,
    cancel: CancellationToken,
    progress: Arc<(AtomicU64, AtomicU64)>,
    displayed_progress: (u64, u64),
    next_check: Instant,
    declined: HashSet<String>,
    error: Option<String>,
    manual_check: bool,
}

impl Default for Updates {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            request: None,
            release: None,
            prepared: None,
            cancel: CancellationToken::new(),
            progress: Arc::new((AtomicU64::new(0), AtomicU64::new(0))),
            displayed_progress: (0, 0),
            next_check: Instant::now() + Duration::from_secs(3),
            declined: HashSet::new(),
            error: None,
            manual_check: false,
        }
    }
}

impl Updates {
    /// An explicit request can adopt an automatic check without starting another HTTP request.
    fn begin_check(&mut self, manual: bool) -> Option<Id> {
        if self.phase == Phase::Checking {
            self.manual_check |= manual;
            return None;
        }
        if !matches!(
            self.phase,
            Phase::Idle | Phase::Available | Phase::NoUpdate | Phase::CheckFailed
        ) {
            return None;
        }
        let request = Id::new_v4();
        self.request = Some(request);
        self.phase = Phase::Checking;
        self.manual_check = manual;
        self.release = None;
        self.error = None;
        self.next_check = Instant::now() + Duration::from_secs(24 * 60 * 60);
        Some(request)
    }

    /// Only explicit checks show empty/error results; explicit intent overrides a previous decline.
    fn finish_check(&mut self, request: Id, result: Result<Option<Release>, String>) -> bool {
        if self.request != Some(request) || self.phase != Phase::Checking {
            return false;
        }
        self.request = None;
        self.phase = Phase::Idle;
        match result {
            Ok(Some(release)) if self.manual_check || !self.declined.contains(&release.version) => {
                self.release = Some(release);
                self.phase = Phase::Available;
            }
            Ok(_) if self.manual_check => self.phase = Phase::NoUpdate,
            Ok(_) => {}
            Err(error) => {
                self.next_check = Instant::now() + Duration::from_secs(60 * 60);
                if self.manual_check {
                    self.phase = Phase::CheckFailed;
                    self.error = Some(error);
                }
            }
        }
        true
    }

    /// Return filesystem-owning staging to the caller for cleanup on a blocking worker.
    fn cancel(&mut self) -> Option<PreparedUpdate> {
        self.cancel.cancel();
        self.request = None;
        if let Some(release) = self.release.take() {
            self.declined.insert(release.version);
        }
        self.phase = Phase::Idle;
        self.error = None;
        self.manual_check = false;
        self.prepared.take()
    }
}

impl Drop for Updates {
    fn drop(&mut self) {
        self.cancel.cancel();
        // Prepared bundles own filesystem cleanup; never perform it in GPUI's destructor.
        if let Some(prepared) = self.prepared.take() {
            std::thread::spawn(move || drop(prepared));
        }
    }
}

impl Workbench {
    #[cfg(debug_assertions)]
    pub(super) fn qa_update_status(&self) -> serde_json::Value {
        serde_json::json!({
            "phase": format!("{:?}", self.updates.phase),
            "version": self.updates.release.as_ref().map(|r| &r.version),
            "request_active": self.updates.request.is_some(),
            "downloaded": self.updates.displayed_progress.0,
            "prepared": self.updates.prepared.is_some(),
            "declined": self.updates.declined,
            "error": self.updates.error,
            "manual": self.updates.manual_check,
        })
    }

    pub(super) fn update_handoff_active(&self) -> bool {
        self.updates.phase == Phase::Closing
    }

    pub(super) fn tick_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.allow_close || self.close_after_save.is_some() {
            return;
        }
        if self.qa.is_none()
            && self.updates.phase == Phase::Idle
            && Instant::now() >= self.updates.next_check
        {
            self.check_updates(window, cx);
        }
        if self.updates.phase == Phase::Available
            && self.modal.is_none()
            && window.is_window_active()
        {
            self.show_modal(Modal::Update, window, cx);
        }
        if self.updates.phase == Phase::Downloading {
            let progress = (
                self.updates.progress.0.load(Ordering::Relaxed),
                self.updates.progress.1.load(Ordering::Relaxed),
            );
            if progress != self.updates.displayed_progress {
                self.updates.displayed_progress = progress;
                cx.notify();
            }
        }
        if self.updates.phase == Phase::Ready && matches!(self.modal, Some(Modal::Update)) {
            self.request_close(dialogs::CloseTarget::Update, window, cx);
        }
    }

    pub(super) fn check_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.start_update_check(false, window, cx);
    }

    pub(super) fn manual_check_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.start_update_check(true, window, cx);
    }

    fn start_update_check(&mut self, manual: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.allow_close
            || self.close_after_save.is_some()
            || (!manual && self.updates.phase != Phase::Idle)
        {
            return;
        }
        let request = self.updates.begin_check(manual);
        if manual && self.updates.phase == Phase::Checking {
            self.show_modal(Modal::Update, window, cx);
        }
        cx.notify();
        let Some(request) = request else { return };
        let task = self
            .backend
            .runtime
            .spawn(async { Updater::new()?.check(crate::APP_VERSION).await });
        cx.spawn_in(window, async move |entity, cx| {
            let result = task
                .await
                .map_err(|error| error.to_string())
                .and_then(|result| result.map_err(|error| format!("{error:#}")));
            let _ = entity.update_in(cx, |this, _, cx| {
                if !this.allow_close && this.updates.finish_check(request, result) {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn check_update_button(&self, cx: &mut Context<Self>) -> controls::Button {
        self.button("check-updates", self.t("check_updates"))
            .disabled(!matches!(
                self.updates.phase,
                Phase::Idle
                    | Phase::Checking
                    | Phase::Available
                    | Phase::NoUpdate
                    | Phase::CheckFailed
            ))
            .when(self.updates.phase == Phase::Checking, |button| {
                button.icon_element(self.loading_spinner())
            })
            .on_click(cx.listener(|this, _, window, cx| this.manual_check_updates(window, cx)))
    }

    pub(super) fn update_dialog_title(&self) -> &'static str {
        self.t(
            if matches!(
                self.updates.phase,
                Phase::Checking | Phase::NoUpdate | Phase::CheckFailed
            ) {
                "check_updates"
            } else {
                "update_title"
            },
        )
    }

    pub(super) fn start_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.modal, Some(Modal::Update))
            || !matches!(self.updates.phase, Phase::Available | Phase::Failed)
        {
            return;
        }
        let Some(release) = self.updates.release.clone() else {
            return;
        };
        let request = Id::new_v4();
        let cancel = CancellationToken::new();
        self.updates.cancel.cancel();
        self.updates.cancel = cancel.clone();
        self.updates.request = Some(request);
        self.updates.phase = Phase::Downloading;
        self.updates.error = None;
        self.updates.displayed_progress = (0, 0);
        let progress = Arc::new((AtomicU64::new(0), AtomicU64::new(0)));
        self.updates.progress = progress.clone();
        let task = self.backend.runtime.spawn(async move {
            Updater::new()?
                .download(release, cancel, move |received, total| {
                    progress.0.store(received, Ordering::Relaxed);
                    progress.1.store(total, Ordering::Relaxed);
                })
                .await
        });
        let runtime = self.backend.runtime.clone();
        cx.spawn_in(window, async move |entity, cx| {
            let mut result = Some(task.await);
            let _ = entity.update_in(cx, |this, window, cx| {
                if this.updates.request != Some(request) || this.allow_close {
                    return;
                }
                match result.take().unwrap() {
                    Ok(Ok(download)) => {
                        this.updates.phase = Phase::Preparing;
                        let task = this
                            .backend
                            .runtime
                            .spawn(crate::update::prepare_install(download));
                        let runtime = this.backend.runtime.clone();
                        cx.spawn_in(window, async move |entity, cx| {
                            let mut result = Some(task.await);
                            let _ = entity.update_in(cx, |this, _, cx| {
                                if this.updates.request != Some(request) || this.allow_close {
                                    return;
                                }
                                match result.take().unwrap() {
                                    Ok(Ok(prepared)) => {
                                        this.updates.prepared = Some(prepared);
                                        this.updates.phase = Phase::Ready;
                                    }
                                    Ok(Err(error)) => this.update_failed(format!("{error:#}")),
                                    Err(error) => this.update_failed(error.to_string()),
                                }
                                cx.notify();
                            });
                            if let Some(result) = result {
                                runtime.spawn_blocking(move || drop(result));
                            }
                        })
                        .detach();
                    }
                    Ok(Err(error)) => this.update_failed(format!("{error:#}")),
                    Err(error) => this.update_failed(error.to_string()),
                }
                cx.notify();
            });
            if let Some(result) = result {
                runtime.spawn_blocking(move || drop(result));
            }
        })
        .detach();
        cx.notify();
    }

    fn update_failed(&mut self, error: String) {
        self.updates.request = None;
        self.updates.phase = Phase::Failed;
        self.updates.error = Some(error);
    }

    /// Cancel consent as well as I/O. A late prepare/download result cannot install anything.
    pub(super) fn cancel_update(&mut self) {
        if let Some(prepared) = self.updates.cancel() {
            self.backend.runtime.spawn_blocking(move || drop(prepared));
        }
        if matches!(self.close_after_save, Some(dialogs::CloseTarget::Update)) {
            self.close_after_save = None;
        }
    }

    /// Called only after the existing dirty-document/active-transfer close guard has passed.
    pub(super) fn finish_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.updates.phase != Phase::Ready || self.updates.prepared.is_none() {
            return;
        }
        self.show_modal(Modal::Update, window, cx);
        self.updates.phase = Phase::Closing;
        self.preferences_dirty = None;
        let task = self
            .backend
            .final_workspace(self.prefs.clone(), self.workspace());
        cx.spawn_in(window, async move |entity, cx| {
            let saved = task.await;
            let _ = entity.update_in(cx, |this, window, cx| {
                if this.updates.phase != Phase::Closing {
                    return;
                }
                if !matches!(saved, Ok(Ok(()))) {
                    this.update_failed(format!("{saved:?}"));
                    if let Some(prepared) = this.updates.prepared.take() {
                        this.backend.runtime.spawn_blocking(move || drop(prepared));
                    }
                    cx.notify();
                    return;
                }
                let Some(prepared) = this.updates.prepared.take() else {
                    return;
                };
                let task = this
                    .backend
                    .runtime
                    .spawn_blocking(move || crate::update::launch_install(prepared));
                cx.spawn_in(window, async move |entity, cx| {
                    let result = task.await;
                    let _ = entity.update_in(cx, |this, _, cx| {
                        match result {
                            Ok(Ok(())) => {
                                this.allow_close = true;
                                #[cfg(target_os = "macos")]
                                if let Some(routing) = &this.native_titlebar {
                                    routing.stop();
                                }
                                this.backend.shutdown();
                                cx.quit();
                            }
                            error => this.update_failed(format!("{error:?}")),
                        }
                        cx.notify();
                    });
                })
                .detach();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn render_update_body(&self) -> AnyElement {
        let version = self
            .updates
            .release
            .as_ref()
            .map(|r| r.version.as_str())
            .unwrap_or("");
        let text = match self.updates.phase {
            Phase::Checking => self.t("update_checking").to_string(),
            Phase::NoUpdate => self
                .t("update_no_update")
                .replace("{current}", crate::APP_VERSION),
            Phase::CheckFailed => self.t("update_check_failed").to_string(),
            Phase::Downloading => {
                let (received, total) = self.updates.displayed_progress;
                self.t("update_downloading")
                    .replace(
                        "{received}",
                        &format!("{:.1}", received as f64 / 1_000_000.),
                    )
                    .replace("{total}", &format!("{:.1}", total as f64 / 1_000_000.))
            }
            Phase::Preparing => self.t("update_preparing").to_string(),
            Phase::Ready | Phase::Closing => self.t("update_installing").to_string(),
            Phase::Failed => self.t("update_failed").to_string(),
            _ => self
                .t("update_available")
                .replace("{current}", crate::APP_VERSION)
                .replace("{version}", version),
        };
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(theme::SPACE_PANEL))
            .child(text);
        if let Some(error) = &self.updates.error {
            body = body.child(
                div()
                    .text_color(theme::Palette::new(self.prefs.theme).muted)
                    .child(error.clone()),
            );
        } else if self.updates.phase == Phase::Available {
            body = body.child(self.t("update_restart_hint"));
        }
        body.into_any_element()
    }

    pub(super) fn render_update_footer(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut footer = div().flex().flex_wrap().gap(px(theme::SPACE_CONTROL));
        if self.updates.phase == Phase::CheckFailed {
            footer = footer.child(
                self.button("retry-update-check", self.t("update_check_retry"))
                    .primary()
                    .on_click(
                        cx.listener(|this, _, window, cx| this.manual_check_updates(window, cx)),
                    ),
            );
        }
        if matches!(self.updates.phase, Phase::Available | Phase::Failed) {
            footer = footer.child(
                self.button(
                    "confirm-update",
                    self.t(if self.updates.phase == Phase::Failed {
                        "update_retry"
                    } else {
                        "update_confirm"
                    }),
                )
                .primary()
                .on_click(cx.listener(|this, _, w, cx| this.start_update(w, cx))),
            );
        }
        footer
            .child(
                self.button(
                    "cancel-update",
                    self.t(
                        if matches!(self.updates.phase, Phase::NoUpdate | Phase::CheckFailed) {
                            "close"
                        } else {
                            "cancel"
                        },
                    ),
                )
                .disabled(self.update_handoff_active())
                .on_click(cx.listener(|this, _, w, cx| this.cancel_modal(w, cx))),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{Phase, Updates};
    use crate::update::Release;
    use std::time::{Duration, Instant};

    fn release() -> Release {
        let name = "MantaSH-9.0.0-macos-arm64.dmg";
        let assets: Vec<_> = [name.to_string(), format!("{name}.sha256")].into_iter().map(|name| {
            serde_json::json!({"browser_download_url": format!("https://github.com/realmx/MantaSH/releases/download/v9.0.0/{name}"), "name": name, "size": 100})
        }).collect();
        let data = serde_json::to_vec(&serde_json::json!({"tag_name": "v9.0.0", "draft": false, "prerelease": false, "assets": assets})).unwrap();
        crate::update::fixture_support::select(&data, "1.0.0", "macos", "aarch64")
            .unwrap()
            .unwrap()
    }

    #[test]
    fn automatic_empty_and_error_results_stay_quiet() {
        let mut state = Updates::default();
        for result in [Ok(None), Err("network unavailable".to_string())] {
            let request = state.begin_check(false).unwrap();
            assert!(state.finish_check(request, result));
            assert_eq!(state.phase, Phase::Idle);
            assert!(state.error.is_none());
            assert!(state.request.is_none());
        }
        assert!(state.next_check > Instant::now() + Duration::from_secs(59 * 60));
    }

    #[test]
    fn manual_checks_report_empty_results_and_errors_and_can_retry() {
        let mut state = Updates::default();
        let request = state.begin_check(true).unwrap();
        assert!(state.finish_check(request, Ok(None)));
        assert_eq!(state.phase, Phase::NoUpdate);
        let request = state.begin_check(true).unwrap();
        assert!(state.finish_check(request, Err("HTTP 403".into())));
        assert_eq!(state.phase, Phase::CheckFailed);
        assert_eq!(state.error.as_deref(), Some("HTTP 403"));
        let request = state.begin_check(true).unwrap();
        assert!(state.error.is_none());
        assert!(state.finish_check(request, Ok(Some(release()))));
        assert_eq!(state.phase, Phase::Available);
    }

    #[test]
    fn explicit_check_adopts_inflight_request_without_duplicates() {
        let mut state = Updates::default();
        let request = state.begin_check(false).unwrap();
        assert!(state.begin_check(true).is_none());
        assert!(state.begin_check(true).is_none());
        assert_eq!(state.request, Some(request));
        assert!(state.finish_check(request, Ok(None)));
        assert_eq!(state.phase, Phase::NoUpdate);
    }

    #[test]
    fn manual_check_can_offer_a_version_declined_for_automatic_checks() {
        let mut state = Updates::default();
        let request = state.begin_check(false).unwrap();
        assert!(state.finish_check(request, Ok(Some(release()))));
        assert!(state.cancel().is_none());
        assert!(state.declined.contains("9.0.0"));
        let request = state.begin_check(false).unwrap();
        assert!(state.finish_check(request, Ok(Some(release()))));
        assert_eq!(state.phase, Phase::Idle);
        let request = state.begin_check(true).unwrap();
        assert!(state.finish_check(request, Ok(Some(release()))));
        assert_eq!(state.phase, Phase::Available);
    }

    #[test]
    fn cancelled_check_cannot_replace_a_later_result() {
        let mut state = Updates::default();
        let old = state.begin_check(true).unwrap();
        assert!(state.cancel().is_none());
        assert_eq!(state.phase, Phase::Idle);
        let current = state.begin_check(true).unwrap();
        assert!(!state.finish_check(old, Ok(Some(release()))));
        assert_eq!(state.request, Some(current));
        assert_eq!(state.phase, Phase::Checking);
        assert!(state.finish_check(current, Ok(None)));
        assert!(!state.finish_check(old, Err("late failure".into())));
        assert_eq!(state.phase, Phase::NoUpdate);
        assert!(state.error.is_none());
    }

    #[test]
    fn checking_cannot_replace_an_active_update() {
        for phase in [
            Phase::Downloading,
            Phase::Preparing,
            Phase::Ready,
            Phase::Closing,
            Phase::Failed,
        ] {
            let mut state = Updates::default();
            state.phase = phase;
            assert!(state.begin_check(true).is_none());
            assert_eq!(state.phase, phase);
        }
    }
}
