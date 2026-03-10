use std::collections::HashSet;

/// What to do after a playlist ends or is stopped.
#[derive(Debug)]
pub enum SessionAction {
    ReselectMedia,
    ReselectDevice,
    Rescan,
    Quit,
}

/// Why a per-track control loop exited.
#[derive(Debug)]
pub enum TrackExit {
    NextTrack,
    StopPlaylist,
    Quit,
}

/// Parses a multi-select string into 0-based indices.
/// Supports: "1", "1,3,5", "2-4", "all"
pub fn parse_selection(input: &str, count: usize) -> Vec<usize> {
    let input = input.trim().to_lowercase();
    if input == "all" {
        return (0..count).collect();
    }
    let mut indices = Vec::new();
    for part in input.split(',') {
        let part = part.trim();
        if let Some(dash_pos) = part.find('-') {
            let start: usize = part[..dash_pos].trim().parse().unwrap_or(0);
            let end: usize = part[dash_pos + 1..].trim().parse().unwrap_or(0);
            if start > 0 && end >= start {
                for i in start..=end {
                    if i <= count {
                        indices.push(i - 1);
                    }
                }
            }
        } else if let Ok(n) = part.parse::<usize>() {
            if n > 0 && n <= count {
                indices.push(n - 1);
            }
        }
    }
    let mut seen = HashSet::new();
    indices.retain(|x| seen.insert(*x));
    indices
}

pub async fn ask_what_next(
    stdin_rx: &mut tokio::sync::mpsc::UnboundedReceiver<String>,
) -> SessionAction {
    loop {
        println!("\nWhat would you like to do?");
        println!("  [m] Select new media (same device)");
        println!("  [d] Choose a different device");
        println!("  [r] Rescan for devices");
        println!("  [q] Quit");
        let input = stdin_rx.recv().await.unwrap_or_default();
        match input.trim() {
            "m" | "M" => return SessionAction::ReselectMedia,
            "d" | "D" => return SessionAction::ReselectDevice,
            "r" | "R" => return SessionAction::Rescan,
            "q" | "Q" => return SessionAction::Quit,
            _ => println!("Enter m, d, r, or q."),
        }
    }
}
