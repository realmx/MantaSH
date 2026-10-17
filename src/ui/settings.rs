//! Settings use the same aligned rows and real platform defaults as the workbench.
use super::*;
use gpui_component::menu::PopupMenuItem;

pub(super) const SWITCH_KEYS: [&str; 4] = [
    "follow_system_theme",
    "mouse_reporting",
    "open_files_on_connect",
    "remember_credentials",
];

impl Workbench {
    pub(super) fn toggle_setting(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match key {
            "follow_system_theme" => {
                self.prefs.follow_system_theme = !self.prefs.follow_system_theme
            }
            "mouse_reporting" => self.prefs.mouse_reporting = !self.prefs.mouse_reporting,
            "open_files_on_connect" => {
                self.prefs.open_files_on_connect = !self.prefs.open_files_on_connect
            }
            "remember_credentials" => {
                self.prefs.remember_credentials = !self.prefs.remember_credentials
            }
            _ => return,
        }
        self.apply_preferences(window, cx);
    }

    pub(super) fn select_theme(
        &mut self,
        theme: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prefs.theme = theme;
        self.prefs.follow_system_theme = false;
        self.apply_preferences(window, cx);
    }

    pub(super) fn reset_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.prefs.restore_visible_defaults();
        self.apply_preferences(window, cx);
    }

    fn setting_row(&self, label: &str, control: impl IntoElement) -> Div {
        div()
            .flex()
            .items_center()
            .gap(px(theme::SPACE_PANEL))
            .child(div().flex_1().min_w_0().child(self.t(label)))
            .child(
                div()
                    .w(px(168. * self.prefs.ui_size / 14.))
                    .flex_none()
                    .flex()
                    .justify_end()
                    .child(control),
            )
    }

    fn setting_toggle(&self, key: &'static str, value: bool, cx: &mut Context<Self>) -> Div {
        let Some(focus) = self.settings_switch_focus.get(key) else {
            return div();
        };
        let control = div().child(
            controls::Switch::new(
                key,
                value,
                focus.clone(),
                self.prefs.ui_size,
                theme::Palette::new(self.prefs.effective_theme()),
            )
            .tooltip(format!(
                "{} · {}",
                self.t(key),
                self.t(if value { "enabled" } else { "disabled" })
            ))
            .on_click(cx.listener(move |this, _, w, cx| this.toggle_setting(key, w, cx))),
        );
        #[cfg(debug_assertions)]
        let control = control.when(self.qa.is_some(), |control| {
            let view = cx.entity();
            control.relative().child(
                canvas(
                    move |bounds, _, cx| {
                        view.update(cx, |this, _| {
                            if let Some(qa) = &mut this.qa {
                                qa.settings_switch_bounds.insert(key, bounds);
                            }
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
        });
        self.setting_row(key, control)
    }

    fn font_settings(&self, terminal: bool, cx: &mut Context<Self>) -> Div {
        let (family, size) = if terminal {
            (&self.prefs.terminal_font, self.prefs.terminal_size)
        } else {
            (&self.prefs.ui_font, self.prefs.ui_size)
        };
        let family_key = if terminal {
            "terminal-family"
        } else {
            "ui-family"
        };
        let family_label = if family.starts_with('.') {
            self.t("system_font").to_string()
        } else {
            family.clone()
        };
        div()
            .flex()
            .flex_col()
            .gap(px(theme::SPACE_CONTROL))
            .child(
                self.setting_row(
                    if terminal { "terminal_font" } else { "ui_font" },
                    self.button(family_key, family_label)
                        .tooltip(family.clone())
                        .on_click(cx.listener(move |this, _, w, cx| {
                            this.open_font_picker(terminal, w, cx)
                        })),
                ),
            )
            .child(
                self.setting_row(
                    if terminal { "terminal_size" } else { "ui_size" },
                    div()
                        .flex()
                        .items_center()
                        .gap(px(theme::SPACE_SMALL))
                        .child(
                            self.button(
                                if terminal {
                                    "terminal-minus"
                                } else {
                                    "ui-minus"
                                },
                                "−",
                            )
                            .disabled(size <= 12.)
                            .on_click(cx.listener(
                                move |this, _, w, cx| {
                                    if terminal {
                                        this.prefs.terminal_size -= 1.;
                                    } else {
                                        this.prefs.ui_size -= 1.;
                                    }
                                    this.apply_preferences(w, cx);
                                },
                            )),
                        )
                        .child(
                            div()
                                .w(px(52.))
                                .text_center()
                                .child(format!("{size:.0} px")),
                        )
                        .child(
                            self.button(if terminal { "terminal-plus" } else { "ui-plus" }, "+")
                                .disabled(size >= 18.)
                                .on_click(cx.listener(move |this, _, w, cx| {
                                    if terminal {
                                        this.prefs.terminal_size += 1.;
                                    } else {
                                        this.prefs.ui_size += 1.;
                                    }
                                    this.apply_preferences(w, cx);
                                })),
                        ),
                ),
            )
    }

    pub(super) fn render_settings(&self, cx: &mut Context<Self>) -> Div {
        let p = theme::Palette::new(self.prefs.effective_theme());
        let section = |key, separated| {
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .pt(px(theme::SPACE_CONTROL))
                .when(separated, |title| title.border_t_1().border_color(p.border))
                .child(self.t(key))
        };
        let languages = div().flex().gap(px(theme::SPACE_SMALL)).children(
            [(Language::Zh, "中文"), (Language::En, "English")].map(|(language, label)| {
                let selected = self.prefs.language == language;
                self.button(label, label)
                    .selected(selected)
                    .when(selected, |button| button.primary())
                    .on_click(cx.listener(move |this, _, w, cx| {
                        this.prefs.language = language;
                        this.apply_preferences(w, cx);
                    }))
            }),
        );
        let themes = div().flex().gap(px(theme::SPACE_SMALL)).children(
            [(Theme::Day, "day"), (Theme::Night, "night")].map(|(theme, label)| {
                let selected = self.prefs.effective_theme() == theme;
                self.button(label, self.t(label))
                    .selected(selected)
                    .when(selected, |button| button.primary())
                    .on_click(cx.listener(move |this, _, w, cx| this.select_theme(theme, w, cx)))
            }),
        );
        let choices = self.local_shells.clone().unwrap_or_default();
        let shell = if self.local_shells_loading {
            div().child(self.t("loading")).into_any_element()
        } else if choices.is_empty() {
            div()
                .text_color(p.error)
                .child(self.t("local_shell_unavailable"))
                .into_any_element()
        } else {
            let current = self.prefs.local_shell.clone();
            let label = current
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(&current)
                .to_string();
            let view = cx.entity().downgrade();
            self.button("local-shell-selector", label)
                .tooltip(current.clone())
                .dropdown_menu(move |mut menu, _, _| {
                    for shell in &choices {
                        let shell = shell.clone();
                        let view = view.clone();
                        menu = menu.item(
                            PopupMenuItem::new(shell.clone())
                                .checked(shell == current)
                                .on_click(move |_, w, cx| {
                                    let _ = view.update(cx, |this, cx| {
                                        if matches!(this.modal, Some(Modal::Settings))
                                            && this
                                                .local_shells
                                                .as_ref()
                                                .is_some_and(|v| v.contains(&shell))
                                        {
                                            this.prefs.local_shell = shell.clone();
                                            this.apply_preferences(w, cx);
                                        }
                                    });
                                }),
                        );
                    }
                    menu
                })
                .into_any_element()
        };
        div()
            .flex()
            .flex_col()
            .gap(px(theme::SPACE_PANEL))
            .child(section("appearance", false))
            .child(self.setting_row("theme", themes))
            .child(self.setting_toggle("follow_system_theme", self.prefs.follow_system_theme, cx))
            .child(self.setting_row("language", languages))
            .child(self.font_settings(false, cx))
            .child(section("terminal", true))
            .child(self.font_settings(true, cx))
            .child(self.setting_row("local_shell", shell))
            .child(self.setting_toggle("mouse_reporting", self.prefs.mouse_reporting, cx))
            .child(section("connection_security", true))
            .child(self.setting_toggle(
                "open_files_on_connect",
                self.prefs.open_files_on_connect,
                cx,
            ))
            .child(self.setting_toggle("remember_credentials", self.prefs.remember_credentials, cx))
    }
}
