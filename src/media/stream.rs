use std::path::Path;

use crate::config::Config;
use crate::dlna::{av_transport, connection_manager, metadata};
use crate::media::manager::{get_mime_type, MediaFile};
use crate::soap::SoapClient;

/// Configures and starts playback of a media file on the DLNA renderer.
///
/// Flow: PrepareForConnection (optional) → SetAVTransportURI → Play
pub async fn stream_media(
    client: &SoapClient,
    config: &Config,
    av_control_url: &str,
    cm_control_url: &str,
    media_file: &MediaFile,
    subtitle_url: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    if !Path::new(&media_file.path).exists() {
        return Err(format!("File '{}' not found.", media_file.path).into());
    }

    // URL the renderer uses to fetch the media over HTTP
    let media_url = format!(
        "http://{}:{}/media/{}",
        config.http_address, config.http_port, media_file.relative_path
    );

    // PrepareForConnection is optional — silently ignore unsupported devices
    let _ = connection_manager::prepare_connection(client, cm_control_url).await;

    let mime_type = get_mime_type(&media_file.path);
    let metadata = metadata::build(&media_file.name, &media_url, mime_type, subtitle_url);

    av_transport::set_uri(client, av_control_url, &media_url, &metadata).await?;
    av_transport::play(client, av_control_url).await?;

    Ok(())
}
