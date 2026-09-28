//! Client-drawn Windows caption; native operations are queued outside GPUI borrows.
use super::*;
use gpui_component::IconName;

#[derive(Clone, Copy)]
pub(super) enum CaptionCommand {
    Move,
    Minimize,
    ToggleMaximize,
    Close,
}

/// Win32 system-command values used only by the Windows bridge.
#[cfg(target_os = "windows")]
fn command_code(command: CaptionCommand, maximized: bool) -> usize {
    match command {
        CaptionCommand::Move => 0xF010 | 2, // SC_MOVE | HTCAPTION
        CaptionCommand::Minimize => 0xF020,
        CaptionCommand::ToggleMaximize if maximized => 0xF120,
        CaptionCommand::ToggleMaximize => 0xF030,
        CaptionCommand::Close => 0xF060,
    }
}

/// Post to this live window only. SC_CLOSE reaches the existing should-close/save guard.
pub(super) fn post(window: &Window, command: CaptionCommand) -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use windows_sys::Win32::UI::{
            Input::KeyboardAndMouse::ReleaseCapture,
            WindowsAndMessaging::{IsZoomed, PostMessageW, WM_SYSCOMMAND},
        };
        let RawWindowHandle::Win32(handle) = window.window_handle()?.as_raw() else {
            anyhow::bail!("Not a Windows window");
        };
        let hwnd = handle.hwnd.get() as windows_sys::Win32::Foundation::HWND;
        // SAFETY: HWND is borrowed from this live GPUI window on its UI thread.
        // Posting (not sending) lets the event callback finish before the native modal loop.
        unsafe {
            let code = command_code(command, IsZoomed(hwnd) != 0);
            if matches!(command, CaptionCommand::Move) {
                ReleaseCapture();
            }
            anyhow::ensure!(
                PostMessageW(hwnd, WM_SYSCOMMAND, code, 0) != 0,
                "Cannot post window command: {}",
                std::io::Error::last_os_error()
            );
        }
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window, command);
        anyhow::bail!("Windows caption command on another platform")
    }
}

impl Workbench {
    pub(super) fn caption_command(
        &mut self,
        window: &Window,
        command: CaptionCommand,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = post(window, command) {
            self.notice = Some(error.to_string());
            cx.notify();
        }
    }

    pub(super) fn windows_caption_buttons(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let maximized = window.is_maximized();
        div()
            .flex()
            .items_center()
            .flex_shrink_0()
            .children(
                [
                    (
                        "caption-minimize",
                        IconName::WindowMinimize,
                        "window_minimize",
                        CaptionCommand::Minimize,
                        WindowControlArea::Min,
                    ),
                    (
                        "caption-maximize",
                        if maximized {
                            IconName::WindowRestore
                        } else {
                            IconName::WindowMaximize
                        },
                        if maximized {
                            "window_unmaximize"
                        } else {
                            "window_maximize"
                        },
                        CaptionCommand::ToggleMaximize,
                        WindowControlArea::Max,
                    ),
                    (
                        "caption-close",
                        IconName::WindowClose,
                        "window_close",
                        CaptionCommand::Close,
                        WindowControlArea::Close,
                    ),
                ]
                .map(|(id, icon, label, command, area)| {
                    let button = self
                        .button(id, "")
                        .icon(icon)
                        .ghost()
                        .tooltip(self.t(label))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.caption_command(window, command, cx);
                        }));
                    let control = self.header_control(id, button, cx);
                    div()
                        .when(cfg!(target_os = "windows"), |control| {
                            control.window_control_area(area)
                        })
                        .child(control)
                        .into_any_element()
                }),
            )
            .into_any_element()
    }
}
