use super::*;

const AV_TRANSPORT: &str = "urn:schemas-upnp-org:service:AVTransport:1";
const RENDERING_CONTROL: &str = "urn:schemas-upnp-org:service:RenderingControl:1";

#[test]
fn faults_by_number_and_service() {
    assert_eq!(
        Fault::from_code(AV_TRANSPORT, 701),
        Fault::TransitionNotAvailable
    );
    assert_eq!(Fault::from_code(AV_TRANSPORT, 702), Fault::NoContents);
    assert_eq!(
        Fault::from_code(AV_TRANSPORT, 718),
        Fault::InvalidInstanceId
    );
    assert_eq!(
        Fault::from_code(RENDERING_CONTROL, 702),
        Fault::InvalidInstanceId
    );
    assert_eq!(Fault::from_code(RENDERING_CONTROL, 701), Fault::Other);
    assert_eq!(Fault::from_code(RENDERING_CONTROL, 402), Fault::InvalidArgs);
    assert_eq!(Fault::from_code(AV_TRANSPORT, 999), Fault::Other);
}

#[test]
fn a_call_reads_back_as_it_was_made() {
    let body = envelope(&format!(
        "<u:Seek xmlns:u=\"{AV_TRANSPORT}\">{}</u:Seek>",
        arguments(&[
            ("InstanceID", "0"),
            ("Unit", "REL_TIME"),
            ("Target", "<0:01:02>")
        ])
    ));
    let message = http::Message {
        head: http::Head {
            start: "POST /control HTTP/1.1".into(),
            headers: vec![("SOAPACTION".into(), format!("\"{AV_TRANSPORT}#Seek\""))],
        },
        body: body.into_bytes(),
    };
    let call = read_call(&message).unwrap();
    assert_eq!(call.service, AV_TRANSPORT);
    assert_eq!(call.action, "Seek");
    assert_eq!(call.arg("Target"), Some("<0:01:02>"));
}
