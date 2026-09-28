//! Fast controls for this window; terminal and connection identities never change.
use super::*;
use crate::window_layout::{Command, Position, Rect};
use gpui_component::IconName;

#[derive(Clone, PartialEq, serde::Deserialize, gpui::Action)]
#[action(namespace = mantash, no_json)]
pub(super) struct ArrangeWindow {
    pub command: Command,
}

pub(super) struct WindowForm {
    pub width: Entity<InputState>,
    pub height: Entity<InputState>,
    pub error: Option<String>,
}

impl Workbench {
    /// Leave authentication and unsaved-change dialogs intact when opening the window panel.

    pub(super) fn open_window_controls(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.modal.is_some() {
            return;
        }
        #[cfg(target_os = "macos")]
        {
            match window_native::prepare_system_menu(window) {
                Ok(menu) => cx.spawn(async move |_, _| menu.show()).detach(),
                Err(error) => {
                    self.notice = Some(error.to_string());
                    cx.notify();
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let frame = window_native::info(window, cx).ok().map(|info| info.frame);
            let size = window.viewport_size();
            let form = WindowForm {
                width: Self::input(
                    &format!("{:.0}", frame.map_or(f32::from(size.width), |r| r.width)),
                    self.t("window_width"),
                    false,
                    window,
                    cx,
                ),
                height: Self::input(
                    &format!("{:.0}", frame.map_or(f32::from(size.height), |r| r.height)),
                    self.t("window_height"),
                    false,
                    window,
                    cx,
                ),
                error: None,
            };
            self.show_modal(Modal::WindowControls(form), window, cx);
        }
    }
    /// Keep invalid input in the form and report native errors without claiming success.
    fn window_error(&mut self, message: String, cx: &mut Context<Self>) {
        if let Some(Modal::WindowControls(form)) = &mut self.modal {
            form.error = Some(message);
        } else {
            self.notice = Some(message);
        }
        cx.notify();
    }
    /// Send custom dimensions through the same validation and native path as presets.
    pub(super) fn apply_custom_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Modal::WindowControls(form)) = &self.modal else {
            return;
        };
        let fields = [form.width.clone(), form.height.clone()];
        if fields.iter().any(|field| {
            field.update(cx, |input, cx| {
                EntityInputHandler::marked_text_range(input, window, cx).is_some()
            })
        }) {
            return;
        }
        let values = (
            form.width.read(cx).value().trim().parse::<f32>(),
            form.height.read(cx).value().trim().parse::<f32>(),
        );
        match values {
            (Ok(width), Ok(height)) => {
                self.arrange_window(Command::Custom { width, height }, window, cx)
            }
            _ => self.window_error(self.t("window_invalid_size").into(), cx),
        }
    }
    /// Run geometry changes after releasing update borrows: native resize callbacks are synchronous.
    pub(super) fn arrange_window(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.window_change.is_some() {
            return;
        }
        if window.is_fullscreen() {
            self.window_error(self.t("window_exit_fullscreen").into(), cx);
            return;
        }
        let info = match window_native::info(window, cx) {
            Ok(info) => info,
            Err(error) => {
                self.window_error(format!("{}: {error}", self.t("window_unavailable")), cx);
                return;
            }
        };
        let target = match crate::window_layout::target(
            command,
            info.frame,
            info.work_area,
            self.window_restore,
        ) {
            Ok(target) => target,
            Err(key) => {
                self.window_error(self.t(key).into(), cx);
                return;
            }
        };
        if !info.positioning {
            if matches!(command, Command::Position { .. }) {
                self.window_error(self.t("window_position_unavailable").into(), cx);
                return;
            }
            self.sync_window_form(target, window, cx);
            if command == Command::Fill {
                if !window.is_maximized() {
                    window.zoom_window();
                }
            } else {
                window.resize(size(px(target.width), px(target.height)));
            }
            self.remember_window_change(command, info.frame);
            cx.notify();
            return;
        }
        let prepared = match window_native::prepare(window, target) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.window_error(format!("{}: {error}", self.t("window_unavailable")), cx);
                return;
            }
        };
        self.sync_window_form(target, window, cx);
        let request = Id::new_v4();
        self.window_change = Some(request);
        cx.spawn(async move |weak, cx| {
            let Some(view) = weak.upgrade() else {
                return;
            };
            let result = prepared.apply();
            let _ = view.update(cx, |this, cx| {
                if this.window_change != Some(request) {
                    return;
                }
                this.window_change = None;
                match result {
                    Ok(()) => this.remember_window_change(command, info.frame),
                    Err(error) => {
                        this.window_error(format!("{}: {error}", this.t("window_unavailable")), cx)
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    /// Keep displayed dimensions in sync after presets are clamped to the work area.
    fn sync_window_form(&mut self, target: Rect, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(Modal::WindowControls(form)) = &mut self.modal {
            form.width.update(cx, |input, cx| {
                input.set_value(format!("{:.0}", target.width), window, cx)
            });
            form.height.update(cx, |input, cx| {
                input.set_value(format!("{:.0}", target.height), window, cx)
            });
            form.error = None;
        }
    }
    /// Preserve the rectangle preceding the first quick adjustment until the user restores it.
    fn remember_window_change(&mut self, command: Command, original: Rect) {
        if command == Command::Restore {
            self.window_restore = None;
        } else if self.window_restore.is_none() {
            self.window_restore = Some(original);
        }
    }
    /// Present size presets, editable dimensions and a spatial nine-grid of positions.
    pub(super) fn render_window_controls(
        &self,
        form: &WindowForm,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = theme::Palette::new(self.prefs.theme);
        let info = window_native::info(window, cx).ok();
        let disabled = window.is_fullscreen() || self.window_change.is_some();
        let positions_disabled = disabled || info.is_none_or(|info| !info.positioning);
        div()
            .flex()
            .flex_col()
            .gap(px(theme::SPACE_PANEL))
            .child(
                div().text_color(p.muted).child(
                    info.map(|info| {
                        format!(
                            "{} {:.0} × {:.0}  ·  {} {:.0} × {:.0}",
                            self.t("window_current"),
                            info.frame.width,
                            info.frame.height,
                            self.t("window_available"),
                            info.work_area.width,
                            info.work_area.height
                        )
                    })
                    .unwrap_or_else(|| self.t("window_unavailable").into()),
                ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(theme::SPACE_CONTROL))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.t("window_sizes")),
                    )
                    .child(
                        div()
                            .grid()
                            .grid_cols(2)
                            .gap(px(theme::SPACE_CONTROL))
                            .children(
                                [
                                    (Command::Compact, "window_compact"),
                                    (Command::Standard, "window_standard"),
                                    (Command::Wide, "window_wide"),
                                    (Command::Fill, "window_fill"),
                                ]
                                .into_iter()
                                .map(|(command, key)| {
                                    self.button(key, self.t(key))
                                        .icon(IconName::Frame)
                                        .menu()
                                        .disabled(disabled)
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.arrange_window(command, window, cx)
                                        }))
                                }),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(theme::SPACE_CONTROL))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.t("window_custom")),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(theme::SPACE_CONTROL))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(theme::SPACE_SMALL))
                                    .child(div().text_color(p.muted).child(self.t("window_width")))
                                    .child(self.input_box(&form.width)),
                            )
                            .child("×")
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(theme::SPACE_SMALL))
                                    .child(div().text_color(p.muted).child(self.t("window_height")))
                                    .child(self.input_box(&form.height)),
                            )
                            .child(
                                self.button("apply-window-size", self.t("apply"))
                                    .disabled(disabled)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.apply_custom_window(window, cx)
                                    })),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(theme::SPACE_CONTROL))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.t("window_position")),
                    )
                    .child(
                        div()
                            .grid()
                            .grid_cols(3)
                            .gap(px(theme::SPACE_CONTROL))
                            .children(Position::ALL.into_iter().enumerate().map(
                                |(index, position)| {
                                    let symbol =
                                        ["↖", "↑", "↗", "←", "•", "→", "↙", "↓", "↘"][index];
                                    self.button(
                                        position.key(),
                                        format!("{symbol} {}", self.t(position.key())),
                                    )
                                    .menu()
                                    .disabled(positions_disabled)
                                    .on_click(cx.listener(
                                        move |this, _, window, cx| {
                                            this.arrange_window(
                                                Command::Position { position },
                                                window,
                                                cx,
                                            )
                                        },
                                    ))
                                },
                            )),
                    ),
            )
            .child(div().text_color(p.muted).whitespace_normal().child(self.t(
                if window.is_fullscreen() {
                    "window_exit_fullscreen"
                } else if info.is_none_or(|info| !info.positioning) {
                    "window_position_unavailable"
                } else {
                    "window_placement_hint"
                },
            )))
            .when_some(form.error.clone(), |body, error| {
                body.child(div().text_color(p.error).child(error))
            })
            .into_any_element()
    }
    /// Keep system actions reachable while the size and position options scroll.
    pub(super) fn window_footer(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let disabled = window.is_fullscreen() || self.window_change.is_some();
        div()
            .flex()
            .flex_wrap()
            .gap(px(theme::SPACE_CONTROL))
            .child(
                self.button("restore-window", self.t("window_restore"))
                    .icon(IconName::Undo)
                    .disabled(disabled || self.window_restore.is_none())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.arrange_window(Command::Restore, window, cx)
                    })),
            )
            .child(
                self.button("minimize-window", self.t("window_minimize"))
                    .icon(IconName::Minus)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.dismiss(window, cx);
                        window.minimize_window();
                    })),
            )
            .child(
                self.button(
                    "fullscreen-window",
                    self.t(if window.is_fullscreen() {
                        "window_exit_fullscreen_action"
                    } else {
                        "window_fullscreen"
                    }),
                )
                .icon(IconName::Maximize)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.dismiss(window, cx);
                    window.toggle_fullscreen();
                })),
            )
    }
}
