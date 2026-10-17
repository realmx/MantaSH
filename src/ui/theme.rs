//! One semantic palette and font measurement policy for the whole native interface.
use crate::model::{Preferences, Theme};
use gpui::{App, Hsla, Window, px, rgb};

/// Keep application actions clear of the window edge or native window controls.
pub(super) const HEADER_END_PADDING: f32 = 12.;
/// Shared spacing for all work pages, toolbars and contextual action lists.
pub(super) const SPACE_SMALL: f32 = 4.;
pub(super) const SPACE_CONTROL: f32 = 8.;
pub(super) const SPACE_PANEL: f32 = 12.;
pub(super) const SPACE_SECTION: f32 = 16.;

/// The only action colors. Ghost/menu/tab treatments are presentation, not tones.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ButtonTone {
    #[default]
    Default,
    Primary,
    Success,
    Warning,
    Danger,
}

/// Opaque filled states plus readable ink for transparent icon/menu buttons.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ButtonColors {
    pub background: Hsla,
    pub foreground: Hsla,
    pub hover: Hsla,
    pub active: Hsla,
    pub ink: Hsla,
}
impl ButtonColors {
    fn new(background: u32, hover: u32, active: u32, foreground: u32, ink: u32) -> Self {
        let c = |value| Hsla::from(rgb(value));
        Self {
            background: c(background),
            foreground: c(foreground),
            hover: c(hover),
            active: c(active),
            ink: c(ink),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct ButtonPalette {
    pub default: ButtonColors,
    pub primary: ButtonColors,
    pub success: ButtonColors,
    pub warning: ButtonColors,
    pub danger: ButtonColors,
}
impl ButtonPalette {
    fn new(theme: Theme) -> Self {
        let c = ButtonColors::new;
        match theme {
            Theme::Day => Self {
                default: c(0xe9eef3, 0xdce3eb, 0xccd6e2, 0x24344a, 0x24344a),
                primary: c(0x23678c, 0x1e5878, 0x194963, 0xffffff, 0x23678c),
                success: c(0x23754c, 0x1c6540, 0x175535, 0xffffff, 0x23754c),
                warning: c(0xf0b44d, 0xe6a432, 0xda9422, 0x382509, 0x8a530b),
                danger: c(0xb33838, 0x9d3030, 0x872929, 0xffffff, 0xb33838),
            },
            Theme::Night => Self {
                default: c(0x263448, 0x304055, 0x3b4d64, 0xdde6f0, 0xdde6f0),
                primary: c(0x82c5ea, 0x9bd1ef, 0xb3ddf4, 0x111925, 0x82c5ea),
                success: c(0x73c69b, 0x8cd2ae, 0xa5dec1, 0x111925, 0x73c69b),
                warning: c(0xf0b44d, 0xf4c16b, 0xf7cf8b, 0x382509, 0xf0b44d),
                danger: c(0xef9696, 0xf3aeae, 0xf7c6c6, 0x111925, 0xef9696),
            },
        }
    }

    pub fn get(self, tone: ButtonTone) -> ButtonColors {
        match tone {
            ButtonTone::Default => self.default,
            ButtonTone::Primary => self.primary,
            ButtonTone::Success => self.success,
            ButtonTone::Warning => self.warning,
            ButtonTone::Danger => self.danger,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub background: Hsla,
    pub surface: Hsla,
    pub terminal: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    pub accent: Hsla,
    pub network_sent: Hsla,
    pub meter_green: Hsla,
    pub meter_blue: Hsla,
    pub meter_red: Hsla,
    pub border: Hsla,
    pub selected: Hsla,
    pub tab_selected: Hsla,
    pub tab_hover: Hsla,
    pub tab_close_hover: Hsla,
    pub error: Hsla,
    pub(super) buttons: ButtonPalette,
}
impl Palette {
    /// Low utilization approaches green below 25%; high utilization approaches red above 85%.
    /// Interpolation is continuous at both thresholds, with blue across the normal range.
    pub(super) fn meter_color(&self, percent: f64) -> Hsla {
        if !percent.is_finite() || !(0. ..=100.).contains(&percent) {
            return self.border;
        }
        if percent < 25. {
            self.meter_green
                .blend(self.meter_blue.opacity(percent as f32 / 25.))
        } else if percent > 85. {
            self.meter_blue
                .blend(self.meter_red.opacity((percent as f32 - 85.) / 15.))
        } else {
            self.meter_blue
        }
    }

    /// Fixed MantaSH colors independent from component-library preset themes.
    pub fn new(theme: Theme) -> Self {
        let c = |v| Hsla::from(rgb(v));
        match theme {
            Theme::Day => Self {
                background: c(0xedf1f5),
                surface: c(0xffffff),
                terminal: c(0xf8fafc),
                text: c(0x24344a),
                muted: c(0x607084),
                accent: c(0x23678c),
                network_sent: c(0x277467),
                meter_green: c(0x32a875),
                meter_blue: c(0x4b82e3),
                meter_red: c(0xe46161),
                border: c(0xdce3eb),
                selected: c(0xe8f2f8),
                tab_selected: c(0xe9eef3),
                tab_hover: c(0xf6f8fa),
                tab_close_hover: c(0xdce3eb),
                error: c(0xb33838),
                buttons: ButtonPalette::new(theme),
            },
            Theme::Night => Self {
                background: c(0x111925),
                surface: c(0x1b2635),
                terminal: c(0x121d2b),
                text: c(0xdde6f0),
                muted: c(0x9bacc1),
                accent: c(0x82c5ea),
                network_sent: c(0x8bd2bb),
                meter_green: c(0x60c894),
                meter_blue: c(0x7ca5ef),
                meter_red: c(0xf08484),
                border: c(0x304055),
                selected: c(0x263e52),
                tab_selected: c(0x263448),
                tab_hover: c(0x172130),
                tab_close_hover: c(0x304055),
                error: c(0xffa39a),
                buttons: ButtonPalette::new(theme),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_colors_respect_both_thresholds_in_each_theme() {
        for theme in [Theme::Day, Theme::Night] {
            let palette = Palette::new(theme);
            assert_eq!(palette.meter_color(0.), palette.meter_green);
            for percent in [25., 50., 85.] {
                assert_eq!(palette.meter_color(percent), palette.meter_blue);
            }
            assert_eq!(palette.meter_color(100.), palette.meter_red);
            for invalid in [f64::NAN, f64::INFINITY, -1., 101.] {
                assert_eq!(palette.meter_color(invalid), palette.border);
            }
        }
    }

    #[test]
    fn usage_colors_change_smoothly_without_jumps_at_thresholds() {
        let palette = Palette::new(Theme::Day);
        let blue = gpui::Rgba::from(palette.meter_blue);
        for percent in [24.999, 85.001] {
            let color = gpui::Rgba::from(palette.meter_color(percent));
            assert!(
                (color.r - blue.r).abs() < 0.0001
                    && (color.g - blue.g).abs() < 0.0001
                    && (color.b - blue.b).abs() < 0.0001
            );
        }
        let low = gpui::Rgba::from(palette.meter_color(12.5));
        let high = gpui::Rgba::from(palette.meter_color(92.5));
        assert!(low.g > blue.g && low.b < blue.b);
        assert!(high.r > blue.r && high.b < blue.b);
    }
}

/// Apply fonts and colors to common controls; terminal cells use the same stored preferences.
pub fn apply(preferences: &Preferences, window: &mut Window, cx: &mut App) {
    super::shell::menus(preferences.language, cx);
    gpui_component::Theme::change(
        if preferences.effective_theme() == Theme::Day {
            gpui_component::ThemeMode::Light
        } else {
            gpui_component::ThemeMode::Dark
        },
        Some(window),
        cx,
    );
    let p = Palette::new(preferences.effective_theme());
    let theme = gpui_component::Theme::global_mut(cx);
    theme.font_family = preferences.ui_font.clone().into();
    theme.font_size = px(preferences.ui_size);
    theme.mono_font_family = preferences.terminal_font.clone().into();
    theme.mono_font_size = px(preferences.terminal_size);
    theme.radius = px(5.);
    theme.radius_lg = px(10.);
    theme.shadow = false;
    theme.scrollbar_show = gpui_component::scroll::ScrollbarShow::Always;
    theme.colors.background = p.surface;
    theme.colors.foreground = p.text;
    theme.colors.border = p.border;
    theme.colors.input = p.border;
    let buttons = p.buttons;
    theme.colors.primary = buttons.primary.background;
    theme.colors.primary_foreground = buttons.primary.foreground;
    theme.colors.primary_hover = buttons.primary.hover;
    theme.colors.primary_active = buttons.primary.active;
    theme.colors.success = buttons.success.background;
    theme.colors.success_foreground = buttons.success.foreground;
    theme.colors.success_hover = buttons.success.hover;
    theme.colors.success_active = buttons.success.active;
    theme.colors.warning = buttons.warning.background;
    theme.colors.warning_foreground = buttons.warning.foreground;
    theme.colors.warning_hover = buttons.warning.hover;
    theme.colors.warning_active = buttons.warning.active;
    theme.colors.danger = buttons.danger.background;
    theme.colors.danger_foreground = buttons.danger.foreground;
    theme.colors.danger_hover = buttons.danger.hover;
    theme.colors.danger_active = buttons.danger.active;
    theme.colors.secondary = buttons.default.background;
    theme.colors.secondary_foreground = buttons.default.foreground;
    theme.colors.secondary_hover = buttons.default.hover;
    theme.colors.secondary_active = buttons.default.active;
    theme.colors.switch = p.muted;
    theme.colors.switch_thumb = buttons.primary.foreground;
    theme.colors.muted = p.background;
    theme.colors.muted_foreground = p.muted;
    theme.colors.accent = p.selected;
    theme.colors.accent_foreground = p.text;
    theme.colors.ring = p.accent;
    theme.colors.scrollbar = p.background;
    theme.colors.scrollbar_thumb = p.muted.opacity(0.55);
    theme.colors.scrollbar_thumb_hover = p.muted;
    window.set_rem_size(px(preferences.ui_size / 0.875));
    gpui_component::set_locale(if preferences.language == crate::model::Language::Zh {
        "zh-CN"
    } else {
        "en"
    });
}

pub(super) fn system_theme(window: &Window) -> Theme {
    match window.appearance() {
        gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark => Theme::Night,
        _ => Theme::Day,
    }
}
