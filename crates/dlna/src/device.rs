//! A renderer, as its device description says: its name and model, its
//! icons, and where its services are.

use std::net::IpAddr;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::http::{self, Url};
use crate::media::Capability;
use crate::soap;
use crate::xml::Element;

/// How long a device has to give its description.
const DESCRIBE: Duration = Duration::from_secs(5);

/// A media renderer.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Device {
    /// Its UDN, `uuid:…`, which stays the same.
    pub id: String,
    pub name: String,
    pub manufacturer: String,
    pub model: String,
    /// Where its description is.
    pub location: String,
    pub icons: Vec<Icon>,
    pub av_transport: Service,
    pub rendering_control: Option<Service>,
    pub connection_manager: Option<Service>,
}

/// One of its services.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Service {
    /// Its type: `urn:schemas-upnp-org:service:AVTransport:1` and so on.
    pub kind: String,
    /// Where its actions go.
    pub control: String,
    /// Where to subscribe to its events, when it has them.
    pub events: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Icon {
    pub mime: String,
    pub width: u32,
    pub height: u32,
    pub url: String,
}

/// `device` and those inside it, depth first.
fn devices<'a>(device: &'a Element, all: &mut Vec<&'a Element>) {
    all.push(device);
    for inner in device
        .child("deviceList")
        .into_iter()
        .flat_map(|list| list.children("device"))
    {
        devices(inner, all);
    }
}

/// The service of `device` whose type is `name`, of any version.
fn service<'a>(device: &'a Element, name: &str) -> Option<&'a Element> {
    let kind = format!(":service:{name}:");
    device
        .child("serviceList")?
        .children("service")
        .find(|service| {
            service
                .child_text("serviceType")
                .is_some_and(|it| it.contains(&kind))
        })
}

/// Whether `kind` reads as a UPnP type, `urn:schemas-upnp-org:service:…`,
/// and so goes into a request as it is.
fn is_type(kind: &str) -> bool {
    !kind.is_empty()
        && kind
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ":.-_".contains(c))
}

fn text(element: &Element, name: &str) -> String {
    element.child_text(name).unwrap_or_default().to_owned()
}

impl Device {
    /// Fetch and read the description at `location`.
    pub fn describe(location: &str) -> Result<Self> {
        let url = Url::parse(location)
            .ok_or_else(|| Error::protocol(format!("a location {location:?}")))?;
        let response = http::request("GET", &url, &[], &[], DESCRIBE)?;
        let status = response.head.status().unwrap_or_default();
        if status != 200 {
            return Err(Error::Http {
                request: "the description".into(),
                status,
            });
        }
        Self::from_description(&response.text(), location)
    }

    /// Read a description fetched from `location`. The renderer may be a
    /// device inside another, as a Sonos speaker's is.
    pub fn from_description(description: &str, location: &str) -> Result<Self> {
        let root = Element::parse(description).map_err(Error::protocol)?;
        let origin = Url::parse(location)
            .ok_or_else(|| Error::protocol(format!("a location {location:?}")))?;
        let base = root
            .child_text("URLBase")
            .and_then(Url::parse)
            .unwrap_or_else(|| origin.clone());
        let mut all = Vec::new();
        devices(
            root.child("device")
                .ok_or_else(|| Error::protocol("a description without a device"))?,
            &mut all,
        );
        let renderer = all
            .iter()
            .find(|device| {
                device
                    .child_text("deviceType")
                    .is_some_and(|kind| kind.contains(":device:MediaRenderer:"))
            })
            .or_else(|| {
                all.iter()
                    .find(|device| service(device, "AVTransport").is_some())
            })
            .ok_or_else(|| Error::protocol("not a media renderer"))?;
        // Only the device the description came from is asked anything: a
        // URL to elsewhere, on this computer say, is no URL of its own.
        let url = |reference: &str| {
            base.join(reference)
                .filter(|url| url.same_host(&origin))
                .map(|url| url.to_string())
        };
        // The renderer's own, or failing that, any device's.
        let find = |name: &str| {
            let found = service(renderer, name)
                .or_else(|| all.iter().find_map(|device| service(device, name)))?;
            Some(Service {
                kind: Some(text(found, "serviceType")).filter(|kind| is_type(kind))?,
                control: url(found.child_text("controlURL")?)?,
                events: found
                    .child_text("eventSubURL")
                    .filter(|events| !events.is_empty())
                    .and_then(url),
            })
        };
        let icons = renderer
            .child("iconList")
            .or_else(|| all[0].child("iconList"))
            .into_iter()
            .flat_map(|list| list.children("icon"))
            .filter_map(|icon| {
                Some(Icon {
                    mime: text(icon, "mimetype"),
                    width: icon.child_text("width")?.parse().ok()?,
                    height: icon.child_text("height")?.parse().ok()?,
                    url: url(icon.child_text("url")?)?,
                })
            })
            .collect();
        Ok(Self {
            id: text(renderer, "UDN"),
            name: text(renderer, "friendlyName"),
            manufacturer: text(renderer, "manufacturer"),
            model: text(renderer, "modelName"),
            location: location.to_owned(),
            icons,
            av_transport: find("AVTransport")
                .ok_or_else(|| Error::protocol("a renderer without AVTransport"))?,
            rendering_control: find("RenderingControl"),
            connection_manager: find("ConnectionManager"),
        })
    }

    /// What it plays: the sink side of ConnectionManager's protocol info.
    pub fn capabilities(&self) -> Result<Vec<Capability>> {
        let service = self
            .connection_manager
            .as_ref()
            .ok_or(Error::Unsupported("ConnectionManager"))?;
        let answer = soap::call(service, "GetProtocolInfo", &[], soap::ANSWER)?;
        Ok(Capability::parse_list(
            answer.get("Sink").unwrap_or_default(),
        ))
    }

    /// Where it is: the address its AVTransport is reached at, where the
    /// description gives one rather than a host name, which this doesn't
    /// look up.
    pub fn address(&self) -> Option<IpAddr> {
        Url::parse(&self.av_transport.control)?.host.parse().ok()
    }

    /// Its biggest icon.
    pub fn icon(&self) -> Option<&Icon> {
        self.icons
            .iter()
            .max_by_key(|icon| icon.width * icon.height)
    }
}

#[cfg(test)]
#[path = "tests/device_properties.rs"]
mod properties;
#[cfg(test)]
#[path = "tests/device.rs"]
mod tests;
