use crate::media::manager::MediaFile;
use std::path::Path;

/// Looks for a `.srt` subtitle alongside the media file (same base name).
/// Returns the HTTP URL the renderer uses to fetch it, or None if not found.
pub fn find_subtitle(media_file: &MediaFile, http_address: &str, http_port: u16) -> Option<String> {
    let path = Path::new(&media_file.path);
    let stem = path.file_stem()?.to_str()?;
    let dir = path.parent()?;

    for ext in &["srt", "SRT"] {
        let candidate = dir.join(format!("{}.{}", stem, ext));
        if candidate.exists() {
            let parent_rel = Path::new(&media_file.relative_path)
                .parent()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            let filename = candidate.file_name()?.to_string_lossy().to_string();
            let rel = if parent_rel.is_empty() {
                filename
            } else {
                format!("{}/{}", parent_rel, filename)
            };
            return Some(format!(
                "http://{}:{}/media/{}",
                http_address, http_port, rel
            ));
        }
    }
    None
}
