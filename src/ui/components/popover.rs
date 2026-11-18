use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    AnchoredPositionMode, AnyElement, App, Corner, ElementId, MouseButton, MouseDownEvent, Pixels,
    Point, Window, anchored, deferred, div, point, px,
};

use crate::ui::theme::menu;

type DismissHandler = Rc<dyn Fn(&MouseDownEvent, &mut Window, &mut App) + 'static>;

/// What it is attached to (a button, say), and while it is open the menu
/// it opens: over everything else and kept inside the window. A click
/// anywhere else dismisses it, and does nothing else. It has no size of
/// its own: the menu in it (a [`Panel`](super::Panel)) has.
#[derive(IntoElement)]
pub struct Popover {
    id: ElementId,
    trigger: AnyElement,
    menu: Option<AnyElement>,
    anchor: Corner,
    position: Point<Pixels>,
    on_dismiss: Option<DismissHandler>,
}

impl Popover {
    /// `id` names the backdrop that takes the clicks around the menu.
    pub fn new(id: impl Into<ElementId>, trigger: impl IntoElement) -> Self {
        Self {
            id: id.into(),
            trigger: trigger.into_any_element(),
            menu: None,
            anchor: Corner::TopLeft,
            position: point(px(0.), px(0.)),
            on_dismiss: None,
        }
    }

    /// The menu, which shows while it is set: set it while open.
    pub fn menu(mut self, menu: impl IntoElement) -> Self {
        self.menu = Some(menu.into_any_element());
        self
    }

    /// Which of the menu's corners goes at `position`, from the trigger's
    /// top left.
    pub fn anchor(mut self, anchor: Corner, position: Point<Pixels>) -> Self {
        self.anchor = anchor;
        self.position = position;
        self
    }

    /// Called on a click outside the menu while it is open, to close it.
    pub fn on_dismiss(
        mut self,
        handler: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_dismiss = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Popover {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let viewport = window.viewport_size();
        let dismiss = move |event: &MouseDownEvent, window: &mut Window, cx: &mut App| {
            cx.stop_propagation();
            if let Some(handler) = &self.on_dismiss {
                handler(event, window, cx);
            }
        };
        let dismiss = Rc::new(dismiss);
        div()
            .relative()
            .flex_none()
            .child(self.trigger)
            .when_some(self.menu, |this, menu| {
                this.child(
                    deferred(
                        anchored()
                            .position_mode(AnchoredPositionMode::Local)
                            .anchor(self.anchor)
                            .position(self.position)
                            .snap_to_window_with_margin(menu::WINDOW_MARGIN)
                            .child(menu),
                    )
                    .with_priority(2),
                )
                .child(
                    deferred(
                        anchored().position(point(px(0.), px(0.))).child(
                            div()
                                .id(self.id)
                                .occlude()
                                .w(viewport.width)
                                .h(viewport.height)
                                .on_mouse_down(MouseButton::Left, {
                                    let dismiss = dismiss.clone();
                                    move |event, window, cx| dismiss(event, window, cx)
                                })
                                .on_mouse_down(MouseButton::Right, move |event, window, cx| {
                                    dismiss(event, window, cx)
                                }),
                        ),
                    )
                    .with_priority(1),
                )
            })
    }
}
