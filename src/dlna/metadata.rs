use crate::soap::xml_escape;

/// Builds a DIDL-Lite XML metadata string, already XML-escaped for embedding
/// directly in a SOAP body (as the value of CurrentURIMetaData).
pub fn build(title: &str, media_url: &str, mime_type: &str, subtitle_url: Option<&str>) -> String {
    let title_esc = xml_escape(title);
    let url_esc = xml_escape(media_url);
    let mime_esc = xml_escape(mime_type);

    let (sec_ns, subtitle_elements) = match subtitle_url {
        Some(url) => {
            let esc = xml_escape(url);
            (
                r#" xmlns:sec="http://www.sec.co.kr/""#.to_string(),
                format!(
                    r#"<res protocolInfo="http-get:*:text/srt:*">{}</res><sec:CaptionInfoEx sec:type="srt">{}</sec:CaptionInfoEx>"#,
                    esc, esc
                ),
            )
        }
        None => (String::new(), String::new()),
    };

    let didl = format!(
        r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/"{}><item id="0" parentID="-1" restricted="1"><dc:title>{}</dc:title><res protocolInfo="http-get:*:{}:DLNA.ORG_OP=01">{}</res>{}</item></DIDL-Lite>"#,
        sec_ns, title_esc, mime_esc, url_esc, subtitle_elements
    );

    // Must be XML-escaped when embedded as text content inside the SOAP envelope
    xml_escape(&didl)
}
