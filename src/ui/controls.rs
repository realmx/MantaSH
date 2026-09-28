//! MantaSH buttons retain native component focus/click semantics with explicit typography and colors.
use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    Disableable, Icon, IconName, Selectable, Sizable,
    button::{Button as NativeButton, ButtonCustomVariant, ButtonVariants},
};
use std::rc::Rc;

#[derive(Clone, Copy)]
enum Tone {
    Normal,
    Ghost,
    Primary,
    Danger,
    TabLabel,
    TabClose,
}

#[derive(IntoElement)]
pub(super) struct Button {
    base: NativeButton,
    /// Raw id kept for the chromeless variant, which does not use the native button.
    raw: ElementId,
    label: SharedString,
    icon: Option<IconName>,
    /// Asset path served by the app asset source for icons outside the upstream set.
    custom_icon: Option<SharedString>,
    /// Arbitrary element rendered in place of the icon glyphs, keeping the
    /// button's own box so neighbors never shift (spinner swap).
    icon_element: Option<AnyElement>,
    /// Explicit icon color, independent of the label foreground.
    icon_color: Option<Hsla>,
    tone: Tone,
    selected: bool,
    disabled: bool,
    menu: bool,
    /// Light-weight rendering without the focusable native button chrome.
    chromeless: bool,
    tooltip: Option<SharedString>,
    /// Click listener kept on the wrapper so the chromeless variant, which
    /// discards the native button, fires the same handler.
    click: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>>,
    font_size: f32,
    palette: super::theme::Palette,
}
impl Button {
    /// Build labels as explicit children so upstream style refinement cannot erase contrast or font size.
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        font_size: f32,
        palette: super::theme::Palette,
    ) -> Self {
        let raw = id.into();
        Self {
            base: NativeButton::new(raw.clone())
                .small()
                .compact()
                // Standard buttons default to the 24px tier, scaling with the font
                // up to the 32px cap; explicit heights override per placement.
                .h(px((font_size * 1.45 + 2.).clamp(24., 32.))),
            raw,
            label: label.into(),
            icon: None,
            custom_icon: None,
            icon_element: None,
            icon_color: None,
            tone: Tone::Normal,
            selected: false,
            disabled: false,
            menu: false,
            chromeless: false,
            tooltip: None,
            click: None,
            font_size,
            palette,
        }
    }
    /// Render without the focusable native button: same cursor, hover, tooltip and
    /// click semantics at a fraction of the cost, for dense lists.
    pub fn chromeless(mut self) -> Self {
        self.chromeless = true;
        self
    }
    /// Pull an explicitly styled pixel extent out of the refinement so the
    /// chromeless variant honors the same `.w()`/`.h()` tier as the native one.
    fn styled_pixels(length: &Option<Length>) -> Option<Pixels> {
        match length {
            Some(Length::Definite(DefiniteLength::Absolute(AbsoluteLength::Pixels(pixels)))) => {
                Some(*pixels)
            }
            _ => None,
        }
    }
    /// Chromeless body: an interactive div with the same affordances (cursor,
    /// hover/press backgrounds, tooltip, icon and label) but no focus target.
    fn render_chromeless(mut self) -> impl IntoElement {
        let foreground = if self.disabled {
            self.palette.muted
        } else {
            self.palette.text
        };
        let icon_size = (self.font_size + 2.).min(20.);
        let tooltip = self.tooltip.clone();
        let click = self.click.clone();
        let size = self.base.style().size.clone();
        let width = Self::styled_pixels(&size.width);
        let height = Self::styled_pixels(&size.height);
        // Decide before `take()` empties the field for the element swap.
        let has_icon_element = self.icon_element.is_some();
        div()
            .id(self.raw.clone())
            .flex()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .min_w_0()
            .when_some(width, |d, width| d.w(width))
            .when_some(height, |d, height| d.h(height))
            .rounded(px(4.))
            .text_size(px(self.font_size))
            .text_color(foreground)
            .when_some(tooltip, |d, tooltip| {
                d.tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
                })
            })
            .when_some(click, |d, click| {
                d.on_click(move |event: &ClickEvent, window, cx| click(event, window, cx))
            })
            .hover(|style| style.bg(self.palette.tab_hover))
            .active(|style| style.bg(self.palette.selected))
            .when_some(self.icon_element.take(), |d, element| d.child(element))
            .when(!has_icon_element, |d| {
                d.when_some(self.icon, |d, icon| {
                    let icon = gpui_component::Icon::new(icon).size(px(icon_size));
                    d.child(match self.icon_color {
                        Some(color) => icon.text_color(color),
                        None => icon,
                    })
                })
                .when_some(self.custom_icon.take(), |d, path| {
                    d.child(
                        gpui::svg()
                            .path(path)
                            .flex_none()
                            .size(px(icon_size))
                            .text_color(self.icon_color.unwrap_or(foreground)),
                    )
                })
            })
            .when(!self.label.is_empty(), |d| {
                d.child(div().min_w_0().overflow_hidden().child(self.label))
            })
            .when(self.disabled, |d| d.cursor_not_allowed())
            .when(!self.disabled, |d| d.cursor_pointer())
    }
    pub fn primary(mut self) -> Self {
        self.tone = Tone::Primary;
        self
    }
    /// Align contextual menu icons and labels in consistent full-width rows.
    pub fn menu(mut self) -> Self {
        self.menu = true;
        self.tone = Tone::Ghost;
        self.base = self
            .base
            .w_full()
            .h_auto()
            .min_h(px((self.font_size * 1.45 + 2.).max(24.)));
        self
    }
    pub fn ghost(mut self) -> Self {
        self.tone = Tone::Ghost;
        self
    }
    /// Use an app-served SVG asset for icons missing from the upstream icon set.
    pub fn svg_icon(mut self, path: impl Into<SharedString>) -> Self {
        self.custom_icon = Some(path.into());
        self
    }
    /// Render an arbitrary element in place of the icon glyphs while keeping
    /// the button's box, so toolbar neighbors never shift while it swaps.
    pub fn icon_element(mut self, element: impl IntoElement) -> Self {
        self.icon_element = Some(element.into_any_element());
        self
    }
    /// Let the parent tab own selection and hover; keep the label's native focus behavior.
    pub fn tab_label(mut self, selected: bool) -> Self {
        self.tone = Tone::TabLabel;
        self.selected = selected;
        self
    }
    /// A compact close affordance gets its own neutral hover only inside the tab.
    pub fn tab_close(mut self) -> Self {
        self.tone = Tone::TabClose;
        self
    }
    pub fn danger(mut self) -> Self {
        self.tone = Tone::Danger;
        self
    }
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }
    /// Color only the icon, e.g. a red disconnect glyph on a neutral ghost button.
    pub fn icon_color(mut self, color: Hsla) -> Self {
        self.icon_color = Some(color);
        self
    }
    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }
    pub fn on_click(
        mut self,
        listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.click = Some(Rc::new(listener));
        self
    }
}
impl Styled for Button {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}
impl RenderOnce for Button {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        if self.chromeless {
            return self.render_chromeless().into_any_element();
        }
        let foreground = if self.disabled {
            self.palette.muted
        } else {
            match self.tone {
                Tone::Primary => self.palette.surface,
                Tone::Danger => gpui::white(),
                Tone::TabLabel if !self.selected => self.palette.muted,
                Tone::TabClose => self.palette.muted,
                _ => self.palette.text,
            }
        };
        // Decide before the chain moves the field into the element swap.
        let has_icon_element = self.icon_element.is_some();
        let mut base = match self.tone {
            Tone::Normal => self.base,
            Tone::Ghost => self.base.ghost(),
            Tone::Primary => self.base.primary(),
            Tone::Danger => self.base.danger(),
            Tone::TabLabel => self
                .base
                .custom(ButtonCustomVariant::new(cx).foreground(foreground)),
            Tone::TabClose => self.base.custom(
                ButtonCustomVariant::new(cx)
                    .foreground(foreground)
                    .hover(self.palette.tab_close_hover)
                    .active(self.palette.tab_close_hover),
            ),
        };
        if let Some(tooltip) = self.tooltip.clone() {
            base = base.tooltip(tooltip);
        }
        if let Some(click) = self.click.clone() {
            base = base.on_click(move |event: &ClickEvent, window, cx| click(event, window, cx));
        }
        // Wrap the button in a relative container whose cursor overlay covers
        // the full button area (including padding), not just the content area.
        div()
            .relative()
            .child(
                base.selected(self.selected).disabled(self.disabled).child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(self.menu, |d| d.w_full().justify_start())
                        .min_w_0()
                        .gap(px(8.))
                        .text_size(px(self.font_size))
                        .text_color(foreground)
                        .when(matches!(self.tone, Tone::TabLabel | Tone::TabClose), |d| {
                            d.group_hover("work-tab", |style| style.text_color(self.palette.text))
                        })
                        .when_some(self.icon_element, |d, element| d.child(element))
                        .when(!has_icon_element, |d| {
                            d.when_some(self.icon, |d, icon| {
                                // Icons scale with the font but never exceed the button's
                                // visual bounds: max 20px fits a 32px button comfortably.
                                let icon_size = if matches!(self.tone, Tone::TabClose) {
                                    self.font_size.min(16.)
                                } else {
                                    (self.font_size + 2.).min(20.)
                                };
                                let icon = Icon::new(icon).size(px(icon_size));
                                d.child(match self.icon_color {
                                    Some(color) => icon.text_color(color),
                                    None => icon,
                                })
                            })
                            .when_some(self.custom_icon, |d, path| {
                                // The svg painter reads only its own style.text.color; ancestor
                                // text colors never reach it, so mirror the upstream Icon element.
                                d.child(
                                    gpui::svg()
                                        .path(path)
                                        .flex_none()
                                        .size(px((self.font_size + 2.).min(20.)))
                                        .text_color(self.icon_color.unwrap_or(foreground)),
                                )
                            })
                        })
                        .when(!self.label.is_empty(), |d| {
                            d.child(
                                div()
                                    .min_w_0()
                                    .when(self.menu, |d| d.whitespace_normal())
                                    .when(!self.menu, |d| d.overflow_hidden())
                                    .child(self.label),
                            )
                        }),
                ),
            )
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .when(self.disabled, |o| o.cursor_not_allowed())
                    .when(!self.disabled, |o| o.cursor_pointer()),
            )
            .into_any_element()
    }
}
