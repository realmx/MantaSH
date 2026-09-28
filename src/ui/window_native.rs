//! Native window geometry, scoped to the GPUI window's own handle on the UI thread.
use crate::window_layout::Rect;
use gpui::{App, Window};

/// Work area excludes system panels; coordinates are relative to the current monitor.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub(crate) struct Info {
    pub frame: Rect,
    pub work_area: Rect,
    pub positioning: bool,
}

#[cfg(target_os = "macos")]
#[allow(deprecated)] // Match the Cocoa ABI used by the pinned GPUI backend.
mod native {
    use super::*;
    use cocoa::{
        appkit::{NSApp, NSApplication, NSMenu, NSMenuItem, NSScreen, NSWindow},
        base::{YES, id, nil},
        foundation::{NSPoint, NSRect, NSSize, NSString},
    };
    use objc::{msg_send, rc::StrongPtr, sel, sel_impl};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    /// Resolve only the NSWindow attached to the borrowed GPUI NSView; never use the global key window.
    #[allow(unexpected_cfgs)] // objc checks a legacy cargo-clippy flag inside its selector macro.
    fn handle(window: &Window) -> anyhow::Result<id> {
        let RawWindowHandle::AppKit(handle) = HasWindowHandle::window_handle(window)
            .map_err(|error| anyhow::anyhow!("Native window handle unavailable: {error:?}"))?
            .as_raw()
        else {
            anyhow::bail!("Not an AppKit window");
        };
        // SAFETY: GPUI calls this on its UI thread; the raw NSView is borrowed from this live Window.
        let native: id = unsafe { msg_send![handle.ns_view.as_ptr() as id, window] };
        anyhow::ensure!(native != nil, "Window is no longer available");
        Ok(native)
    }

    /// Queue test-only AppKit events for this exact window; never move the system pointer or target another app.
    #[cfg(debug_assertions)]
    #[allow(unexpected_cfgs)]
    pub(crate) fn prepare_test_gesture(
        window: &Window,
        points: &[[f32; 2]],
        cancel: bool,
        click_count: i64,
    ) -> anyhow::Result<TestGesture> {
        use cocoa::appkit::{NSEvent, NSEventModifierFlags, NSEventType};
        use cocoa::base::NO;
        use objc::class;
        anyhow::ensure!(
            (2..=64).contains(&points.len())
                && points
                    .iter()
                    .flatten()
                    .all(|v| v.is_finite() && v.abs() <= 20000.),
            "Invalid test pointer path"
        );
        let native = handle(window)?;
        static TEST_GESTURE_NUMBER: std::sync::atomic::AtomicI64 =
            std::sync::atomic::AtomicI64::new(1);
        let event_number = TEST_GESTURE_NUMBER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut events = Vec::new();
        // SAFETY: The opt-in debug driver supplies public fixture coordinates. Events are posted to
        // this NSWindow's number on this application's queue, after GPUI releases its update borrow.
        unsafe {
            let number: i64 = msg_send![native, windowNumber];
            let process: id = msg_send![class!(NSProcessInfo), processInfo];
            let timestamp: f64 = msg_send![process, systemUptime];
            let height = f32::from(window.viewport_size().height);
            for (index, &[x, y]) in points.iter().enumerate() {
                let position = NSPoint::new(x as f64, (height - y) as f64);
                if index == 0 {
                    // AppKit asks GPUI which window-control hitbox is under the pointer before
                    // delivering mouseDown. Move first, just as an actual pointer would.
                    let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure_(
                        nil, NSEventType::NSMouseMoved, position, NSEventModifierFlags::empty(), timestamp, number, nil, 0, 0, 0.);
                    events.push(StrongPtr::retain(event));
                }
                if cancel && index + 1 == points.len() {
                    let escape = NSString::alloc(nil).init_str("\u{1b}");
                    let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode_(
                        nil, NSEventType::NSKeyDown, position, NSEventModifierFlags::empty(), timestamp, number, nil, escape, escape, NO, 53);
                    events.push(StrongPtr::retain(event));
                    let up = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode_(
                        nil, NSEventType::NSKeyUp, position, NSEventModifierFlags::empty(), timestamp, number, nil, escape, escape, NO, 53);
                    events.push(StrongPtr::retain(up));
                    let _: () = msg_send![escape, release];
                }
                let kind = if index == 0 {
                    NSEventType::NSLeftMouseDown
                } else if index + 1 == points.len() {
                    NSEventType::NSLeftMouseUp
                } else {
                    NSEventType::NSLeftMouseDragged
                };
                let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure_(
                    nil,
                    kind,
                    position,
                    NSEventModifierFlags::empty(),
                    timestamp + index as f64 * 0.01,
                    number,
                    nil,
                    event_number,
                    click_count.max(1),
                    if index + 1 == points.len() { 0. } else { 1. },
                );
                events.push(StrongPtr::retain(event));
            }
        }
        Ok(TestGesture {
            window: unsafe { StrongPtr::retain(native) },
            events,
        })
    }

    /// Retained fixture input is delivered after the owning GPUI update has finished.
    #[cfg(debug_assertions)]
    pub(crate) struct TestGesture {
        window: StrongPtr,
        events: Vec<StrongPtr>,
    }
    #[cfg(debug_assertions)]
    impl TestGesture {
        /// Activate only the fixture window before posting events, so activation clicks cannot hide a regression.
        #[allow(unexpected_cfgs)]
        pub(crate) fn post(self) {
            use cocoa::base::NO;
            // SAFETY: This object stays on the foreground executor and owns its exact test NSWindow.
            unsafe {
                NSApp().activateIgnoringOtherApps_(YES);
                let _: () = msg_send![*self.window, makeKeyAndOrderFront:nil];
                for event in &self.events {
                    NSApp().postEvent_atStart_(**event, NO);
                }
            }
        }
    }

    /// Register the localized Window menu; the pinned GPUI backend only recognizes the English title.
    #[allow(unexpected_cfgs)]
    pub(crate) fn install_system_menu(title: &str, minimize: &str, zoom: &str, fullscreen: &str) {
        // SAFETY: Application menus are rebuilt synchronously on the main AppKit thread.
        unsafe {
            let app = NSApp();
            let name = NSString::alloc(nil).init_str(title);
            let item: id = msg_send![app.mainMenu(), itemWithTitle:name];
            let _: () = msg_send![name, release];
            if item != nil {
                let menu: id = msg_send![item, submenu];
                if menu != nil {
                    // AppKit recognizes the standard responder selectors when generating window tools.
                    let count: isize = msg_send![menu, numberOfItems];
                    if count == 0 {
                        for (label, action, key) in [
                            (minimize, sel!(performMiniaturize:), "m"),
                            (zoom, sel!(performZoom:), ""),
                            (fullscreen, sel!(toggleFullScreen:), ""),
                        ] {
                            let label = NSString::alloc(nil).init_str(label);
                            let key = NSString::alloc(nil).init_str(key);
                            let item = NSMenuItem::alloc(nil)
                                .initWithTitle_action_keyEquivalent_(label, action, key);
                            item.setTarget_(nil);
                            menu.addItem_(item);
                            let _: () = msg_send![item, release];
                            let _: () = msg_send![label, release];
                            let _: () = msg_send![key, release];
                        }
                    }
                    app.setWindowsMenu_(menu);
                }
            }
        }
    }
    /// Ask AppKit to show its standard About panel, with version metadata for unbundled debug runs.
    #[allow(unexpected_cfgs)]
    pub(crate) fn show_about() -> anyhow::Result<()> {
        #[allow(non_upper_case_globals)]
        #[link(name = "AppKit", kind = "framework")]
        unsafe extern "C" {
            static NSAboutPanelOptionApplicationName: id;
            static NSAboutPanelOptionApplicationVersion: id;
        }
        // Called on the UI thread after the action's update cycle, without a Workbench borrow.
        unsafe {
            let app = NSApp();
            anyhow::ensure!(app != nil, "Application is unavailable");
            let options: id = msg_send![objc::class!(NSMutableDictionary), dictionary];
            let name = NSString::alloc(nil).init_str("MantaSH");
            let version = NSString::alloc(nil).init_str(crate::APP_VERSION);
            let _: () = msg_send![options, setObject:name forKey:NSAboutPanelOptionApplicationName];
            let _: () =
                msg_send![options, setObject:version forKey:NSAboutPanelOptionApplicationVersion];
            let _: () = msg_send![app, orderFrontStandardAboutPanelWithOptions:options];
            let _: () = msg_send![name, release];
            let _: () = msg_send![version, release];
        }
        Ok(())
    }

    /// Read the actual AppKit application menu in an isolated native QA run.
    #[cfg(debug_assertions)]
    #[allow(unexpected_cfgs)]
    fn application_menu() -> anyhow::Result<id> {
        unsafe {
            let bar: id = msg_send![NSApp(), mainMenu];
            anyhow::ensure!(bar != nil, "Application menu bar is unavailable");
            let item: id = msg_send![bar, itemAtIndex: 0isize];
            anyhow::ensure!(item != nil, "Application menu is unavailable");
            let menu: id = msg_send![item, submenu];
            anyhow::ensure!(menu != nil, "Application submenu is unavailable");
            Ok(menu)
        }
    }

    #[cfg(debug_assertions)]
    #[allow(unexpected_cfgs)]
    pub(crate) fn application_menu_snapshot() -> serde_json::Value {
        let Ok(menu) = application_menu() else {
            return serde_json::json!({"registered":false});
        };
        unsafe {
            let count: isize = msg_send![menu, numberOfItems];
            let items: Vec<_> = (0..count)
                .map(|index| {
                    let item: id = msg_send![menu, itemAtIndex:index];
                    let title: id = msg_send![item, title];
                    std::ffi::CStr::from_ptr(title.UTF8String())
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            serde_json::json!({"registered":true,"items":items})
        }
    }

    /// Inspect only this application's visible standard About panel in the isolated native QA process.
    #[cfg(debug_assertions)]
    #[allow(unexpected_cfgs)]
    fn about_panel() -> Option<id> {
        unsafe {
            let windows: id = msg_send![NSApp(), windows];
            let count: usize = msg_send![windows, count];
            (0..count).find_map(|index| {
                let panel: id = msg_send![windows, objectAtIndex:index];
                let visible: bool = msg_send![panel, isVisible];
                let native_panel: bool = msg_send![panel, isKindOfClass:objc::class!(NSPanel)];
                if !visible || !native_panel {
                    return None;
                }
                let content: id = msg_send![panel, contentView];
                (content != nil && panel_version(content, 0)).then_some(panel)
            })
        }
    }

    #[cfg(debug_assertions)]
    #[allow(unexpected_cfgs)]
    fn panel_version(view: id, depth: usize) -> bool {
        if depth > 16 {
            return false;
        }
        unsafe {
            let text_field: bool = msg_send![view, isKindOfClass:objc::class!(NSTextField)];
            if text_field {
                let value: id = msg_send![view, stringValue];
                if value != nil
                    && std::ffi::CStr::from_ptr(value.UTF8String())
                        .to_string_lossy()
                        .contains(crate::APP_VERSION)
                {
                    return true;
                }
            }
            let children: id = msg_send![view, subviews];
            let count: usize = msg_send![children, count];
            (0..count).any(|index| {
                let child: id = msg_send![children, objectAtIndex:index];
                panel_version(child, depth + 1)
            })
        }
    }

    #[cfg(debug_assertions)]
    #[allow(unexpected_cfgs)]
    pub(crate) fn about_panel_snapshot() -> serde_json::Value {
        let Some(panel) = about_panel() else {
            return serde_json::json!({"visible": false});
        };
        unsafe {
            let content: id = msg_send![panel, contentView];
            serde_json::json!({"visible": true, "version_visible": panel_version(content, 0)})
        }
    }

    #[cfg(debug_assertions)]
    #[allow(unexpected_cfgs)]
    pub(crate) fn close_about_panel() -> anyhow::Result<()> {
        let panel = about_panel().ok_or_else(|| anyhow::anyhow!("About panel is not visible"))?;
        unsafe {
            let _: () = msg_send![panel, performClose:nil];
        }
        Ok(())
    }

    #[cfg(debug_assertions)]
    pub(crate) struct PreparedAboutMenu(StrongPtr);

    /// Select only the About item in this process's app menu, after GPUI releases the window borrow.
    #[cfg(debug_assertions)]
    #[allow(unexpected_cfgs)]
    pub(crate) fn prepare_about_menu(title: &str) -> anyhow::Result<PreparedAboutMenu> {
        let menu = application_menu()?;
        unsafe {
            let item: id = msg_send![menu, itemAtIndex:0isize];
            anyhow::ensure!(item != nil, "About menu item is unavailable");
            let item_title: id = msg_send![item, title];
            let actual = std::ffi::CStr::from_ptr(item_title.UTF8String()).to_string_lossy();
            anyhow::ensure!(
                actual == title,
                "The first application menu item is not About"
            );
            Ok(PreparedAboutMenu(StrongPtr::retain(menu)))
        }
    }
    #[cfg(debug_assertions)]
    impl PreparedAboutMenu {
        #[allow(unexpected_cfgs)]
        pub(crate) fn activate(self) {
            // SAFETY: The retained menu belongs to this process and AppKit runs after GPUI's update.
            unsafe {
                let _: () = msg_send![*self.0, update];
                let _: () = msg_send![*self.0, performActionForItemAtIndex:0isize];
            }
        }
    }
    /// Inspect only this application's registered menu for isolated native QA.
    #[cfg(debug_assertions)]
    #[allow(unexpected_cfgs)]
    pub(crate) fn system_menu_snapshot() -> serde_json::Value {
        // SAFETY: Read-only AppKit access on the current application's UI thread.
        unsafe {
            let menu: id = msg_send![NSApp(), windowsMenu];
            if menu == nil {
                return serde_json::json!({"registered":false});
            }
            let count: isize = msg_send![menu, numberOfItems];
            let mut titles = Vec::new();
            for index in 0..count {
                let item: id = msg_send![menu, itemAtIndex:index];
                let title: id = msg_send![item, title];
                let text = std::ffi::CStr::from_ptr(title.UTF8String())
                    .to_string_lossy()
                    .into_owned();
                titles.push(text);
            }
            serde_json::json!({"registered":true,"items":titles})
        }
    }

    /// Retain this application's system menu and its own content view while the popup is queued.
    pub(crate) struct PreparedMenu {
        menu: StrongPtr,
        view: StrongPtr,
        point: NSPoint,
    }

    /// Resolve menu positioning without invoking a nested native event loop under a GPUI borrow.
    #[allow(unexpected_cfgs)]
    pub(crate) fn prepare_system_menu(window: &Window) -> anyhow::Result<PreparedMenu> {
        let native = handle(window)?;
        // SAFETY: Both retained objects belong to this process and are used only on the UI thread.
        unsafe {
            let menu: id = msg_send![NSApp(), windowsMenu];
            anyhow::ensure!(menu != nil, "System Window menu is unavailable");
            let view = native.contentView();
            let flipped: bool = msg_send![view, isFlipped];
            Ok(PreparedMenu {
                menu: StrongPtr::retain(menu),
                view: StrongPtr::retain(view),
                point: NSPoint::new(
                    f32::from(window.viewport_size().width).max(40.) as f64 - 40.,
                    if flipped {
                        40.
                    } else {
                        f32::from(window.viewport_size().height) as f64 - 40.
                    },
                ),
            })
        }
    }
    impl PreparedMenu {
        /// Open the native menu for QA and dismiss it through AppKit's public tracking API, without input.
        #[cfg(debug_assertions)]
        #[allow(unexpected_cfgs)]
        pub(crate) fn probe(self) {
            use cocoa::foundation::NSArray;
            // SAFETY: Schedule cancellation on the same UI run loop; the retained menu outlives tracking.
            unsafe {
                let mode = NSString::alloc(nil).init_str("NSEventTrackingRunLoopMode");
                let modes = NSArray::arrayWithObjects(nil, &[mode]);
                let _: () = msg_send![*self.menu, performSelector:sel!(cancelTracking) withObject:nil afterDelay:0.5f64 inModes:modes];
                let _: () = msg_send![mode, release];
            }
            self.show();
        }
        /// Show the actual AppKit menu, including tiling options supplied by the running macOS version.
        #[allow(unexpected_cfgs)]
        pub(crate) fn show(self) {
            // SAFETY: Called on GPUI's foreground executor after releasing Workbench/Window borrows.
            unsafe {
                let _: bool = msg_send![*self.menu, popUpMenuPositioningItem:nil atLocation:self.point inView:*self.view];
            }
        }
    }

    /// Convert AppKit's bottom-left screen coordinates to monitor-local top-left logical pixels.
    fn local(rect: NSRect, screen: NSRect) -> Rect {
        Rect {
            x: (rect.origin.x - screen.origin.x) as f32,
            y: (screen.origin.y + screen.size.height - rect.origin.y - rect.size.height) as f32,
            width: rect.size.width as f32,
            height: rect.size.height as f32,
        }
    }

    /// Read the current window and usable screen without changing focus or geometry.
    pub(crate) fn info(window: &Window, _: &App) -> anyhow::Result<Info> {
        let native = handle(window)?;
        // SAFETY: Native window and screen are only accessed synchronously on the UI thread.
        unsafe {
            let screen = native.screen();
            anyhow::ensure!(screen != nil, "Window has no screen");
            let screen_frame = NSScreen::frame(screen);
            Ok(Info {
                frame: local(NSWindow::frame(native), screen_frame),
                work_area: local(NSScreen::visibleFrame(screen), screen_frame),
                positioning: true,
            })
        }
    }

    /// Retain the exact window until the queued UI task applies the geometry.
    pub(crate) struct Prepared {
        window: StrongPtr,
        target: Rect,
    }
    /// Capture this exact window for a later foreground geometry update.
    pub(crate) fn prepare(window: &Window, target: Rect) -> anyhow::Result<Prepared> {
        let native = handle(window)?;
        // SAFETY: Retaining here prevents a released NSWindow pointer from being reused before application.
        Ok(Prepared {
            window: unsafe { StrongPtr::retain(native) },
            target,
        })
    }
    impl Prepared {
        /// Run outside a Workbench/Window update borrow because AppKit resize callbacks are synchronous.
        pub(crate) fn apply(self) -> anyhow::Result<()> {
            // SAFETY: Prepared never leaves GPUI's foreground executor and owns this NSWindow reference.
            unsafe {
                let native = *self.window;
                let screen = native.screen();
                anyhow::ensure!(screen != nil, "Window has no screen");
                let frame = NSScreen::frame(screen);
                let target = NSRect::new(
                    NSPoint::new(
                        frame.origin.x + self.target.x as f64,
                        frame.origin.y + frame.size.height
                            - self.target.y as f64
                            - self.target.height as f64,
                    ),
                    NSSize::new(self.target.width as f64, self.target.height as f64),
                );
                native.setFrame_display_(target, YES);
            }
            Ok(())
        }
    }
}

#[cfg(target_os = "windows")]
mod native {
    use super::*;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::{
        Foundation::{HWND, RECT},
        Graphics::Gdi::{
            GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
        },
        UI::WindowsAndMessaging::{
            GetWindowRect, IsWindow, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos,
        },
    };

    /// Borrow only this GPUI window's HWND.
    fn handle(window: &Window) -> anyhow::Result<HWND> {
        let RawWindowHandle::Win32(handle) = HasWindowHandle::window_handle(window)
            .map_err(|error| anyhow::anyhow!("Native window handle unavailable: {error:?}"))?
            .as_raw()
        else {
            anyhow::bail!("Not a Win32 window");
        };
        Ok(handle.hwnd.get() as HWND)
    }
    /// The Windows Shell owns the About dialog; no GPUI view or secondary workbench is created.
    pub(crate) struct PreparedAbout {
        hwnd: usize,
        label: Vec<u16>,
    }

    pub(crate) fn prepare_about(window: &Window, version: &str) -> anyhow::Result<PreparedAbout> {
        Ok(PreparedAbout {
            hwnd: handle(window)? as usize,
            label: format!("MantaSH#{version}")
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect(),
        })
    }

    impl PreparedAbout {
        pub(crate) fn show(self) -> anyhow::Result<()> {
            use windows_sys::Win32::UI::Shell::ShellAboutW;
            let hwnd = self.hwnd as HWND;
            anyhow::ensure!(
                unsafe { IsWindow(hwnd) } != 0,
                "Window is no longer available"
            );
            // ShellAboutW owns its native modal loop; execute after releasing GPUI's borrow.
            anyhow::ensure!(
                unsafe {
                    ShellAboutW(
                        hwnd,
                        self.label.as_ptr(),
                        std::ptr::null(),
                        std::ptr::null_mut(),
                    )
                } != 0,
                "Windows About dialog could not be opened"
            );
            Ok(())
        }
    }

    /// Read monitor work bounds in physical pixels, then convert with this window's DPI scale.
    fn monitor(hwnd: HWND) -> anyhow::Result<MONITORINFO> {
        let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        // SAFETY: Win32 writes to a correctly sized stack structure for this live HWND.
        anyhow::ensure!(
            unsafe {
                GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info)
            } != 0,
            "Monitor unavailable"
        );
        Ok(info)
    }
    fn local(rect: RECT, screen: RECT, scale: f32) -> Rect {
        Rect {
            x: (rect.left - screen.left) as f32 / scale,
            y: (rect.top - screen.top) as f32 / scale,
            width: (rect.right - rect.left) as f32 / scale,
            height: (rect.bottom - rect.top) as f32 / scale,
        }
    }
    /// Read the current window and usable screen without changing focus or geometry.
    pub(crate) fn info(window: &Window, _: &App) -> anyhow::Result<Info> {
        let hwnd = handle(window)?;
        let screen = monitor(hwnd)?;
        let mut frame: RECT = unsafe { std::mem::zeroed() };
        // SAFETY: The pointer references a valid RECT and the HWND belongs to this GPUI window.
        anyhow::ensure!(
            unsafe { GetWindowRect(hwnd, &mut frame) } != 0,
            "Window unavailable"
        );
        let scale = window.scale_factor().max(0.1);
        Ok(Info {
            frame: local(frame, screen.rcMonitor, scale),
            work_area: local(screen.rcWork, screen.rcMonitor, scale),
            positioning: true,
        })
    }
    pub(crate) struct Prepared {
        hwnd: HWND,
        target: Rect,
        scale: f32,
    }
    /// Capture this exact window for a later foreground geometry update.
    pub(crate) fn prepare(window: &Window, target: Rect) -> anyhow::Result<Prepared> {
        Ok(Prepared {
            hwnd: handle(window)?,
            target,
            scale: window.scale_factor().max(0.1),
        })
    }
    impl Prepared {
        /// Apply on the UI executor after verifying that the original native handle remains live.
        pub(crate) fn apply(self) -> anyhow::Result<()> {
            anyhow::ensure!(unsafe { IsWindow(self.hwnd) } != 0, "Window unavailable");
            let screen = monitor(self.hwnd)?.rcMonitor;
            // SAFETY: Geometry is finite and bounded, and no other HWND is selected or activated.
            let ok = unsafe {
                SetWindowPos(
                    self.hwnd,
                    std::ptr::null_mut(),
                    screen.left + (self.target.x * self.scale).round() as i32,
                    screen.top + (self.target.y * self.scale).round() as i32,
                    (self.target.width * self.scale).round() as i32,
                    (self.target.height * self.scale).round() as i32,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                )
            };
            anyhow::ensure!(ok != 0, "Window manager rejected geometry");
            Ok(())
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod native {
    use super::*;
    pub(crate) struct PreparedAbout;
    pub(crate) fn prepare_about(_: &Window, _: &str) -> anyhow::Result<PreparedAbout> {
        anyhow::bail!("System About is available only on macOS and Windows")
    }
    impl PreparedAbout {
        pub(crate) fn show(self) -> anyhow::Result<()> {
            anyhow::bail!("System About is available only on macOS and Windows")
        }
    }

    /// Read the current window and usable screen without changing focus or geometry.
    pub(crate) fn info(window: &Window, cx: &App) -> anyhow::Result<Info> {
        let bounds = window.bounds();
        let area = window
            .display(cx)
            .ok_or_else(|| anyhow::anyhow!("Display unavailable"))?
            .bounds();
        let rect = |b: gpui::Bounds<gpui::Pixels>| Rect {
            x: b.origin.x.into(),
            y: b.origin.y.into(),
            width: b.size.width.into(),
            height: b.size.height.into(),
        };
        Ok(Info {
            frame: rect(bounds),
            work_area: rect(area),
            positioning: false,
        })
    }
    pub(crate) struct Prepared;
    pub(crate) fn prepare(_: &Window, _: Rect) -> anyhow::Result<Prepared> {
        anyhow::bail!("Window positioning is managed by the compositor")
    }
    impl Prepared {
        pub(crate) fn apply(self) -> anyhow::Result<()> {
            anyhow::bail!("Window positioning is managed by the compositor")
        }
    }
}

#[cfg(target_os = "windows")]
pub(crate) use native::prepare_about;
#[cfg(all(target_os = "macos", debug_assertions))]
pub(crate) use native::prepare_test_gesture;
#[cfg(all(target_os = "macos", debug_assertions))]
pub(crate) use native::system_menu_snapshot;
#[cfg(all(target_os = "macos", debug_assertions))]
pub(crate) use native::{
    about_panel_snapshot, application_menu_snapshot, close_about_panel, prepare_about_menu,
};
pub(crate) use native::{info, prepare};
#[cfg(target_os = "macos")]
pub(crate) use native::{install_system_menu, prepare_system_menu, show_about};
