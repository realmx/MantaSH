//! Shared native controls with consistent focus, pointer feedback, typography and colors.
use super::theme::ButtonTone;
use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    Disableable, Icon, IconName, Selectable, Sizable,
    button::{Button as NativeButton, ButtonCustomVariant, ButtonVariants},
    menu::{DropdownMenu, PopupMenu},
    switch::Switch as NativeSwitch,
};
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Appearance {
    Solid,
    Ghost,
    TabLabel,
    TabClose,
}

/// Both native and lightweight buttons consume the same resolved state colors.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ButtonPaint {
    background: Hsla,
    foreground: Hsla,
    border: Hsla,
    hover: Hsla,
    active: Hsla,
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
    tone: ButtonTone,
    appearance: Appearance,
    selected: bool,
    disabled: bool,
    menu: bool,
    dropdown: Option<Box<dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>>,
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
            tone: ButtonTone::Default,
            appearance: Appearance::Solid,
            selected: false,
            disabled: false,
            menu: false,
            dropdown: None,
            chromeless: false,
            tooltip: None,
            click: None,
            font_size,
            palette,
        }
    }
    /// Open a compact left-click dropdown without forcing full-width menu layout.
    pub fn dropdown_menu(
        mut self,
        builder: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
    ) -> Self {
        self.dropdown = Some(Box::new(builder));
        self
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
    fn colors(&self) -> ButtonPaint {
        let p = self.palette;
        let transparent = gpui::black().opacity(0.);
        let colors = p.buttons.get(self.tone);
        let mut paint = match self.appearance {
            Appearance::Solid => ButtonPaint {
                background: colors.background,
                foreground: colors.foreground,
                border: if self.tone == ButtonTone::Default {
                    p.border
                } else {
                    colors.background
                },
                hover: colors.hover,
                active: colors.active,
            },
            Appearance::Ghost => ButtonPaint {
                background: transparent,
                foreground: colors.ink,
                border: transparent,
                hover: if self.tone == ButtonTone::Default {
                    p.tab_hover
                } else {
                    p.surface.blend(colors.ink.opacity(0.08))
                },
                active: if self.tone == ButtonTone::Default {
                    p.tab_selected
                } else {
                    p.surface.blend(colors.ink.opacity(0.14))
                },
            },
            Appearance::TabLabel | Appearance::TabClose => ButtonPaint {
                background: transparent,
                foreground: if self.appearance == Appearance::TabLabel && self.selected {
                    p.text
                } else {
                    p.muted
                },
                border: transparent,
                hover: if self.appearance == Appearance::TabClose {
                    p.tab_close_hover
                } else {
                    transparent
                },
                active: if self.appearance == Appearance::TabClose {
                    p.tab_close_hover
                } else {
                    transparent
                },
            },
        };
        if self.selected {
            paint.background = paint.active;
            paint.hover = paint.active;
        }
        // Disabled is the same neutral state for all five tones and wins over
        // selection. No icon or lightweight rendering path may keep a live color.
        if self.disabled {
            paint.foreground = p.muted;
            paint.background = if self.appearance == Appearance::Solid {
                p.background
            } else {
                transparent
            };
            paint.border = if self.appearance == Appearance::Solid {
                p.border
            } else {
                transparent
            };
            paint.hover = paint.background;
            paint.active = paint.background;
        }
        paint
    }

    /// Chromeless body: an interactive div with the same affordances (cursor,
    /// hover/press backgrounds, tooltip, icon and label) but no focus target.
    fn render_chromeless(mut self) -> impl IntoElement {
        let colors = self.colors();
        let foreground = colors.foreground;
        let icon_size = (self.font_size + 2.).min(20.);
        let tooltip = self.tooltip.clone();
        let click = self.click.clone().filter(|_| !self.disabled);
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
            .bg(colors.background)
            .border_1()
            .border_color(colors.border)
            .when_some(tooltip, |d, tooltip| {
                d.tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
                })
            })
            .when_some(click, |d, click| {
                d.on_click(move |event: &ClickEvent, window, cx| click(event, window, cx))
            })
            .when(!self.disabled, |d| {
                d.hover(|style| style.bg(colors.hover))
                    .active(|style| style.bg(colors.active))
            })
            .when_some(self.icon_element.take(), |d, element| d.child(element))
            .when(!has_icon_element, |d| {
                d.when_some(self.icon, |d, icon| {
                    d.child(
                        gpui_component::Icon::new(icon)
                            .size(px(icon_size))
                            .text_color(foreground),
                    )
                })
                .when_some(self.custom_icon.take(), |d, path| {
                    d.child(
                        gpui::svg()
                            .path(path)
                            .flex_none()
                            .size(px(icon_size))
                            .text_color(foreground),
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
        self.tone = ButtonTone::Primary;
        self
    }
    pub fn success(mut self) -> Self {
        self.tone = ButtonTone::Success;
        self
    }
    pub fn warning(mut self) -> Self {
        self.tone = ButtonTone::Warning;
        self
    }
    /// Align contextual menu icons and labels in consistent full-width rows.
    pub fn menu(mut self) -> Self {
        self.menu = true;
        self.appearance = Appearance::Ghost;
        self.base = self
            .base
            .w_full()
            .h_auto()
            .min_h(px((self.font_size * 1.45 + 2.).max(24.)));
        self
    }
    pub fn ghost(mut self) -> Self {
        self.appearance = Appearance::Ghost;
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
        self.appearance = Appearance::TabLabel;
        self.tone = ButtonTone::Default;
        self.selected = selected;
        self
    }
    /// A compact close affordance gets its own neutral hover only inside the tab.
    pub fn tab_close(mut self) -> Self {
        self.appearance = Appearance::TabClose;
        self.tone = ButtonTone::Default;
        self
    }
    pub fn danger(mut self) -> Self {
        self.tone = ButtonTone::Danger;
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
        let colors = self.colors();
        let foreground = colors.foreground;
        // Decide before the chain moves the field into the element swap.
        let has_icon_element = self.icon_element.is_some();
        let mut base = self.base.custom(
            ButtonCustomVariant::new(cx)
                .color(colors.background)
                .foreground(foreground)
                .border(colors.border)
                .hover(colors.hover)
                .active(colors.active),
        );
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
                base.selected(self.selected)
                    .disabled(self.disabled)
                    // Upstream disabled colors use opacity; pin the same neutral
                    // paint as the lightweight path while retaining disabled input.
                    .when(self.disabled, |button| {
                        button
                            .bg(colors.background)
                            .border_color(colors.border)
                            .text_color(foreground)
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(self.menu, |d| d.w_full().justify_start())
                            .min_w_0()
                            .gap(px(8.))
                            .text_size(px(self.font_size))
                            .text_color(foreground)
                            .when(
                                !self.disabled
                                    && matches!(
                                        self.appearance,
                                        Appearance::TabLabel | Appearance::TabClose
                                    ),
                                |d| {
                                    d.group_hover("work-tab", |style| {
                                        style.text_color(self.palette.text)
                                    })
                                },
                            )
                            .when_some(self.icon_element, |d, element| d.child(element))
                            .when(!has_icon_element, |d| {
                                d.when_some(self.icon, |d, icon| {
                                    // Icons scale with the font but never exceed the button's
                                    // visual bounds: max 20px fits a 32px button comfortably.
                                    let icon_size = if self.appearance == Appearance::TabClose {
                                        self.font_size.min(16.)
                                    } else {
                                        (self.font_size + 2.).min(20.)
                                    };
                                    d.child(
                                        Icon::new(icon).size(px(icon_size)).when(
                                            self.appearance != Appearance::TabClose,
                                            |icon| icon.text_color(foreground),
                                        ),
                                    )
                                })
                                .when_some(
                                    self.custom_icon,
                                    |d, path| {
                                        // The svg painter reads only its own style.text.color; ancestor
                                        // text colors never reach it, so mirror the upstream Icon element.
                                        d.child(
                                            gpui::svg()
                                                .path(path)
                                                .flex_none()
                                                .size(px((self.font_size + 2.).min(20.)))
                                                .text_color(foreground),
                                        )
                                    },
                                )
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
                    )
                    .map(|button| {
                        if let Some(builder) = self.dropdown {
                            button
                                .dropdown_caret(true)
                                .dropdown_menu(builder)
                                .into_any_element()
                        } else {
                            button.into_any_element()
                        }
                    }),
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

/// Controlled switch: the component draws the track/thumb; this wrapper owns
/// focus and input so pointer release, Space and Enter share one change callback.
#[derive(IntoElement)]
pub(super) struct Switch {
    id: ElementId,
    checked: bool,
    disabled: bool,
    focus: FocusHandle,
    height: f32,
    palette: super::theme::Palette,
    tooltip: Option<SharedString>,
    on_click: Option<Rc<dyn Fn(&bool, &mut Window, &mut App)>>,
}

impl Switch {
    pub fn new(
        id: impl Into<ElementId>,
        checked: bool,
        focus: FocusHandle,
        font_size: f32,
        palette: super::theme::Palette,
    ) -> Self {
        Self {
            id: id.into(),
            checked,
            disabled: false,
            focus,
            height: (font_size * 1.45 + 2.).clamp(24., 32.),
            palette,
            tooltip: None,
            on_click: None,
        }
    }

    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    pub fn on_click(mut self, listener: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(listener));
        self
    }
}

impl Disableable for Switch {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl RenderOnce for Switch {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let checked = self.checked;
        let focused = !self.disabled && self.focus.is_focused(window);
        let p = self.palette;
        let click = self.on_click.filter(|_| !self.disabled);
        let keyboard = click.clone();
        div()
            .id(self.id.clone())
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .w(px(44.))
            .h(px(self.height))
            .rounded(px(self.height / 2.))
            .border_1()
            .border_color(if focused {
                p.accent
            } else {
                gpui::transparent_black()
            })
            .when(!self.disabled, |control| {
                control
                    .track_focus(&self.focus)
                    .cursor_pointer()
                    .hover(|style| style.border_color(if focused { p.accent } else { p.border }))
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        self.focus.focus(window);
                        cx.stop_propagation();
                    })
            })
            .when(self.disabled, |control| control.cursor_not_allowed())
            .when_some(self.tooltip, |control, tooltip| {
                control.tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
                })
            })
            .when_some(click, |control, click| {
                control.on_click(move |event, window, cx| {
                    cx.stop_propagation();
                    // GPUI also synthesizes clicks on key-up; keyboard input is handled below.
                    if matches!(event, ClickEvent::Mouse(_)) {
                        click(&!checked, window, cx);
                    }
                })
            })
            .when_some(keyboard, |control, keyboard| {
                control.on_key_down(move |event, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "space" | "enter")
                        && event.keystroke.modifiers == Modifiers::default()
                    {
                        cx.stop_propagation();
                        if !event.is_held {
                            keyboard(&!checked, window, cx);
                        }
                    }
                })
            })
            // No inner listener: the wrapper handles release and keyboard input.
            .child(
                NativeSwitch::new("track")
                    .checked(checked)
                    .disabled(self.disabled),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::super::theme::{ButtonTone, Palette};
    use super::{Appearance, Button};
    use crate::model::Theme;
    use gpui::{Hsla, Rgba};

    const TONES: [ButtonTone; 5] = [
        ButtonTone::Default,
        ButtonTone::Primary,
        ButtonTone::Success,
        ButtonTone::Warning,
        ButtonTone::Danger,
    ];

    fn button(palette: Palette, tone: ButtonTone) -> Button {
        let mut button = Button::new("color-test", "Action", 14., palette);
        button.tone = tone;
        button
    }

    fn contrast(a: Hsla, b: Hsla) -> f32 {
        let luminance = |color: Hsla| {
            let color = Rgba::from(color);
            let linear = |c: f32| {
                if c <= 0.04045 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
        };
        let a = luminance(a);
        let b = luminance(b);
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn action_text_remains_readable_in_both_themes_and_interaction_states() {
        for theme in [Theme::Day, Theme::Night] {
            let p = Palette::new(theme);
            for tone in TONES {
                for appearance in [Appearance::Solid, Appearance::Ghost] {
                    let mut button = button(p, tone);
                    button.appearance = appearance;
                    let paint = button.colors();
                    for surface in [p.background, p.surface, p.selected, p.tab_selected] {
                        for background in [paint.background, paint.hover, paint.active] {
                            let ratio = contrast(paint.foreground, surface.blend(background));
                            assert!(ratio >= 4.5, "{theme:?} {tone:?}: contrast {ratio}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn presentation_preserves_tone_regardless_of_builder_order() {
        let p = Palette::new(Theme::Day);
        let new = || Button::new("color-test", "Action", 14., p);
        for apply in [
            Button::primary,
            Button::success,
            Button::warning,
            Button::danger,
        ] {
            let ghost = apply(new()).ghost().colors();
            assert_eq!(ghost, apply(new().ghost()).colors());
            assert_eq!(ghost, apply(new().menu()).colors());
            assert_eq!(ghost, apply(new().ghost().chromeless()).colors());
            assert_ne!(ghost.foreground, new().ghost().colors().foreground);
        }
    }

    #[test]
    fn disabled_selection_is_neutral_for_every_tone_and_renderer() {
        for theme in [Theme::Day, Theme::Night] {
            let p = Palette::new(theme);
            for appearance in [
                Appearance::Solid,
                Appearance::Ghost,
                Appearance::TabLabel,
                Appearance::TabClose,
            ] {
                let mut reference = button(p, ButtonTone::Default).disabled(true);
                reference.appearance = appearance;
                let neutral = reference.colors();
                for tone in TONES {
                    for selected in [false, true] {
                        for chromeless in [false, true] {
                            let mut button = button(p, tone).selected(selected).disabled(true);
                            button.appearance = appearance;
                            button.chromeless = chromeless;
                            assert_eq!(button.colors(), neutral);
                        }
                    }
                }
                assert_eq!(neutral.background, neutral.hover);
                assert_eq!(neutral.background, neutral.active);
            }
        }
    }
}
