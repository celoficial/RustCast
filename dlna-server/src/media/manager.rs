use std::fs;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct MediaFile {
    pub name: String,
    pub path: String,
}

pub fn list_media_files(directory: &str) -> Vec<MediaFile> {
    let mut media_files = Vec::new();
    println!("Reading media directory: {}", directory);
    if let Ok(entries) = fs::read_dir(directory) {
        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension() {
                        if is_supported_format(ext.to_str().unwrap_or_default()) {
                            media_files.push(MediaFile {
                                name: path.file_name().unwrap().to_string_lossy().to_string(),
                                path: path.to_string_lossy().to_string(),
                            });
                        }
                    }
                }
            }
        }
    }
    media_files
}

fn is_supported_format(ext: &str) -> bool {
    matches!(ext.to_lowercase().as_str(), "mp4" | "mkv" | "avi" | "mp3")
}
