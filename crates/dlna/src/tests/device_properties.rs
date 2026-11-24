use hegel::TestCase;
use hegel::generators::{self as gs, Generator};

use super::*;
use crate::http::is_clean;
use crate::http::properties::url_text;
use crate::xml::escape;

/// A URL a description might give: anything, or one that tries to
/// reach elsewhere or break a request.
#[hegel::composite]
fn reference(tc: &TestCase) -> String {
    tc.draw(hegel::one_of!(
        url_text(),
        gs::sampled_from(vec![
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
        .map(str::to_owned),
    ))
}

/// `text` as it goes into an element, its line breaks as references.
fn escaped(text: &str) -> String {
    escape(text).replace('\r', "&#13;").replace('\n', "&#10;")
}

#[hegel::test]
fn a_description_reaches_only_its_own_device_however_it_is_written(tc: TestCase) {
    let control = tc.draw(reference());
    let events = tc.draw(reference());
    let icon = tc.draw(reference());
    let base = tc.draw(gs::optional(reference()));
    let name = tc.draw(gs::text());
    let mut description = include_str!("../../fixtures/samsung.xml")
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
        description = description.replacen(
            "<device>",
            &format!("<URLBase>{}</URLBase><device>", escaped(&base)),
            1,
        );
    }
    let Ok(tv) = Device::from_description(&description, "http://192.0.2.10:9197/dmr") else {
        return;
    };
    let services = [
        Some(&tv.av_transport),
        tv.rendering_control.as_ref(),
        tv.connection_manager.as_ref(),
    ];
    let urls = services
        .into_iter()
        .flatten()
        .flat_map(|service| [Some(&service.control), service.events.as_ref()])
        .flatten()
        .chain(tv.icons.iter().map(|icon| &icon.url));
    for url in urls {
        let parsed = Url::parse(url);
        assert!(parsed.is_some(), "{url:?}");
        assert_eq!(parsed.unwrap().host, "192.0.2.10", "{}", url);
        assert!(is_clean(url), "{url:?}");
    }
}
