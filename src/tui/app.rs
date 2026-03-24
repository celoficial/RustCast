use std::collections::HashSet;

use tokio::{sync::watch, task::JoinHandle};

use crate::discovery::health::PollSignal;
use crate::media::manager::MediaFile;

// ── Device entry returned by the background scan ──────────────────────────────

pub struct ScannedDevice {
    pub usn: String,
    pub name: String,
    pub av_url: String,
    pub cm_url: String,
}

// ── App phase ─────────────────────────────────────────────────────────────────

#[derive(PartialEq, Eq)]
pub enum AppPhase {
    Scanning,
    Idle,
    Playing,
    SeekInput,
}

// ── Focus panel ───────────────────────────────────────────────────────────────

#[derive(PartialEq, Eq)]
pub enum FocusPanel {
    Devices,
    Media,
}

// ── Unified app state ─────────────────────────────────────────────────────────

pub struct AppState {
    // scan
    pub phase: AppPhase,
    pub scan_rx: Option<std::sync::mpsc::Receiver<Vec<ScannedDevice>>>,
    pub tick: u8,

    // devices panel
    pub devices: Vec<ScannedDevice>,
    pub device_cursor: usize,
    pub active_device: Option<usize>, // index into devices[]
    pub av_url: String,
    pub cm_url: String,

    // media panel
    pub media_files: Vec<MediaFile>,
    pub media_cursor: usize,
    pub media_selected: HashSet<usize>,
    pub media_scroll: usize,
    pub media_viewport_h: usize,

    // focus
    pub focus: FocusPanel,

    // playback
    pub playlist: Vec<usize>, // sorted indices into media_files
    pub playlist_pos: usize,  // current position in playlist
    pub transport_state: String,
    pub poll_task: Option<JoinHandle<()>>,
    pub poll_rx: Option<watch::Receiver<PollSignal>>,
    pub seek_input: String,

    // status bar
    pub status_msg: Option<String>,
    pub status_ticks: u8, // auto-clear countdown (decremented per tick)
}

impl AppState {
    pub fn new(media_files: Vec<MediaFile>) -> Self {
        Self {
            phase: AppPhase::Idle,
            scan_rx: None,
            tick: 0,

            devices: vec![],
            device_cursor: 0,
            active_device: None,
            av_url: String::new(),
            cm_url: String::new(),

            media_files,
            media_cursor: 0,
            media_selected: HashSet::new(),
            media_scroll: 0,
            media_viewport_h: 20,

            focus: FocusPanel::Devices,

            playlist: vec![],
            playlist_pos: 0,
            transport_state: String::new(),
            poll_task: None,
            poll_rx: None,
            seek_input: String::new(),

            status_msg: None,
            status_ticks: 0,
        }
    }

    pub fn set_status(&mut self, msg: impl Into<String>, ticks: u8) {
        self.status_msg = Some(msg.into());
        self.status_ticks = ticks;
    }

    pub fn clear_playback(&mut self) {
        if let Some(h) = self.poll_task.take() {
            h.abort();
        }
        self.poll_rx = None;
        self.playlist.clear();
        self.playlist_pos = 0;
        self.transport_state.clear();
        self.phase = AppPhase::Idle;
    }

    /// Returns the MediaFile for the currently playing track, if any.
    pub fn current_track(&self) -> Option<&MediaFile> {
        if matches!(self.phase, AppPhase::Playing | AppPhase::SeekInput) {
            self.playlist
                .get(self.playlist_pos)
                .and_then(|&i| self.media_files.get(i))
        } else {
            None
        }
    }

    /// Scroll media list so cursor is in viewport.
    pub fn scroll_media_to_cursor(&mut self) {
        if self.media_cursor < self.media_scroll {
            self.media_scroll = self.media_cursor;
        } else if self.media_viewport_h > 0
            && self.media_cursor >= self.media_scroll + self.media_viewport_h
        {
            self.media_scroll = self.media_cursor - self.media_viewport_h + 1;
        }
    }
}
