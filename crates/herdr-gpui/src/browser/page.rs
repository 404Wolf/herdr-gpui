//! A page in the window: the native view WebKit or WebView2 draws a page
//! into, and the element that lays that view out where its group draws.
//!
//! The view is a child of the window's own view, so it draws above
//! everything the window draws, and the window hides a page that one of its
//! overlays reaches (`view::present`).
use {
    gpui::{
        App, Bounds, DispatchPhase, Element, ElementId, GlobalElementId, Hitbox, HitboxBehavior,
        InspectorElementId, IntoElement, LayoutId, MouseDownEvent, Pixels, Size, Style, Window,
    },
    std::{cell::Cell, rc::Rc},
};

/// One tab's native page.
pub(crate) struct Page {
    view: wry::WebView,
    /// Read as the page lays out, after the window shows or hides it for
    /// the frame: a hidden page stays where it was moved out of the way.
    shown: Cell<bool>,
}

impl Page {
    /// Takes over `view`, which wry built as a child of `window`.
    pub(crate) fn new(view: wry::WebView, window: &Window) -> Self {
        let _ = window;
        // Nothing until the window lays it out.
        let _ = view.set_bounds(wry::Rect::default());
        // Shown only once the window presents it.
        Self {
            view,
            shown: Cell::new(false),
        }
    }

    pub(crate) fn raw(&self) -> &wry::WebView {
        &self.view
    }

    pub(crate) fn show(&self) {
        self.shown.set(true);
        let _ = self.view.set_visible(true);
    }

    /// Hides the page, first taking the keyboard back from it: a hidden view
    /// would go on receiving keys.
    pub(crate) fn hide(&self) {
        self.shown.set(false);
        let _ = self.view.focus_parent();
        let _ = self.view.set_visible(false);
    }

    /// Moves keyboard input into the page.
    pub(crate) fn focus(&self) {
        let _ = self.view.focus();
    }

    /// Hands the keyboard back from the page to the window.
    pub(crate) fn blur(&self) {
        let _ = self.view.focus_parent();
    }

    /// Puts the page at `bounds`, in the window's points.
    fn place(&self, bounds: Bounds<Pixels>) {
        let _ = self.view.set_bounds(wry::Rect {
            size: wry::dpi::Size::Logical(wry::dpi::LogicalSize::new(
                bounds.size.width.into(),
                bounds.size.height.into(),
            )),
            position: wry::dpi::Position::Logical(wry::dpi::LogicalPosition::new(
                bounds.origin.x.into(),
                bounds.origin.y.into(),
            )),
        });
    }
}

impl Drop for Page {
    fn drop(&mut self) {
        self.hide();
    }
}

/// The element a page lays out in: it fills its space, and the native view
/// follows wherever that is.
pub(crate) struct PageElement {
    page: Rc<Page>,
}

impl PageElement {
    pub(crate) fn new(page: Rc<Page>) -> Self {
        Self { page }
    }
}

impl IntoElement for PageElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for PageElement {
    type RequestLayoutState = ();
    type PrepaintState = Option<Hitbox>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let style = Style {
            size: Size::full(),
            flex_shrink: 1.,
            ..Default::default()
        };
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        _: &mut App,
    ) -> Self::PrepaintState {
        // A hidden page stays where it was moved out of the way.
        if !self.page.shown.get() {
            return None;
        }
        self.page.place(bounds);
        Some(window.insert_hitbox(bounds, HitboxBehavior::Normal))
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        _: &mut App,
    ) {
        let Some(hitbox) = hitbox.take() else {
            return;
        };
        // A press elsewhere in the window takes the keyboard back.
        let page = self.page.clone();
        let area = hitbox.bounds;
        window.on_mouse_event(move |event: &MouseDownEvent, phase, _, _| {
            if phase == DispatchPhase::Bubble && !area.contains(&event.position) {
                page.blur();
            }
        });
    }
}
