use super::*;

#[test]
fn names_lose_their_prefixes_and_text_its_escapes() {
    let root = Element::parse(
        "\u{feff}<?xml version=\"1.0\"?>\n<s:Envelope xmlns:s=\"urn:s\" s:style=\"x &amp; y\">\
             <s:Body><u:Answer xmlns:u=\"urn:u\"><Name>Tom &amp; Jerry &#233;&#x41;</Name>\
             <Empty/><Data><![CDATA[<b>bold</b>]]></Data></u:Answer></s:Body></s:Envelope>",
    )
    .unwrap();
    assert_eq!(root.name, "Envelope");
    assert_eq!(root.attributes, [("style".into(), "x & y".into())]);
    let answer = root.child("Body").unwrap().child("Answer").unwrap();
    assert_eq!(answer.child_text("Name"), Some("Tom & Jerry éA"));
    assert_eq!(answer.child_text("Empty"), Some(""));
    assert_eq!(answer.child_text("Data"), Some("<b>bold</b>"));
    assert_eq!(root.find("Data").unwrap().text, "<b>bold</b>");
    assert!(root.find("Nothing").is_none());
}

#[test]
fn xml_inside_xml_comes_out_to_be_read_again() {
    let outer = Element::parse(
        "<e:propertyset xmlns:e=\"urn:schemas-upnp-org:event-1-0\"><e:property>\
             <LastChange>&lt;Event&gt;&lt;InstanceID val=&quot;0&quot;&gt;\
             &lt;TransportState val=&quot;PLAYING&quot;/&gt;&lt;/InstanceID&gt;&lt;/Event&gt;\
             </LastChange></e:property></e:propertyset>",
    )
    .unwrap();
    let inner = Element::parse(outer.find("LastChange").unwrap().text()).unwrap();
    assert_eq!(
        inner
            .child("InstanceID")
            .and_then(|instance| instance.child("TransportState"))
            .and_then(|state| state.attribute("val")),
        Some("PLAYING")
    );
}

#[test]
fn broken_documents_are_errors() {
    assert!(Element::parse("").is_err());
    assert!(Element::parse("<a><b></a>").is_err());
    assert!(Element::parse("<a>").is_err());
    assert!(Element::parse("<a>&nbsp;</a>").is_err());
}

#[test]
fn documents_nested_too_deep_are_errors() {
    let nested = |depth: usize| format!("{}{}", "<a>".repeat(depth), "</a>".repeat(depth));
    assert!(
        Element::parse(&nested(MAX_DEPTH)).is_ok(),
        "as deep as it may be"
    );
    assert!(
        Element::parse(&nested(MAX_DEPTH + 1)).is_err(),
        "a level deeper"
    );
    let empty = format!(
        "{}<b/>{}",
        "<a>".repeat(MAX_DEPTH),
        "</a>".repeat(MAX_DEPTH)
    );
    assert!(
        Element::parse(&empty).is_err(),
        "an empty element a level deeper"
    );
    // Deep enough to overflow the stack dropping it.
    assert!(Element::parse(&nested(100_000)).is_err());
}

#[test]
fn escaping_makes_text_safe() {
    assert_eq!(
        escape("<a href=\"x\">&'"),
        "&lt;a href=&quot;x&quot;&gt;&amp;&apos;"
    );
}
