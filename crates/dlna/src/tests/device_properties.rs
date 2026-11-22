use proptest::prelude::*;
use proptest::sample::select;

use super::*;
use crate::http::is_clean;
use crate::http::properties::url_text;
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
