//! Native desktop entry point.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use gpui::prelude::*;
use gpui::{
    App, Application, AssetSource, Bounds, SharedString, WindowBounds, WindowOptions, px, size,
};
use std::borrow::Cow;
struct Assets;
/// Extra Lucide icons served under `icons/` before the upstream component set.
/// Drop new SVGs into `assets/icons/` and reference them via `svg_icon("icons/<name>.svg")`.
#[derive(rust_embed::RustEmbed)]
#[folder = "assets/icons"]
struct ExtraIcons;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        match path {
            "mantash-mark.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/mantash-monochrome.svg"
            )))),
            "mantash-play.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/mantash-play.svg"
            )))),
            _ => {
                if let Some(file) = path
                    .strip_prefix("icons/")
                    .and_then(|name| ExtraIcons::get(name))
                {
                    return Ok(Some(file.data));
                }
                gpui_component_assets::Assets.load(path)
            }
        }
    }
    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        let mut entries = gpui_component_assets::Assets.list(path)?;
        if path == "icons" {
            entries.extend(ExtraIcons::iter().map(|name| name.to_string().into()));
        }
        Ok(entries)
    }
}

fn main() {
    let (backend, receiver, snapshot, warning) =
        mantash::services::Backend::initialize().expect("initialize MantaSH data");
    // Documentation QA can render a separate isolated window without activating the user's app.
    let background_qa = cfg!(debug_assertions)
        && std::env::var_os("MANTASH_QA_CONTROL").is_some()
        && std::env::var_os("MANTASH_QA_BACKGROUND").is_some();
    // Keyboard dispatch checks can stay fully hidden while a user tries the visible app.
    let hidden_qa = background_qa
        && mantash::platform::data_override().is_some()
        && std::env::var_os("MANTASH_QA_HIDDEN").is_some();
    Application::new()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            mantash::ui::bind_keys(cx);
            mantash::ui::register_about(cx);
            let desired_size = size(
                px(snapshot.preferences.window_width),
                px(snapshot.preferences.window_height),
            );
            let mut bounds = Bounds::centered(None, desired_size, cx);
            if let (Some(x), Some(y)) =
                (snapshot.preferences.window_x, snapshot.preferences.window_y)
            {
                if x.is_finite() && y.is_finite() {
                    let candidate = Bounds::new(gpui::point(px(x), px(y)), desired_size);
                    if cx
                        .displays()
                        .iter()
                        .any(|d| d.bounds().contains(&candidate.center()))
                    {
                        bounds = candidate;
                    }
                }
            }
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    focus: !background_qa,
                    show: !hidden_qa,
                    window_min_size: Some(size(px(960.), px(640.))),
                    titlebar: if cfg!(target_os = "windows") {
                        // Keep the Win32 non-client title bar and its native caption buttons.
                        Some(gpui::TitlebarOptions::default())
                    } else {
                        Some(gpui_component::TitleBar::title_bar_options())
                    },
                    app_id: Some(
                        if background_qa {
                            "app.mantash.MantaSH.NativeQA"
                        } else {
                            "app.mantash.MantaSH"
                        }
                        .into(),
                    ),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("MantaSH");
                    window.resize(desired_size);
                    #[cfg(debug_assertions)]
                    if std::env::var_os("MANTASH_QA_CONTROL").is_some() {
                        window.set_window_title("MantaSH · Native QA");
                    }
                    let view = cx.new(|cx| {
                        mantash::ui::Workbench::new(
                            backend, receiver, snapshot, warning, window, cx,
                        )
                    });
                    cx.new(|cx| gpui_component::Root::new(view, window, cx))
                },
            )
            .expect("open MantaSH window");
            if !background_qa {
                cx.activate(true);
            }
        });
}
