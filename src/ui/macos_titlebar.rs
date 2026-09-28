//! Exclude interactive title-bar regions from AppKit's Window Server drag map.
#![allow(deprecated, unexpected_cfgs)] // Match the Cocoa ABI of the pinned GPUI backend.
use cocoa::{
    appkit::NSView,
    base::{BOOL, NO, YES, id, nil},
    foundation::{NSPoint, NSRect, NSSize},
};
use gpui::{Bounds, Pixels, Window};
use objc::{
    class,
    declare::ClassDecl,
    msg_send,
    rc::{StrongPtr, WeakPtr},
    runtime::{Class, Object, Sel},
    sel, sel_impl,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

/// A transparent region marker must leave mouse/scroll delivery with the original GPUIView.
extern "C" fn pass_through(_: &Object, _: Sel, _: NSPoint) -> id {
    nil
}

/// Prevent background dragging in this view's bounds without disabling NSWindow movement.
extern "C" fn cannot_move_window(_: &Object, _: Sel) -> BOOL {
    NO
}

/// AppKit queries this separately from hitTest/mouseDownCanMoveWindow for full-size title bars.
/// A local NSEvent monitor is too late: Window Server can start the drag without sending the
/// app a mouse-down. This narrowly scoped compatibility hook is also used by Firefox/GPUI;
/// see docs/development.md. It changes native drag geometry, not drawing opacity or input.
extern "C" fn titlebar_exclusion(this: &mut Object, _: Sel) -> NSRect {
    // SAFETY: AppKit invokes NSView methods on the UI thread; the ivar belongs to our class.
    unsafe {
        #[cfg(debug_assertions)]
        {
            let queries = *this.get_ivar::<u64>("mantashRegionQueries");
            this.set_ivar("mantashRegionQueries", queries.wrapping_add(1));
        }
        msg_send![this, bounds]
    }
}

/// Register an application-owned view class; never subclass or swizzle GPUI's responder classes.
fn exclusion_class() -> anyhow::Result<&'static Class> {
    if let Some(class) = Class::get("MantaSHTitlebarExclusionView") {
        return Ok(class);
    }
    let mut class = ClassDecl::new("MantaSHTitlebarExclusionView", class!(NSView))
        .ok_or_else(|| anyhow::anyhow!("Could not register the title bar region view"))?;
    #[cfg(debug_assertions)]
    class.add_ivar::<u64>("mantashRegionQueries");
    // SAFETY: Each selector's argument and return ABI matches NSView's implementation.
    unsafe {
        class.add_method(
            sel!(hitTest:),
            pass_through as extern "C" fn(&Object, Sel, NSPoint) -> id,
        );
        class.add_method(
            sel!(mouseDownCanMoveWindow),
            cannot_move_window as extern "C" fn(&Object, Sel) -> BOOL,
        );
        class.add_method(
            sel!(_opaqueRectForWindowMoveWhenInTitlebar),
            titlebar_exclusion as extern "C" fn(&mut Object, Sel) -> NSRect,
        );
    }
    Ok(class.register())
}

struct Regions {
    view: WeakPtr,
    blockers: RefCell<Vec<StrongPtr>>,
    attached: Cell<bool>,
}
impl Regions {
    /// Detach only our geometry markers when their owning window closes.
    fn stop(&self) {
        self.attached.set(false);
        for blocker in self.blockers.borrow_mut().drain(..) {
            // SAFETY: All region views and their owner stay on AppKit's main thread.
            unsafe {
                let _: () = msg_send![*blocker, removeFromSuperview];
            }
        }
    }
}
impl Drop for Regions {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Keep transparent native region markers aligned with the rendered tabs and buttons.
#[derive(Clone)]
pub(super) struct TitlebarRegions(Rc<Regions>);
impl TitlebarRegions {
    /// Resolve only this window's GPUIView, without retaining the window or intercepting events.
    pub(super) fn new(window: &Window) -> anyhow::Result<Self> {
        let RawWindowHandle::AppKit(handle) = HasWindowHandle::window_handle(window)
            .map_err(|error| anyhow::anyhow!("Native window handle unavailable: {error:?}"))?
            .as_raw()
        else {
            anyhow::bail!("The title bar needs an AppKit content view");
        };
        exclusion_class()?;
        // SAFETY: The raw handle is a live GPUIView on the UI thread; WeakPtr avoids a view cycle.
        unsafe {
            let view = handle.ns_view.as_ptr() as id;
            let native: id = msg_send![view, window];
            anyhow::ensure!(native != nil, "The title bar window is unavailable");
            Ok(Self(Rc::new(Regions {
                view: WeakPtr::new(view),
                blockers: RefCell::new(Vec::new()),
                attached: Cell::new(true),
            })))
        }
    }
    /// Apply clipped layout rectangles before the next input, reusing unchanged native views.
    pub(super) fn set_regions(&self, mut regions: Vec<Bounds<Pixels>>) {
        if !self.0.attached.get() {
            return;
        }
        let view = self.0.view.load();
        if *view == nil {
            return;
        }
        // Control bounds come from a HashMap. Stable ordering avoids invalidating AppKit's drag
        // map on every terminal output frame when the actual geometry has not changed.
        regions.sort_by(|a, b| {
            f32::from(a.left())
                .total_cmp(&f32::from(b.left()))
                .then(f32::from(a.top()).total_cmp(&f32::from(b.top())))
        });
        let mut blockers = self.0.blockers.borrow_mut();
        let mut changed = false;
        // SAFETY: This runs in the native layout/paint pass on the UI thread. NSView geometry
        // and native drag-map invalidation stay within this exact window.
        unsafe {
            let bounds = NSView::bounds(*view);
            let flipped: BOOL = msg_send![*view, isFlipped];
            for (index, region) in regions.iter().enumerate() {
                let frame = NSRect::new(
                    NSPoint::new(
                        bounds.origin.x + f64::from(f32::from(region.left())),
                        bounds.origin.y
                            + if flipped == YES {
                                f64::from(f32::from(region.top()))
                            } else {
                                bounds.size.height - f64::from(f32::from(region.bottom()))
                            },
                    ),
                    NSSize::new(
                        f64::from(f32::from(region.size.width)),
                        f64::from(f32::from(region.size.height)),
                    ),
                );
                if let Some(blocker) = blockers.get(index) {
                    let previous = NSView::frame(**blocker);
                    if previous.origin.x != frame.origin.x
                        || previous.origin.y != frame.origin.y
                        || previous.size.width != frame.size.width
                        || previous.size.height != frame.size.height
                    {
                        changed = true;
                        #[cfg(debug_assertions)]
                        (***blocker).set_ivar("mantashRegionQueries", 0_u64);
                        let _: () = msg_send![**blocker, setFrame:frame];
                    }
                } else {
                    // Registration was checked at construction and Cocoa classes are permanent.
                    let class = class!(MantaSHTitlebarExclusionView);
                    let blocker: id = msg_send![class, alloc];
                    let blocker: id = msg_send![blocker, initWithFrame:frame];
                    let blocker = StrongPtr::new(blocker);
                    let _: () = msg_send![*view, addSubview:*blocker];
                    blockers.push(blocker);
                    changed = true;
                }
            }
            for blocker in blockers.drain(regions.len()..) {
                let _: () = msg_send![*blocker, removeFromSuperview];
                changed = true;
            }
            if changed {
                // GPUI uses its own drawing loop. Frame changes alone can leave AppKit's
                // server-side drag map cached (observed after closing a tab). Changing this
                // property and immediately restoring its original value forces a fresh map,
                // as in Firefox's UpdateWindowDraggingRegion. No event dispatch occurs between
                // the setters; isMovable and the final background-drag setting remain intact.
                let native: id = msg_send![*view, window];
                let background: BOOL = msg_send![native, isMovableByWindowBackground];
                let _: () = msg_send![native, setMovableByWindowBackground:if background == YES { NO } else { YES }];
                let _: () = msg_send![native, setMovableByWindowBackground:background];
            }
        }
    }
    /// Detach markers while the owner still exists; dropping the last handle is a fallback.
    pub(super) fn stop(&self) {
        self.0.stop();
    }
    /// Read native geometry and AppKit query counts; never call the exclusion hook from QA.
    #[cfg(debug_assertions)]
    pub(super) fn snapshot(&self) -> serde_json::Value {
        let view = self.0.view.load();
        if *view == nil {
            return serde_json::Value::Null;
        }
        // SAFETY: Read only this window's AppKit objects on the UI thread. Query counts are
        // incremented by AppKit itself, not by snapshot or the synthetic pointer test driver.
        unsafe {
            let native: id = msg_send![*view, window];
            let movable: BOOL = msg_send![native, isMovable];
            let background: BOOL = msg_send![native, isMovableByWindowBackground];
            let bounds = NSView::bounds(*view);
            let flipped: BOOL = msg_send![*view, isFlipped];
            let regions = self.0.blockers.borrow().iter().map(|blocker| {
                let frame = NSView::frame(**blocker);
                let hit: id = msg_send![**blocker, hitTest:NSPoint::new(frame.origin.x + frame.size.width / 2., frame.origin.y + frame.size.height / 2.)];
                let parent: id = msg_send![**blocker, superview];
                serde_json::json!({"x":frame.origin.x - bounds.origin.x,"y":if flipped==YES { frame.origin.y - bounds.origin.y } else { bounds.size.height - (frame.origin.y - bounds.origin.y) - frame.size.height },
                    "width":frame.size.width,"height":frame.size.height,"attached":parent==*view,"passes_input":hit==nil,
                    "appkit_queries":*(***blocker).get_ivar::<u64>("mantashRegionQueries")})
            }).collect::<Vec<_>>();
            serde_json::json!({"installed":self.0.attached.get(),"movable":movable==YES,
                "background_movable":background==YES,"regions":regions})
        }
    }
}
