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
                background: c(0xf6f7f9),
                surface: c(0xffffff),
                terminal: c(0xf6f7f9),
                text: c(0x242c3a),
                muted: c(0x626f80),
                accent: c(0x3b5fc0),
                network_sent: c(0x277467),
                meter_green: c(0x32a875),
                meter_blue: c(0x4b82e3),
                meter_red: c(0xe46161),
                border: c(0xe1e5eb),
                selected: c(0xedf1fc),
                tab_selected: c(0xf0f3f8),
                tab_hover: c(0xf5f6f8),
                tab_close_hover: c(0xdfe5ed),
                error: c(0xb33838),
            },
            Theme::Night => Self {
                background: c(0x191f29),
                surface: c(0x1d232d),
                terminal: c(0x191f29),
                text: c(0xdce3ed),
                muted: c(0xa2aec0),
                accent: c(0xa2baff),
                network_sent: c(0x8bd2bb),
                meter_green: c(0x60c894),
                meter_blue: c(0x7ca5ef),
                meter_red: c(0xf08484),
                border: c(0x333c4b),
                selected: c(0x293955),
                tab_selected: c(0x2a3340),
                tab_hover: c(0x252d38),
                tab_close_hover: c(0x3b4656),
                error: c(0xffa39a),
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
        if preferences.theme == Theme::Day {
            gpui_component::ThemeMode::Light
        } else {
            gpui_component::ThemeMode::Dark
        },
        Some(window),
        cx,
    );
    let p = Palette::new(preferences.theme);
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
    theme.colors.primary = p.accent;
    theme.colors.primary_foreground = p.surface;
    theme.colors.primary_hover = p.accent.opacity(0.92);
    theme.colors.primary_active = p.accent;
    theme.colors.secondary_hover = p.selected;
    theme.colors.secondary_active = p.selected;
    theme.colors.danger = rgb(0xa92f3b).into();
    theme.colors.danger_foreground = gpui::white();
    theme.colors.danger_hover = rgb(0x962532).into();
    theme.colors.danger_active = rgb(0x86202b).into();
    theme.colors.secondary = p.background;
    theme.colors.secondary_foreground = p.text;
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
