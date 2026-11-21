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
mod tests {
    use proptest::prelude::*;
    use proptest::sample::select;

    use super::*;
    use crate::http::is_clean;
    use crate::http::tests::url_text;
    use crate::xml::escape;

    /// A URL a description might give: anything, or one that tries to
    /// reach elsewhere or break a request.
    fn reference() -> impl Strategy<Value = String> {
        prop_oneof![
            url_text(),
            select(vec![
                "/upnp/control/AVTransport1",
                "http://192.0.2.10:7676/x",
                "HTTP://192.0.2.10/x",
                "http://127.0.0.1:8080/x",
                "//198.51.100.1/x",
                "/x\r\nX-Evil: 1",
                " /padded ",
                "/a b",
                "/é",
                "",
            ])
            .prop_map(str::to_owned),
        ]
    }

    /// `text` as it goes into an element, its line breaks as references.
    fn escaped(text: &str) -> String {
        escape(text).replace('\r', "&#13;").replace('\n', "&#10;")
    }

    proptest! {
        #[test]
        fn a_description_reaches_only_its_own_device_however_it_is_written(
            control in reference(),
            events in reference(),
            icon in reference(),
            base in proptest::option::of(reference()),
            name in any::<String>(),
        ) {
            let mut description = include_str!("../fixtures/samsung.xml")
                .replace(
                    "<controlURL>/upnp/control/AVTransport1</controlURL>",
                    &format!("<controlURL>{}</controlURL>", escaped(&control)),
                )
                .replace(
                    "<eventSubURL>/upnp/event/AVTransport1</eventSubURL>",
                    &format!("<eventSubURL>{}</eventSubURL>", escaped(&events)),
                )
                .replace(
                    "<url>/dmr/icon_SML.jpg</url>",
                    &format!("<url>{}</url>", escaped(&icon)),
                )
                .replace(
                    "<friendlyName>[TV] Samsung Q70 Series (55)</friendlyName>",
                    &format!("<friendlyName>{}</friendlyName>", escaped(&name)),
                );
            if let Some(base) = base {
                description = description
                    .replacen("<device>", &format!("<URLBase>{}</URLBase><device>", escaped(&base)), 1);
            }
            let Ok(tv) = Device::from_description(&description, "http://192.0.2.10:9197/dmr") else {
                return Ok(());
            };
            let services = [Some(&tv.av_transport), tv.rendering_control.as_ref(), tv.connection_manager.as_ref()];
            let urls = services
                .into_iter()
                .flatten()
                .flat_map(|service| [Some(&service.control), service.events.as_ref()])
                .flatten()
                .chain(tv.icons.iter().map(|icon| &icon.url));
            for url in urls {
                let parsed = Url::parse(url);
                prop_assert!(parsed.is_some(), "{url:?}");
                prop_assert_eq!(parsed.unwrap().host, "192.0.2.10", "{}", url);
                prop_assert!(is_clean(url), "{url:?}");
            }
        }
    }

    #[test]
    fn a_samsung_tv() {
        let tv = Device::from_description(
            include_str!("../fixtures/samsung.xml"),
            "http://192.0.2.10:9197/dmr",
        )
        .unwrap();
        assert_eq!(tv.id, "uuid:00000000-0000-1000-8000-0000000000a1");
        assert_eq!(tv.name, "[TV] Samsung Q70 Series (55)");
        assert_eq!(tv.manufacturer, "Samsung Electronics");
        assert_eq!(tv.model, "QE55Q70RAT");
        assert_eq!(
            tv.av_transport,
            Service {
                kind: "urn:schemas-upnp-org:service:AVTransport:1".into(),
                control: "http://192.0.2.10:9197/upnp/control/AVTransport1".into(),
                events: Some("http://192.0.2.10:9197/upnp/event/AVTransport1".into()),
            }
        );
        assert_eq!(
            tv.rendering_control.unwrap().control,
            "http://192.0.2.10:9197/upnp/control/RenderingControl1"
        );
        assert!(tv.connection_manager.is_some());
        assert_eq!(tv.icons.len(), 4);
        let icon = tv
            .icons
            .iter()
            .find(|icon| icon.mime == "image/png" && icon.width == 120);
        assert_eq!(icon.unwrap().url, "http://192.0.2.10:9197/dmr/icon_LRG.png");
    }

    #[test]
    fn a_description_is_kept_to_the_device_it_came_from() {
        let samsung = include_str!("../fixtures/samsung.xml");
        let location = "http://192.0.2.10:9197/dmr";
        let control = "<controlURL>/upnp/control/AVTransport1</controlURL>";
        for elsewhere in [
            "<controlURL>http://127.0.0.1:8080/x</controlURL>",
            "<controlURL>//198.51.100.1/x</controlURL>",
            "<controlURL>/x&#13;&#10;X-Evil: 1</controlURL>",
        ] {
            let tv = Device::from_description(&samsung.replace(control, elsewhere), location);
            assert!(tv.is_err(), "{elsewhere} taken: {tv:?}");
        }
        let based = samsung.replace(
            "<device>",
            "<URLBase>http://127.0.0.1:8080/</URLBase><device>",
        );
        assert!(Device::from_description(&based, location).is_err());
        let odd = samsung.replace(
            "urn:schemas-upnp-org:service:AVTransport:1<",
            "urn:schemas-upnp-org:service:AVTransport:1&quot; x=&quot;<",
        );
        assert!(Device::from_description(&odd, location).is_err());
        // Another port on the same host is the device's own.
        let port = samsung.replace(control, "<controlURL>http://192.0.2.10:7676/x</controlURL>");
        let tv = Device::from_description(&port, location).unwrap();
        assert_eq!(tv.av_transport.control, "http://192.0.2.10:7676/x");
        // An icon elsewhere is left out.
        let icon = samsung.replace(
            "<url>/dmr/icon_SML.jpg</url>",
            "<url>http://198.51.100.1/icon.jpg</url>",
        );
        let tv = Device::from_description(&icon, location).unwrap();
        assert_eq!(tv.icons.len(), 3);
    }

    #[test]
    fn an_lg_tv_with_paths_beside_its_description() {
        let tv = Device::from_description(
            include_str!("../fixtures/lg.xml"),
            "http://192.0.2.11:1754/",
        )
        .unwrap();
        assert_eq!(tv.name, "[LG] webOS TV OLED55C1");
        assert_eq!(
            tv.av_transport.control,
            "http://192.0.2.11:1754/AVTransport/00000000-0000-1000-8000-0000000000b2/control.xml"
        );
        assert_eq!(
            tv.connection_manager
                .as_ref()
                .unwrap()
                .events
                .as_deref()
                .unwrap(),
            "http://192.0.2.11:1754/ConnectionManager/00000000-0000-1000-8000-0000000000b2/event.xml"
        );
        assert_eq!(tv.icon().unwrap().url, "http://192.0.2.11:1754/LG_TV.png");
    }

    #[test]
    fn a_sonos_speaker_renders_inside_its_player() {
        let speaker = Device::from_description(
            include_str!("../fixtures/sonos.xml"),
            "http://192.0.2.30:1400/xml/device_description.xml",
        )
        .unwrap();
        assert_eq!(speaker.id, "uuid:RINCON_000000000000001400_MR");
        assert!(speaker.name.starts_with("Kitchen"));
        assert_eq!(
            speaker.av_transport.control,
            "http://192.0.2.30:1400/MediaRenderer/AVTransport/Control"
        );
        assert_eq!(
            speaker.connection_manager.as_ref().unwrap().control,
            "http://192.0.2.30:1400/MediaRenderer/ConnectionManager/Control",
            "the renderer's own, not the media server's"
        );
        assert_eq!(
            speaker.icon().unwrap().url,
            "http://192.0.2.30:1400/img/icon-S18.png",
            "the player's icons"
        );
    }

    #[test]
    fn a_devialet_speaker_with_services_of_version_two() {
        let speaker = Device::from_description(
            include_str!("../fixtures/devialet.xml"),
            "http://192.0.2.40:42673/00000000-0000-1000-8000-0000000000c3.xml",
        )
        .unwrap();
        assert_eq!(speaker.id, "uuid:00000000-0000-1000-8000-0000000000c3");
        assert_eq!(speaker.name, "Phantom Right");
        assert_eq!(speaker.manufacturer, "Devialet");
        assert_eq!(speaker.model, "Phantom 108 dB");
        assert_eq!(
            speaker.av_transport,
            Service {
                kind: "urn:schemas-upnp-org:service:AVTransport:2".into(),
                control: "http://192.0.2.40:42673/Control/DvltRygelRendererPlugin/RygelAVTransport"
                    .into(),
                events: Some(
                    "http://192.0.2.40:42673/Event/DvltRygelRendererPlugin/RygelAVTransport".into()
                ),
            }
        );
        assert_eq!(
            speaker.rendering_control.as_ref().unwrap().kind,
            "urn:schemas-upnp-org:service:RenderingControl:2"
        );
        assert!(speaker.connection_manager.is_some());
        assert_eq!(speaker.icons.len(), 4);
    }

    #[test]
    fn url_base_wins_over_the_location() {
        let description = "<root><URLBase>http://192.0.2.12:52323/</URLBase><device>\
             <deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>\
             <friendlyName>Old TV</friendlyName><UDN>uuid:old</UDN><serviceList><service>\
             <serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType>\
             <controlURL>upnp/control/AVTransport</controlURL><eventSubURL></eventSubURL>\
             </service></serviceList></device></root>";
        let tv = Device::from_description(description, "http://192.0.2.12:8080/dd.xml").unwrap();
        assert_eq!(
            tv.av_transport.control,
            "http://192.0.2.12:52323/upnp/control/AVTransport"
        );
        assert_eq!(tv.av_transport.events, None, "an empty eventSubURL is none");
        assert_eq!(tv.rendering_control, None);
        assert_eq!(tv.address(), Some([192, 0, 2, 12].into()));
    }

    #[test]
    fn an_address_is_where_its_transport_is_and_never_looked_up() {
        let at = |location: &str, control: &str| {
            let description = format!(
                "<root><device>\
                 <deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>\
                 <friendlyName>TV</friendlyName><UDN>uuid:tv</UDN><serviceList><service>\
                 <serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType>\
                 <controlURL>{control}</controlURL></service></serviceList></device></root>"
            );
            Device::from_description(&description, location)
                .unwrap()
                .address()
        };
        let location = "http://192.0.2.13/dd.xml";
        assert_eq!(at(location, "/control"), Some([192, 0, 2, 13].into()));
        assert_eq!(
            at(
                "http://[2001:db8::1]/dd.xml",
                "http://[2001:db8::1]:8080/control"
            ),
            Some("2001:db8::1".parse().unwrap())
        );
        assert_eq!(
            at("http://tv.local/dd.xml", "http://tv.local:8080/control"),
            None
        );
    }

    #[test]
    fn what_isnt_a_renderer() {
        let server = "<root><device>\
             <deviceType>urn:schemas-upnp-org:device:MediaServer:1</deviceType><serviceList>\
             <service><serviceType>urn:schemas-upnp-org:service:ContentDirectory:1</serviceType>\
             <controlURL>/cd</controlURL></service></serviceList></device></root>";
        assert!(Device::from_description(server, "http://192.0.2.1/").is_err());
        assert!(Device::from_description("<root/>", "http://192.0.2.1/").is_err());
        assert!(Device::from_description("not xml", "http://192.0.2.1/").is_err());
    }
}
