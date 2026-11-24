//! UPnP's actions: SOAP over HTTP, each a POST to a service's control URL,
//! and the faults a device refuses with.

use std::time::Duration;

use crate::device::Service;
use crate::error::{Error, Result};
use crate::http::{self, Url};
use crate::xml::{Element, escape};

/// How long a device has to answer an action. Some fetch what they're
/// given before answering SetAVTransportURI.
pub const ANSWER: Duration = Duration::from_secs(10);

const ENVELOPE: &str = "http://schemas.xmlsoap.org/soap/envelope/";
const ENCODING: &str = "http://schemas.xmlsoap.org/soap/encoding/";

/// Why a device refused an action, by UPnP's number for it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 401: it hasn't that action.
    InvalidAction,
    /// 402: the arguments are wrong.
    InvalidArgs,
    /// 501: it tried and failed.
    ActionFailed,
    /// 600 and 601: an argument's value is wrong, or out of range.
    InvalidValue,
    /// 602: an optional action it hasn't.
    NotImplemented,
    /// 701: not now, as Play with nothing set or Seek while stopped.
    TransitionNotAvailable,
    /// 702: it has nothing to play.
    NoContents,
    /// 703: it couldn't read what it was given.
    ReadError,
    /// 704: it can't play that format.
    FormatNotSupported,
    /// 705: another controller has it locked.
    TransportLocked,
    /// 710: it doesn't seek that way.
    SeekModeNotSupported,
    /// 711: it can't seek there.
    IllegalSeekTarget,
    /// 714: it doesn't take that MIME type.
    IllegalMimeType,
    /// 715: what it was given is busy.
    ContentBusy,
    /// 716: it couldn't find what it was given.
    ResourceNotFound,
    /// 717: it doesn't play at that speed.
    PlaySpeedNotSupported,
    /// AVTransport's 718 and RenderingControl's 702: no such instance.
    InvalidInstanceId,
    /// Another number.
    Other,
}

impl Fault {
    /// What `code` means from `service`, as each numbers its own from 700.
    pub fn from_code(service: &str, code: u16) -> Self {
        let rendering = service.contains(":RenderingControl:");
        match code {
            401 => Self::InvalidAction,
            402 => Self::InvalidArgs,
            501 => Self::ActionFailed,
            600 | 601 => Self::InvalidValue,
            602 => Self::NotImplemented,
            702 if rendering => Self::InvalidInstanceId,
            _ if rendering => Self::Other,
            701 => Self::TransitionNotAvailable,
            702 => Self::NoContents,
            703 => Self::ReadError,
            704 => Self::FormatNotSupported,
            705 => Self::TransportLocked,
            710 => Self::SeekModeNotSupported,
            711 => Self::IllegalSeekTarget,
            714 => Self::IllegalMimeType,
            715 => Self::ContentBusy,
            716 => Self::ResourceNotFound,
            717 => Self::PlaySpeedNotSupported,
            718 => Self::InvalidInstanceId,
            _ => Self::Other,
        }
    }
}

/// What an action gave back, by name.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Answer(pub Vec<(String, String)>);

impl Answer {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

fn envelope(body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
         <s:Envelope xmlns:s=\"{ENVELOPE}\" s:encodingStyle=\"{ENCODING}\">\
         <s:Body>{body}</s:Body></s:Envelope>"
    )
}

fn arguments(args: &[(&str, &str)]) -> String {
    args.iter()
        .map(|(name, value)| format!("<{name}>{}</{name}>", escape(value)))
        .collect()
}

/// Have `service` do `action` with `args`, waiting up to `timeout`.
pub fn call(
    service: &Service,
    action: &str,
    args: &[(&str, &str)],
    timeout: Duration,
) -> Result<Answer> {
    let url = Url::parse(&service.control)
        .ok_or_else(|| Error::protocol(format!("a control URL {:?}", service.control)))?;
    let body = envelope(&format!(
        "<u:{action} xmlns:u=\"{}\">{}</u:{action}>",
        escape(&service.kind),
        arguments(args)
    ));
    let soap_action = format!("\"{}#{action}\"", service.kind);
    let response = http::request(
        "POST",
        &url,
        &[
            ("Content-Type", "text/xml; charset=\"utf-8\""),
            ("SOAPAction", &soap_action),
        ],
        body.as_bytes(),
        timeout,
    )?;
    let status = response.head.status().unwrap_or_default();
    let envelope = Element::parse(&response.text());
    if let Ok(envelope) = &envelope
        && let Some(error) = envelope.find("UPnPError")
    {
        let code = error
            .child_text("errorCode")
            .and_then(|code| code.parse().ok())
            .unwrap_or_default();
        let description = error.child_text("errorDescription").unwrap_or_default();
        log::debug!("dlna: {action} refused: {code} {description}");
        return Err(Error::Upnp {
            action: action.to_owned(),
            fault: Fault::from_code(&service.kind, code),
            code,
            description: description.to_owned(),
        });
    }
    if status != 200 {
        return Err(Error::Http {
            request: action.to_owned(),
            status,
        });
    }
    let envelope = envelope.map_err(|err| Error::protocol(format!("{action}'s answer: {err}")))?;
    let answer = envelope
        .child("Body")
        .and_then(|body| body.children.first())
        .ok_or_else(|| Error::protocol(format!("{action}'s answer is empty")))?;
    Ok(Answer(
        answer
            .children
            .iter()
            .map(|out| (out.name.clone(), out.text().to_owned()))
            .collect(),
    ))
}

/// A call as a device takes it.
#[cfg(any(test, feature = "fake"))]
pub fn read_call(message: &http::Message) -> Option<crate::fake::Call> {
    let header = message.header("SOAPAction")?.trim_matches('"');
    let (kind, action) = header.rsplit_once('#')?;
    let envelope = Element::parse(&message.text()).ok()?;
    let call = envelope.child("Body")?.children.first()?;
    if call.name != action {
        return None;
    }
    let args = call
        .children
        .iter()
        .map(|arg| (arg.name.clone(), arg.text.clone()))
        .collect();
    Some(crate::fake::Call {
        service: kind.to_owned(),
        action: action.to_owned(),
        args,
        at: std::time::Instant::now(),
    })
}

/// The answer to `action`, as a device gives it.
#[cfg(any(test, feature = "fake"))]
pub fn answer(kind: &str, action: &str, out: &[(&str, &str)]) -> String {
    envelope(&format!(
        "<u:{action}Response xmlns:u=\"{kind}\">{}</u:{action}Response>",
        arguments(out)
    ))
}

/// A refusal, as a device gives it, with HTTP's 500.
#[cfg(any(test, feature = "fake"))]
pub fn fault(code: u16, description: &str) -> String {
    envelope(&format!(
        "<s:Fault><faultcode>s:Client</faultcode><faultstring>UPnPError</faultstring>\
         <detail><UPnPError xmlns=\"urn:schemas-upnp-org:control-1-0\">\
         <errorCode>{code}</errorCode><errorDescription>{}</errorDescription>\
         </UPnPError></detail></s:Fault>",
        escape(description)
    ))
}

#[cfg(test)]
#[path = "tests/soap.rs"]
mod tests;
