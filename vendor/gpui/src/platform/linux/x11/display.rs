use anyhow::Context as _;
use uuid::Uuid;
use x11rb::{
    connection::Connection as _,
    protocol::{
        randr::ConnectionExt as _,
        xproto::{self, ConnectionExt as _},
    },
    xcb_ffi::XCBConnection,
};

use crate::{Bounds, DisplayId, Pixels, PlatformDisplay, Size, point, px, size};

/// Plays: a display is a RandR monitor, as an X screen spans every monitor
/// of the desktop; the screen as a whole only where RandR doesn't say. A
/// monitor's id is its name's atom, which stays as long as the X server
/// does, and its bounds are where it is on the screen, as windows' are.
#[derive(Clone, Debug)]
pub(crate) struct X11Display {
    id: DisplayId,
    name: Option<String>,
    bounds: Bounds<Pixels>,
    uuid: Uuid,
    primary: bool,
}

impl X11Display {
    /// X screen `x_screen_index` as a whole.
    pub(crate) fn new(
        xcb: &XCBConnection,
        scale_factor: f32,
        x_screen_index: usize,
    ) -> anyhow::Result<Self> {
        let screen = xcb
            .setup()
            .roots
            .get(x_screen_index)
            .with_context(|| format!("No screen found with index {x_screen_index}"))?;
        Ok(Self {
            id: DisplayId(x_screen_index as u32),
            name: None,
            bounds: Bounds {
                origin: Default::default(),
                size: Size {
                    width: px(screen.width_in_pixels as f32 / scale_factor),
                    height: px(screen.height_in_pixels as f32 / scale_factor),
                },
            },
            uuid: Uuid::from_bytes([0; 16]),
            primary: true,
        })
    }

    /// Plays: the displays of X screen `x_screen_index`: its monitors, or
    /// itself where RandR doesn't say them.
    pub(crate) fn all(xcb: &XCBConnection, scale_factor: f32, x_screen_index: usize) -> Vec<Self> {
        let monitors = Self::monitors(xcb, scale_factor, x_screen_index).unwrap_or_default();
        if !monitors.is_empty() {
            return monitors;
        }
        Self::new(xcb, scale_factor, x_screen_index)
            .ok()
            .into_iter()
            .collect()
    }

    /// The monitors RandR has on the screen, by their names: the product
    /// name in the EDID of a monitor's first output, or else the monitor's
    /// own (its connector's, such as DP-1). Two of the same name are told
    /// apart by their connectors.
    fn monitors(
        xcb: &XCBConnection,
        scale_factor: f32,
        x_screen_index: usize,
    ) -> anyhow::Result<Vec<Self>> {
        let root = xcb
            .setup()
            .roots
            .get(x_screen_index)
            .context("no such screen")?
            .root;
        let monitors = xcb.randr_get_monitors(root, true)?.reply()?.monitors;
        let edid = xcb.intern_atom(true, b"EDID")?.reply()?.atom;
        // Asked all at once, then waited for.
        let asked = monitors
            .iter()
            .map(|monitor| {
                let connector = xcb.get_atom_name(monitor.name).ok();
                let product = monitor
                    .outputs
                    .first()
                    .filter(|_| edid != u32::from(xproto::AtomEnum::NONE))
                    .and_then(|&output| {
                        xcb.randr_get_output_property(
                            output,
                            edid,
                            xproto::AtomEnum::ANY,
                            0,
                            // In 32-bit units: 256 bytes, a base block and
                            // one extension.
                            64,
                            false,
                            false,
                        )
                        .ok()
                    });
                (monitor, connector, product)
            })
            .collect::<Vec<_>>();
        let mut displays = asked
            .into_iter()
            .map(|(monitor, connector, product)| {
                let connector = connector
                    .and_then(|cookie| cookie.reply().ok())
                    .map(|reply| String::from_utf8_lossy(&reply.name).into_owned());
                let product = product
                    .and_then(|cookie| cookie.reply().ok())
                    .and_then(|reply| edid_name(&reply.data));
                let scaled = |pixels: f32| px(pixels / scale_factor);
                (
                    Self {
                        id: DisplayId(monitor.name),
                        name: product.or_else(|| connector.clone()),
                        bounds: Bounds::new(
                            point(scaled(monitor.x.into()), scaled(monitor.y.into())),
                            size(scaled(monitor.width.into()), scaled(monitor.height.into())),
                        ),
                        uuid: Uuid::new_v5(
                            &Uuid::NAMESPACE_DNS,
                            connector.as_deref().unwrap_or_default().as_bytes(),
                        ),
                        primary: monitor.primary,
                    },
                    connector,
                )
            })
            .collect::<Vec<_>>();
        let names = displays
            .iter()
            .map(|(display, _)| display.name.clone())
            .collect::<Vec<_>>();
        for (display, connector) in &mut displays {
            let twins = names.iter().filter(|name| **name == display.name).count();
            if twins > 1
                && let (Some(name), Some(connector)) = (&display.name, connector)
                && name != connector
            {
                display.name = Some(format!("{name} ({connector})"));
            }
        }
        Ok(displays.into_iter().map(|(display, _)| display).collect())
    }

    /// Plays: the primary display of the screen: its primary monitor, or
    /// else its first.
    pub(crate) fn primary(displays: Vec<Self>) -> Option<Self> {
        let index = displays
            .iter()
            .position(|display| display.primary)
            .unwrap_or(0);
        displays.into_iter().nth(index)
    }

    /// Plays: the display of a window at `bounds`: the one with its middle,
    /// or else the one it covers most of, or else the first.
    pub(crate) fn of(displays: Vec<Self>, bounds: Bounds<Pixels>) -> Option<Self> {
        let center = bounds.center();
        let area = |display: &Self| {
            let common = display.bounds.intersect(&bounds);
            f32::from(common.size.width) * f32::from(common.size.height)
        };
        let index = displays
            .iter()
            .position(|display| {
                let room = &display.bounds;
                (room.left()..room.right()).contains(&center.x)
                    && (room.top()..room.bottom()).contains(&center.y)
            })
            .or_else(|| {
                displays
                    .iter()
                    .enumerate()
                    .filter(|(_, display)| area(display) > 0.)
                    .max_by(|(_, a), (_, b)| area(a).total_cmp(&area(b)))
                    .map(|(index, _)| index)
            })
            .unwrap_or(0);
        displays.into_iter().nth(index)
    }
}

/// The product name an EDID gives its display: the text of its "display
/// product name" descriptor, if it has one.
fn edid_name(edid: &[u8]) -> Option<String> {
    edid.get(54..126)?
        .chunks_exact(18)
        .find(|block| block[..3] == [0, 0, 0] && block[3] == 0xFC)
        .map(|block| {
            let text = &block[5..];
            let end = text.iter().position(|&b| b == b'\n').unwrap_or(text.len());
            String::from_utf8_lossy(&text[..end]).trim().to_owned()
        })
        .filter(|name| !name.is_empty())
}

impl PlatformDisplay for X11Display {
    fn id(&self) -> DisplayId {
        self.id
    }

    fn uuid(&self) -> anyhow::Result<Uuid> {
        Ok(self.uuid)
    }

    fn bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }

    fn name(&self) -> Option<String> {
        self.name.clone()
    }
}
