use super::*;

#[test]
fn a_samsung_tv() {
    let tv = Device::from_description(
        include_str!("../../fixtures/samsung.xml"),
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
    let samsung = include_str!("../../fixtures/samsung.xml");
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
        include_str!("../../fixtures/lg.xml"),
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
        include_str!("../../fixtures/sonos.xml"),
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
        include_str!("../../fixtures/devialet.xml"),
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
