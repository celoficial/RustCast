use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct MediaFile {
    pub name: String,          // bare filename: "movie.mkv"
    pub path: String,          // absolute filesystem path
    pub relative_path: String, // relative to media root: "Action/movie.mkv"
}

pub fn list_media_files(directory: &str) -> Vec<MediaFile> {
    let root = Path::new(directory);
    let mut results = Vec::new();
    collect_media(root, root, &mut results);
    results.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    results
}

/// Recursively collects media files under `current`, computing `relative_path`
/// relative to `root`. Does not follow symlinks (avoids infinite loops).
fn collect_media(root: &Path, current: &Path, results: &mut Vec<MediaFile>) {
    let entries = match std::fs::read_dir(current) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let file_type = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };

        let path = entry.path();

        if file_type.is_dir() {
            collect_media(root, &path, results);
        } else if file_type.is_file() {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default();

            if is_supported_format(ext) {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                let relative_path = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    // Normalise to forward slashes for URL construction on all platforms
                    .replace('\\', "/");

                results.push(MediaFile {
                    name,
                    path: path.to_string_lossy().to_string(),
                    relative_path,
                });
            }
        }
    }
}

fn is_supported_format(ext: &str) -> bool {
    matches!(ext.to_lowercase().as_str(), "mp4" | "mkv" | "avi" | "mp3")
}
