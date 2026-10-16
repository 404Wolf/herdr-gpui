//! Native check of pages beneath the window's drawing, in the daemon-free
//! fixture window. A real page in the right of two groups must lie below
//! GPUI's view, where it was laid out, with its router above GPUI's view.
//! AppKit's hit test must hand a click on the page to the page, and AppKit's
//! drag hit test must reach the page. GPUI's layer must be transparent in an
//! opaque window.
//!
//! Checking needs AppKit calls with no safe binding: reading a view's
//! superview and layer, a synthetic mouse move delivered to GPUI's view
//! alone, and AppKit's private drag hit test, which only this check calls.
#![allow(unsafe_code)]

use super::*;
use crate::browser::{Location, Store, TabId, WebUrl};
use objc2::{
    ClassType, Message as _, class, msg_send,
    rc::Retained,
    runtime::{AnyObject, NSObjectProtocol as _},
};
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType, NSView};
use objc2_foundation::NSPoint;
use objc2_web_kit::WKWebView;
use wry::raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// The views the check reads, and where it points, in the content view's
/// coordinates.
struct Native {
    gpui: Retained<NSView>,
    content: Retained<NSView>,
    page: Retained<NSView>,
    window_number: isize,
    /// The page's middle.
    center: NSPoint,
}

pub(super) fn start_pages(handle: WindowHandle<HerdrWindow>, cx: &mut App) {
    EXIT_CODE.store(1, Ordering::SeqCst);
    let timer = cx.background_executor().clone();
    cx.spawn(async move |cx| {
        let result = async {
            let id = handle
                .update(cx, open_page)
                .map_err(|error| anyhow!("{error:#}"))??;
            // Let the page show, lay out, and paint a few frames.
            for _ in 0..10 {
                timer.timer(Duration::from_millis(100)).await;
                AnyWindowHandle::from(handle)
                    .update(cx, |_, window, cx| window.draw(cx).clear(cx))
                    .map_err(|error| anyhow!("{error:#}"))?;
            }
            let native = handle
                .update(cx, |view, window, _| locate(view, window, id))
                .map_err(|error| anyhow!("{error:#}"))??;
            // Outside every GPUI borrow, as GPUI handles the mouse move
            // there and then.
            check(&native)
        }
        .await;
        match result {
            Ok(summary) => {
                eprintln!("PAGES native PASS: {summary}");
                std::process::exit(0);
            }
            Err(error) => {
                eprintln!("PAGES native FAIL: {error:#}");
                std::process::exit(1);
            }
        }
    })
    .detach();
}

/// Splits the fixture's one workspace and opens a page in the right group.
/// The address refuses connections, so nothing leaves the machine.
fn open_page(
    view: &mut HerdrWindow,
    window: &mut Window,
    cx: &mut Context<HerdrWindow>,
) -> Result<TabId> {
    window.resize(size(px(1200.), px(780.)));
    let mut shown = sidebar::layout_tests::snapshot(1);
    shown.focused_workspace_id = Some("w0".into());
    shown.focused_tab_id = Some("t0".into());
    view.live.snapshot = Some(Arc::new(shown));
    view.command(Command::SplitEditor, window, cx);
    let right = view
        .group_slots()
        .get(1)
        .map(|slot| slot.id)
        .context("the split made no second group")?;
    let url = WebUrl::try_from("http://127.0.0.1:9/")?;
    let scope = crate::browser::scope(&view.endpoints[view.selected_endpoint]);
    let id = Store::update(cx, |store| {
        store.open(scope, "w0", Some(Location::Web { url }), None)
    })
    .context("no browser tab opened")?;
    view.show_browser_tab_in(Some(right), id, window, cx);
    Ok(id)
}

fn locate(view: &HerdrWindow, window: &Window, id: TabId) -> Result<Native> {
    let page_at = view
        .browser
        .drawn_pages()
        .borrow()
        .get(&id)
        .copied()
        .context("the page never laid out")?;
    let handle = HasWindowHandle::window_handle(window)?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        bail!("not an AppKit window");
    };
    // SAFETY: the handle borrows `window`, whose GPUI view is a live NSView
    // for that borrow, retained before it ends, on the main thread.
    let gpui = unsafe { handle.ns_view.cast::<NSView>().as_ref() }.retain();
    // SAFETY: a property read on GPUI's view, on the main thread.
    let content = unsafe { gpui.superview() }.context("GPUI's view has no superview")?;
    let page = content
        .subviews()
        .iter()
        .find(|view| view.isKindOfClass(WKWebView::class()))
        .context("no page in the content view")?;
    let window_number = gpui
        .window()
        .context("GPUI's view has no window")?
        .windowNumber();
    let height = content.bounds().size.height;
    let appkit = |at: Point<Pixels>| NSPoint::new(f64::from(at.x), height - f64::from(at.y));
    Ok(Native {
        center: appkit(page_at.center()),
        gpui,
        content,
        page,
        window_number,
    })
}

fn check(native: &Native) -> Result<String> {
    let subviews = native.content.subviews();
    let index = |target: &NSView| {
        subviews
            .iter()
            .position(|view| std::ptr::eq(&*view, target))
    };
    let (Some(page), Some(gpui)) = (index(&native.page), index(&native.gpui)) else {
        bail!("the page and GPUI's view are not siblings");
    };
    if page > gpui {
        bail!("the page lies above GPUI's view");
    }
    let router = subviews
        .iter()
        .position(|view| view.class().name() == c"HerdrPageRouter")
        .context("the page has no router")?;
    if router < gpui {
        bail!("the router lies beneath GPUI's view");
    }
    // SAFETY: a property read on GPUI's view, on the main thread.
    let layer: Option<Retained<AnyObject>> = unsafe { msg_send![&*native.gpui, layer] };
    let layer = layer.context("GPUI's view has no layer")?;
    // SAFETY: `isOpaque` is a BOOL property of every CALayer.
    let layer_opaque: bool = unsafe { msg_send![&*layer, isOpaque] };
    if layer_opaque {
        bail!("GPUI's layer is opaque, so it hides the page");
    }
    if !native.gpui.window().is_some_and(|window| window.isOpaque()) {
        bail!("the opaque window lets the desktop through");
    }
    point_at(native, native.center)?;
    let on_page = native.content.hitTest(native.center);
    if !on_page.is_some_and(|view| view.isDescendantOf(&native.page)) {
        bail!("a click on the page does not reach it");
    }
    let types = native.page.registeredDraggedTypes();
    // SAFETY: `setWithArray:` takes an array and returns a new set.
    let types: Retained<AnyObject> = unsafe { msg_send![class!(NSSet), setWithArray: &*types] };
    let mut at = native.center;
    // SAFETY: AppKit's own drag hit test on NSView, which takes a point in
    // the receiver's superview's coordinates by reference and a set of
    // pasteboard types, and returns a view or nil.
    let dropped: *mut AnyObject = unsafe {
        msg_send![&*native.content, _hitTest: &mut at as *mut NSPoint, dragTypes: &*types]
    };
    // SAFETY: AppKit returns a live view in the window, or nil.
    let dropped = unsafe { dropped.cast::<NSView>().as_ref() };
    if !dropped.is_some_and(|view| view.isDescendantOf(&native.page)) {
        bail!("a drag over the page does not reach it");
    }
    Ok(format!(
        "page {page} < GPUI view {gpui} < router {router}; GPUI's layer transparent in an opaque window; page click and drag each reach the page"
    ))
}

/// Tells GPUI the pointer is at `at`, as a mouse move to its view alone
/// would: nothing is posted to the system, and the real pointer stays.
fn point_at(native: &Native, at: NSPoint) -> Result<()> {
    // SAFETY: the documented constructor for a mouse event, with no
    // graphics context, as AppKit asks.
    let event: Option<Retained<NSEvent>> = unsafe {
        msg_send![
            NSEvent::class(),
            mouseEventWithType: NSEventType::MouseMoved,
            location: at,
            modifierFlags: NSEventModifierFlags::empty(),
            timestamp: 0_f64,
            windowNumber: native.window_number,
            context: None::<&AnyObject>,
            eventNumber: 0_isize,
            clickCount: 0_isize,
            pressure: 0_f32
        ]
    };
    let event = event.context("no mouse event")?;
    native.gpui.mouseMoved(&event);
    Ok(())
}
