use gpui::prelude::*;
use gpui::{
    App, ClickEvent, ElementId, Hsla, Refineable, SharedString, StyleRefinement, Window, div,
    transparent_black, white,
};

use super::{ClickHandler, ComponentSize, ControlFrame, Icon, Sizable};
use crate::ui::theme::controls::{
    BUTTON_ICON, DISABLED, FILLED_ACTIVE, FILLED_HOVER, FILLED_SHORTCUT, HEIGHT, TINT, TINT_ACTIVE,
    TINT_HOVER, WIDE_PADDING,
};
use crate::ui::theme::{Theme, overlay};

/// How a button looks.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum ButtonVariant {
    /// The surface color with a hairline border.
    #[default]
    Normal,
    /// Filled with the accent color, for the main action of a dialog.
    Primary,
    /// Tinted with the accent color, for an action that matters less than
    /// a primary one but more than the rest.
    Secondary,
    /// Filled with the danger color, for an action that destroys something.
    Danger,
    /// Just its label and icon until it is hovered.
    Ghost,
    /// Text in the accent color, underlined while hovered.
    Link,
    /// A square with just an icon, which shows only the icon until hovered.
    Icon,
}

/// Which side of its label a button's icon is on.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum IconSide {
    #[default]
    Left,
    /// At the far end, such as the chevron of a button that opens a menu.
    Right,
}

/// A button: a label with an icon on either side if it has one, and
/// optionally its keyboard shortcut after the label; or just an icon
/// ([`Button::icon`]). It can show as selected (pressed in, or on) and as
/// disabled. Size and place it like a `div`; styles set on it win over its
/// own. A label too long for a width set on it is cut short.
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: Option<SharedString>,
    icon: Option<SharedString>,
    icon_side: IconSide,
    icon_color: Option<Hsla>,
    shortcut: Option<SharedString>,
    variant: ButtonVariant,
    size: ComponentSize,
    wide: bool,
    overlay: bool,
    selected: bool,
    disabled: bool,
    on_click: Option<ClickHandler>,
    style: StyleRefinement,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self::build(id.into(), Some(label.into()), None, ButtonVariant::Normal)
    }

    /// A square button showing just `icon`.
    pub fn icon(id: impl Into<ElementId>, icon: impl Into<SharedString>) -> Self {
        Self::build(id.into(), None, Some(icon.into()), ButtonVariant::Icon)
    }

    fn build(
        id: ElementId,
        label: Option<SharedString>,
        icon: Option<SharedString>,
        variant: ButtonVariant,
    ) -> Self {
        Self {
            id,
            label,
            icon,
            icon_side: IconSide::Left,
            icon_color: None,
            shortcut: None,
            variant,
            size: ComponentSize::Medium,
            wide: false,
            overlay: false,
            selected: false,
            disabled: false,
            on_click: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn primary(self) -> Self {
        self.variant(ButtonVariant::Primary)
    }

    pub fn secondary(self) -> Self {
        self.variant(ButtonVariant::Secondary)
    }

    pub fn danger(self) -> Self {
        self.variant(ButtonVariant::Danger)
    }

    pub fn ghost(self) -> Self {
        self.variant(ButtonVariant::Ghost)
    }

    pub fn link(self) -> Self {
        self.variant(ButtonVariant::Link)
    }

    /// An icon before the label.
    pub fn icon_left(mut self, name: impl Into<SharedString>) -> Self {
        self.icon = Some(name.into());
        self.icon_side = IconSide::Left;
        self
    }

    /// An icon at the far end, after the label.
    pub fn icon_right(mut self, name: impl Into<SharedString>) -> Self {
        self.icon = Some(name.into());
        self.icon_side = IconSide::Right;
        self
    }

    /// The icon's color, in place of the variant's.
    pub fn icon_color(mut self, color: Hsla) -> Self {
        self.icon_color = Some(color);
        self
    }

    /// Shown after the label, dimmer, such as "⌘O".
    pub fn shortcut(mut self, keys: impl Into<SharedString>) -> Self {
        self.shortcut = Some(keys.into());
        self
    }

    /// More room on the sides, for a button standing on its own.
    pub fn wide(mut self) -> Self {
        self.wide = true;
        self
    }

    /// Colors for drawing over video, which is dark in both themes, for
    /// ghost and icon buttons.
    pub fn overlay(mut self) -> Self {
        self.overlay = true;
        self
    }

    /// Shown pressed in; an icon button shows its icon in the accent color
    /// instead. For what is on, or the one picked.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Dimmed, and clicks do nothing.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl Sizable for Button {
    fn with_size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }
}

impl Styled for Button {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

/// A button's colors: at rest, hovered, pressed and selected.
struct Look {
    background: Hsla,
    border: Hsla,
    text: Hsla,
    icon: Hsla,
    shortcut: Hsla,
    hover: Hsla,
    active: Hsla,
    selected: Hsla,
}

impl Look {
    fn new(variant: ButtonVariant, theme: &Theme, over_video: bool) -> Self {
        let clear = transparent_black();
        let (text, muted, hover, active) = if over_video {
            (
                overlay::text(),
                overlay::text_muted(),
                overlay::hover(),
                overlay::hover(),
            )
        } else {
            (
                theme.text,
                theme.text_muted,
                theme.surface_hover,
                theme.surface_active,
            )
        };
        let filled = |color: Hsla| Self {
            background: color,
            border: color,
            text: white(),
            icon: white(),
            shortcut: white().opacity(FILLED_SHORTCUT),
            hover: color.opacity(FILLED_HOVER),
            active: color.opacity(FILLED_ACTIVE),
            selected: color.opacity(FILLED_ACTIVE),
        };
        let bare = |text: Hsla, icon: Hsla| Self {
            background: clear,
            border: clear,
            text,
            icon,
            shortcut: muted,
            hover,
            active,
            selected: active,
        };
        match variant {
            ButtonVariant::Normal => Self {
                background: theme.surface,
                border: theme.border,
                text: theme.text,
                icon: theme.text_muted,
                shortcut: theme.text_muted,
                hover: theme.surface_hover,
                active: theme.surface_active,
                selected: theme.surface_active,
            },
            ButtonVariant::Primary => filled(theme.accent),
            ButtonVariant::Danger => filled(theme.danger),
            ButtonVariant::Secondary => {
                let accent = theme.accent;
                Self {
                    background: accent.opacity(TINT),
                    border: clear,
                    text: accent,
                    icon: accent,
                    shortcut: accent.opacity(FILLED_SHORTCUT),
                    hover: accent.opacity(TINT_HOVER),
                    active: accent.opacity(TINT_ACTIVE),
                    selected: accent.opacity(TINT_ACTIVE),
                }
            }
            ButtonVariant::Ghost => bare(text, muted),
            // Selected shows in the icon's color, not behind it.
            ButtonVariant::Icon => Self {
                selected: clear,
                ..bare(text, text)
            },
            ButtonVariant::Link => Self {
                hover: clear,
                active: clear,
                selected: clear,
                ..bare(theme.accent, theme.accent)
            },
        }
    }
}

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self {
            id,
            label,
            icon,
            icon_side,
            icon_color,
            shortcut,
            variant,
            size,
            wide,
            overlay,
            selected,
            disabled,
            on_click,
            style,
        } = self;
        let theme = Theme::get(window, cx);
        let look = Look::new(variant, &theme, overlay);
        let icon_only = variant == ButtonVariant::Icon;
        let icon_color = if icon_only && selected {
            theme.accent
        } else {
            icon_color.unwrap_or(look.icon)
        };
        let icon = icon.map(|name| {
            let icon = Icon::new(name).color(icon_color);
            if icon_only {
                icon.with_size(size)
            } else {
                icon.size(size.pick(BUTTON_ICON))
            }
        });
        let label = label.map(|label| div().min_w_0().truncate().child(label));
        let shortcut = shortcut.map(|keys| div().ml_0p5().text_color(look.shortcut).child(keys));
        let (before, after) = match icon_side {
            IconSide::Left => (icon, None),
            IconSide::Right => (None, icon),
        };
        let (hover, active) = (look.hover, look.active);
        // Tests find a button by its id, and tell it's selected by an empty
        // mark in it tagged "{id}-selected".
        let selector = id.clone();
        let mark = (cfg!(test) && selected).then(|| {
            let selector = format!("{selector}-selected");
            div().absolute().debug_selector(move || selector)
        });

        let mut button = div()
            .id(id)
            .debug_selector(move || selector.to_string())
            .control_frame(size)
            .flex_none()
            .gap_1p5()
            .border_color(look.border)
            .bg(if selected {
                look.selected
            } else {
                look.background
            })
            .text_color(look.text)
            .map(|this| match variant {
                ButtonVariant::Icon => this.justify_center().w(size.pick(HEIGHT)).px_0(),
                ButtonVariant::Link => this.px_0(),
                _ if wide => this.px(size.pick(WIDE_PADDING)),
                _ => this,
            })
            .when(icon_side == IconSide::Right, |this| this.justify_between())
            .children(before)
            .children(label)
            .children(shortcut)
            .children(after)
            .children(mark)
            .map(|this| {
                if disabled {
                    return this.opacity(DISABLED);
                }
                this.cursor_pointer()
                    .map(|this| match variant {
                        ButtonVariant::Link => this.hover(|style| style.underline()),
                        // A selected button stays pressed in while hovered.
                        _ if selected => this,
                        _ => this.hover(move |style| style.bg(hover)),
                    })
                    .active(move |style| style.bg(active))
                    .when_some(on_click, |this, handler| this.on_click(handler))
            });
        button.style().refine(&style);
        button
    }
}
