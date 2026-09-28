//! Compact, read-only port inspection inside the SSH system-tools dialog.
use super::*;
use crate::port_view::{self, PortRow};
use gpui_component::IconName;

fn port_sample_unavailable(sample: &crate::monitor::Sample) -> bool {
    sample.errors.contains_key("ports") || sample.errors.contains_key("system")
}

impl Workbench {
    fn measure_port_region(&self, name: &'static str, _cx: &mut Context<Self>) -> AnyElement {
        #[cfg(debug_assertions)]
        if self.qa.is_some() {
            let view = _cx.entity();
            return canvas(
                move |bounds, _, cx| {
                    view.update(cx, |this, _| {
                        if let Some(qa) = &mut this.qa {
                            qa.port_geometry.insert(name, bounds);
                            qa.port_revision += 1;
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

    /// Drop hidden or disappeared details; an old sample must never revive an expanded row.
    pub(super) fn prune_port_expansion(&mut self, owner: Owner, cx: &mut Context<Self>) {
        let Some(pane) = self.pane_mut(owner) else {
            return;
        };
        let query = pane.port_filter.read(cx).value();
        let rows = pane
            .monitor
            .as_ref()
            .filter(|sample| !port_sample_unavailable(sample))
            .map(|sample| {
                port_view::visible(&sample.ports, &query, pane.port_protocol, pane.port_sort)
            });
        if let Some(rows) = rows {
            pane.port_expanded
                .retain(|key| rows.iter().any(|row| &row.key == key));
        } else {
            pane.port_expanded.clear();
        }
    }

    pub(super) fn toggle_port_detail(
        &mut self,
        owner: Owner,
        key: PortKey,
        cx: &mut Context<Self>,
    ) {
        if !matches!(&self.modal, Some(Modal::SystemTools { owner: current, page: SystemPage::Ports }) if *current == owner)
        {
            return;
        }
        let Some(pane) = self.pane_mut(owner) else {
            return;
        };
        if pane.state != ConnectionState::Connected {
            return;
        }
        let query = pane.port_filter.read(cx).value();
        if pane.monitor.as_ref().is_none_or(|sample| {
            port_sample_unavailable(sample)
                || !port_view::visible(&sample.ports, &query, pane.port_protocol, pane.port_sort)
                    .iter()
                    .any(|row| row.key == key)
        }) {
            return;
        }
        if !pane.port_expanded.remove(&key) {
            pane.port_expanded.insert(key);
        }
        cx.notify();
    }
    pub(super) fn copy_port_value(
        &self,
        owner: Owner,
        key: &PortKey,
        number: bool,
        cx: &mut App,
    ) -> bool {
        if !matches!(&self.modal, Some(Modal::SystemTools { owner: current, page: SystemPage::Ports }) if *current == owner)
        {
            return false;
        }
        let Some(pane) = self.pane(owner) else {
            return false;
        };
        if pane.state != ConnectionState::Connected || !pane.port_expanded.contains(key) {
            return false;
        }
        let query = pane.port_filter.read(cx).value();
        let Some(row) = pane
            .monitor
            .as_ref()
            .filter(|sample| !port_sample_unavailable(sample))
            .and_then(|sample| {
                port_view::visible(&sample.ports, &query, pane.port_protocol, pane.port_sort)
                    .into_iter()
                    .find(|row| &row.key == key)
            })
        else {
            return false;
        };
        let value = if number {
            row.number.map(|number| number.to_string())
        } else {
            Some(row.source.local.clone())
        };
        if let Some(value) = value {
            cx.write_to_clipboard(ClipboardItem::new_string(value));
            true
        } else {
            false
        }
    }

    /// Resolve the PIDs from one `ss` row against the same live process sample.
    fn port_associated_processes(
        &self,
        owner: Owner,
        raw_process: &str,
    ) -> Vec<crate::monitor::Process> {
        let Some(sample) = self.pane(owner).and_then(|pane| pane.monitor.as_ref()) else {
            return Vec::new();
        };
        port_view::process_pids(raw_process)
            .into_iter()
            .filter_map(|pid| sample.processes.iter().find(|process| process.pid == pid))
            .cloned()
            .collect()
    }

    fn port_result_row(
        &self,
        owner: Owner,
        index: usize,
        row: &PortRow<'_>,
        expanded: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = theme::Palette::new(self.prefs.theme);
        let port = row.source;
        let key = row.key.clone();
        let number = row.number;
        let label = if row.label.is_empty() {
            port.local.as_str()
        } else {
            row.label
        };
        let process = if port.process.is_empty() {
            self.t("port_missing")
        } else {
            &port.process
        };
        let process_ids = port_view::process_pids(&port.process);
        let associated_processes = self.port_associated_processes(owner, &port.process);
        let copy_endpoint = row.key.clone();
        let copy_number = row.key.clone();
        div()
            .relative()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .when(index == 0, |item| {
                item.child(self.measure_port_region("first_row", cx))
            })
            .border_b_1()
            .border_color(p.border)
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(theme::SPACE_CONTROL))
                    .py(px(theme::SPACE_SMALL))
                    .child(
                        div()
                            .relative()
                            .w(px(88.))
                            .flex_shrink_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(label.to_string())
                            .when(index == 0, |cell| {
                                cell.child(self.measure_port_region("row_port", cx))
                            }),
                    )
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .font_family(self.prefs.terminal_font.clone())
                            .child(row.address.to_string())
                            .when(index == 0, |cell| {
                                cell.child(self.measure_port_region("address", cx))
                            }),
                    )
                    .child(
                        div()
                            .relative()
                            .w(px(144.))
                            .flex_shrink_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(format!("{} / {}", port.protocol, port.state))
                            .when(index == 0, |cell| {
                                cell.child(self.measure_port_region("protocol", cx))
                            }),
                    )
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_color(p.muted)
                            .child(process.to_string())
                            .when(index == 0, |cell| {
                                cell.child(self.measure_port_region("process", cx))
                            }),
                    )
                    .child(
                        self.button(("port-expand", index), "")
                            .icon(if expanded {
                                IconName::ChevronUp
                            } else {
                                IconName::ChevronDown
                            })
                            .ghost()
                            .chromeless()
                            .w(px(20.))
                            .h(px(20.))
                            .p_0()
                            .tooltip(self.t(if expanded {
                                "port_collapse"
                            } else {
                                "port_expand"
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.toggle_port_detail(owner, key.clone(), cx)
                            })),
                    ),
            )
            .when(expanded, |item| {
                item.child(
                    div()
                        .w_full()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(theme::SPACE_SMALL))
                        .px(px(theme::SPACE_CONTROL))
                        .pb(px(theme::SPACE_CONTROL))
                        .text_color(p.muted)
                        .child(div().min_w_0().whitespace_normal().child(format!(
                            "{}: {}",
                            self.t("port_local"),
                            port.local
                        )))
                        .child(div().min_w_0().whitespace_normal().child(format!(
                            "{}: {}",
                            self.t("port_peer"),
                            port.peer
                        )))
                        .child(div().min_w_0().whitespace_normal().child(format!(
                            "{}: {} / {}",
                            self.t("port_protocol"),
                            port.protocol,
                            port.state
                        )))
                        .child(div().min_w_0().whitespace_normal().child(format!(
                            "{}: {}",
                            self.t("port_process"),
                            process
                        )))
                        .when(!process_ids.is_empty(), |details| {
                            details.child(
                                div()
                                    .w_full()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(theme::SPACE_SMALL))
                                    .child(
                                        div()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(self.t("port_associated_process")),
                                    )
                                    .when(associated_processes.is_empty(), |details| {
                                        details.child(
                                            div()
                                                .min_w_0()
                                                .whitespace_normal()
                                                .child(self.t("port_process_unavailable")),
                                        )
                                    })
                                    .children(associated_processes.into_iter().enumerate().map(
                                        |(process_index, process)| {
                                            let process_for_click = process.clone();
                                            let command = if process.command.is_empty() {
                                                "—".to_string()
                                            } else {
                                                process.command.clone()
                                            };
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
                                                        .child(format!(
                                                            "{} (PID {})",
                                                            command, process.pid
                                                        )),
                                                )
                                                .child(
                                                    self.button(
                                                        (
                                                            "port-process-details",
                                                            index * 1024 + process_index,
                                                        ),
                                                        "",
                                                    )
                                                    .icon(IconName::ChevronRight)
                                                    .ghost()
                                                    .chromeless()
                                                    .w(px(20.))
                                                    .h(px(20.))
                                                    .p_0()
                                                    .tooltip(self.t("process_details"))
                                                    .on_click(cx.listener(
                                                        move |this, _, window, cx| {
                                                            this.show_process_details(
                                                                owner,
                                                                process_for_click.clone(),
                                                                window,
                                                                cx,
                                                            )
                                                        },
                                                    )),
                                                )
                                                .into_any_element()
                                        },
                                    )),
                            )
                        })
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap(px(theme::SPACE_CONTROL))
                                .child(
                                    self.button(
                                        ("port-copy-endpoint", index),
                                        self.t("port_copy_endpoint"),
                                    )
                                    .icon(IconName::Copy)
                                    .ghost()
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.copy_port_value(owner, &copy_endpoint, false, cx);
                                        },
                                    )),
                                )
                                .child(
                                    self.button(
                                        ("port-copy-number", index),
                                        self.t("port_copy_number"),
                                    )
                                    .icon(IconName::Copy)
                                    .ghost()
                                    .disabled(number.is_none())
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.copy_port_value(owner, &copy_number, true, cx);
                                        },
                                    )),
                                ),
                        ),
                )
            })
            .into_any_element()
    }

    /// Fixed toolbar and column labels; only the rows beneath them scroll.
    pub(super) fn render_ports(&self, owner: Owner, cx: &mut Context<Self>) -> AnyElement {
        let p = theme::Palette::new(self.prefs.theme);
        let Some(pane) = self.pane(owner) else {
            return div().into_any_element();
        };
        let connected = pane.state == ConnectionState::Connected;
        let sample = pane.monitor.as_ref();
        let section_error = sample.and_then(|sample| {
            sample
                .errors
                .get("ports")
                .or_else(|| sample.errors.get("system"))
        });
        let query = pane.port_filter.read(cx).value();
        let rows = sample
            .filter(|_| connected && section_error.is_none())
            .map(|sample| {
                port_view::visible(&sample.ports, &query, pane.port_protocol, pane.port_sort)
            })
            .unwrap_or_default();
        let total = sample.map_or(0, |sample| sample.ports.len());
        let refreshing = pane.monitor_request.is_some();
        let filters = [
            (ProtocolFilter::All, "port_all"),
            (ProtocolFilter::Tcp, "port_tcp"),
            (ProtocolFilter::Udp, "port_udp"),
        ];
        let status = if !connected {
            self.t("port_disconnected").to_string()
        } else if let Some(error) = section_error {
            format!("{}: {error}", self.t("unavailable"))
        } else if let Some(error) = &pane.monitor_error {
            format!("{}: {error}", self.t("port_old_data"))
        } else if sample.is_none() {
            self.t(if refreshing {
                "loading"
            } else {
                "refresh_to_sample"
            })
            .to_string()
        } else if total == 0 {
            self.t("port_empty").to_string()
        } else if rows.is_empty() {
            self.t("no_matches").to_string()
        } else {
            format!("{} / {}", rows.len(), total)
        };
        let error_color = !connected || section_error.is_some() || pane.monitor_error.is_some();
        div()
            .w_full()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .relative()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(theme::SPACE_CONTROL))
                    .child(self.measure_port_region("toolbar", cx))
                    .px(px(theme::SPACE_PANEL))
                    .pt(px(theme::SPACE_PANEL))
                    .pb(px(theme::SPACE_CONTROL))
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
                                    .child(self.input_box(&pane.port_filter)),
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
                            .child(self.measure_port_region("controls", cx))
                            .child(
                                div()
                                    .relative()
                                    .flex()
                                    .items_center()
                                    .gap(px(theme::SPACE_SMALL))
                                    .child(self.measure_port_region("filters", cx))
                                    .children(filters.into_iter().enumerate().map(
                                        |(index, (filter, text))| {
                                            self.button(("port-filter", index), self.t(text))
                                                .ghost()
                                                .selected(pane.port_protocol == filter)
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    if let Some(pane) = this.pane_mut(owner) {
                                                        pane.port_protocol = filter;
                                                    }
                                                    this.prune_port_expansion(owner, cx);
                                                    cx.notify();
                                                }))
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .relative()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .text_color(if error_color { p.error } else { p.muted })
                                    .child(self.measure_port_region("status", cx))
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
                    .child(self.measure_port_region("columns", cx))
                    .px(px(theme::SPACE_PANEL))
                    .py(px(theme::SPACE_SMALL))
                    .bg(p.background)
                    .border_t_1()
                    .border_b_1()
                    .border_color(p.border)
                    .text_color(p.muted)
                    .child(
                        div()
                            .relative()
                            .w(px(88.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .child(self.measure_port_region("header_port", cx))
                            .child(
                                self.button(
                                    "port-sort-number",
                                    self.t(if pane.port_sort == PortSort::Descending {
                                        "port_sort_desc"
                                    } else {
                                        "port_sort_asc"
                                    }),
                                )
                                .ghost()
                                .selected(pane.port_sort != PortSort::Protocol)
                                .h(px(24.))
                                .p_0()
                                .tooltip(self.t(if pane.port_sort == PortSort::Ascending {
                                    "port_sort_next_desc"
                                } else {
                                    "port_sort_next_asc"
                                }))
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        if let Some(pane) = this.pane_mut(owner) {
                                            pane.port_sort =
                                                if pane.port_sort == PortSort::Ascending {
                                                    PortSort::Descending
                                                } else {
                                                    PortSort::Ascending
                                                };
                                        }
                                        cx.notify();
                                    },
                                )),
                            ),
                    )
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(self.measure_port_region("header_address", cx))
                            .child(self.t("port_address")),
                    )
                    .child(
                        div()
                            .relative()
                            .w(px(144.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .child(self.measure_port_region("header_protocol", cx))
                            .child(
                                self.button("port-sort-protocol", self.t("port_protocol_state"))
                                    .ghost()
                                    .selected(pane.port_sort == PortSort::Protocol)
                                    .h(px(24.))
                                    .p_0()
                                    .tooltip(self.t("port_sort_protocol"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(pane) = this.pane_mut(owner) {
                                            pane.port_sort = PortSort::Protocol;
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(self.measure_port_region("header_process", cx))
                            .child(self.t("port_process")),
                    )
                    .child(div().w(px(20.)).flex_none()),
            )
            .child(
                div()
                    .relative()
                    .w_full()
                    .min_w_0()
                    // Keep the list viewport and its position indicator in a
                    // non-scrolling flex shell so rows cannot carry the bar
                    // when the scroll offset changes.
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(
                        div()
                            .relative()
                            .id(("port-list", owner.session.as_u128() as u64))
                            .w_full()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h_0()
                            .child(self.measure_port_region("viewport", cx))
                            .overflow_y_scroll()
                            .track_scroll(&pane.monitor_scroll[2])
                            .child(
                                div()
                                    .w_full()
                                    .min_w_0()
                                    .px(px(theme::SPACE_PANEL))
                                    .flex()
                                    .flex_col()
                                    .when_some(section_error, |list, error| {
                                        list.child(
                                            div()
                                                .w_full()
                                                .min_w_0()
                                                .py(px(theme::SPACE_CONTROL))
                                                .whitespace_normal()
                                                .text_color(p.error)
                                                .child(error.clone()),
                                        )
                                    })
                                    .when_some(pane.monitor_error.as_ref(), |list, error| {
                                        list.child(
                                            div()
                                                .w_full()
                                                .min_w_0()
                                                .py(px(theme::SPACE_CONTROL))
                                                .whitespace_normal()
                                                .text_color(p.error)
                                                .child(error.clone()),
                                        )
                                    })
                                    .children(rows.iter().enumerate().map(|(index, row)| {
                                        self.port_result_row(
                                            owner,
                                            index,
                                            row,
                                            pane.port_expanded.contains(&row.key),
                                            cx,
                                        )
                                    })),
                            ),
                    )
                    // Keep the indicator layer attached to the fixed shell;
                    // only the rows inside the sibling viewport may scroll.
                    .child(div().absolute().inset_0().child(self.overlay_scrollbar(
                        "port-list-scrollbar",
                        pane.monitor_scroll[2].clone(),
                        Resize::ModalScroll,
                        cx,
                    ))),
            )
            .into_any_element()
    }
}
