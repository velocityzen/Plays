//! XML as UPnP uses it, read into a small tree: elements by their local
//! names, as devices are loose with prefixes, with the text and attributes
//! unescaped. Written by hand, escaped.

use std::borrow::Cow;

use quick_xml::XmlVersion;
use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;

/// How deep elements may nest. UPnP's go a dozen deep; a tree much deeper
/// would overflow the stack being dropped or searched.
const MAX_DEPTH: usize = 64;

/// An element and what's in it.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Element {
    /// Its local name, without a prefix.
    pub name: String,
    /// By local name; namespace declarations are left out.
    pub attributes: Vec<(String, String)>,
    pub children: Vec<Element>,
    /// All its own text, as it is.
    pub text: String,
}

impl Element {
    /// The document's root element.
    pub fn parse(document: &str) -> Result<Self, String> {
        let document = document.trim_start_matches('\u{feff}');
        let mut reader = Reader::from_str(document);
        let mut open: Vec<Element> = Vec::new();
        loop {
            let event = reader.read_event().map_err(|err| err.to_string())?;
            if matches!(event, Event::Start(_) | Event::Empty(_)) && open.len() >= MAX_DEPTH {
                return Err(format!("elements nested more than {MAX_DEPTH} deep"));
            }
            match event {
                Event::Start(start) => open.push(Self::start(&start)?),
                Event::Empty(start) => {
                    let element = Self::start(&start)?;
                    match open.last_mut() {
                        Some(parent) => parent.children.push(element),
                        None => return Ok(element),
                    }
                }
                Event::End(_) => {
                    let element = open.pop().ok_or("an end without a start")?;
                    match open.last_mut() {
                        Some(parent) => parent.children.push(element),
                        None => return Ok(element),
                    }
                }
                Event::Text(text) => {
                    if let Some(element) = open.last_mut() {
                        element.text.push_str(&text.xml10_content());
                    }
                }
                Event::CData(data) => {
                    if let Some(element) = open.last_mut() {
                        element.text.push_str(&data.into_inner());
                    }
                }
                Event::GeneralRef(reference) => {
                    if let Some(element) = open.last_mut() {
                        match reference
                            .resolve_char_ref()
                            .map_err(|err| err.to_string())?
                        {
                            Some(char) => element.text.push(char),
                            None => match resolve_predefined_entity(&reference) {
                                Some(text) => element.text.push_str(text),
                                None => return Err(format!("an unknown entity &{};", &*reference)),
                            },
                        }
                    }
                }
                Event::Eof => return Err("no root element, or one not closed".into()),
                _ => {}
            }
        }
    }

    fn start(start: &BytesStart<'_>) -> Result<Self, String> {
        let mut attributes = Vec::new();
        for attribute in start.attributes() {
            let attribute = attribute.map_err(|err| err.to_string())?;
            if attribute.key.as_namespace_binding().is_some() {
                continue;
            }
            let value = attribute
                .normalized_value(XmlVersion::Implicit1_0)
                .map_err(|err| err.to_string())?;
            attributes.push((
                attribute.key.local_name().as_ref().to_owned(),
                value.into_owned(),
            ));
        }
        Ok(Self {
            name: start.local_name().as_ref().to_owned(),
            attributes,
            children: Vec::new(),
            text: String::new(),
        })
    }

    /// Its text without the space around it.
    pub fn text(&self) -> &str {
        self.text.trim()
    }

    /// The first child called `name`.
    pub fn child(&self, name: &str) -> Option<&Self> {
        self.children.iter().find(|child| child.name == name)
    }

    /// The children called `name`.
    pub fn children<'a>(&'a self, name: &str) -> impl Iterator<Item = &'a Self> {
        self.children.iter().filter(move |child| child.name == name)
    }

    /// The text of the first child called `name`.
    pub fn child_text(&self, name: &str) -> Option<&str> {
        self.child(name).map(Self::text)
    }

    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// The first element called `name` inside, depth first.
    pub fn find(&self, name: &str) -> Option<&Self> {
        self.children.iter().find_map(|child| {
            (child.name == name)
                .then_some(child)
                .or_else(|| child.find(name))
        })
    }
}

/// `text` safe in XML's text and attributes.
pub fn escape(text: &str) -> Cow<'_, str> {
    quick_xml::escape::escape(text)
}

#[cfg(test)]
mod tests {
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
}
