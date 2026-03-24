use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use futures::future::join_all;
use futures::StreamExt;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};
use tokio::time::{interval, Duration};

use crate::config::Config;
use crate::discovery::device::{
    extract_base_url, fetch_device_description, find_control_url_with_fallback, reconnect_device,
};
use crate::discovery::health::{spawn_poll_task, PollSignal};
use crate::discovery::ssdp::discover_ssdp;
use crate::media::finder::find_subtitle;
use crate::media::manager::MediaFile;
use crate::media::stream::stream_media;
use crate::soap::SoapClient;
use crate::tui::{
    app::{AppPhase, AppState, FocusPanel, ScannedDevice},
    event::TuiEvent,
    terminal::TerminalGuard,
};

const SPIN: &[char] = &['|', '/', '-', '\\'];

// ── Scan thread ───────────────────────────────────────────────────────────────

fn start_scan(config: &Config) -> std::sync::mpsc::Receiver<Vec<ScannedDevice>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let cfg = config.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("scan runtime");
        let devices = rt.block_on(async move {
            let devs = discover_ssdp(&cfg.multicast_address, cfg.multicast_port)
                .await
                .unwrap_or_default();
            let descs = join_all(devs.iter().map(|d| {
                let loc = d.location.clone();
                async move { fetch_device_description(&loc).await }
            }))
            .await;
            devs.into_iter()
                .zip(descs)
                .map(|(d, desc)| {
                    let base = extract_base_url(&d.location);
                    let (name, av, cm) = match &desc {
                        Ok(dd) => (
                            if dd.device.friendly_name.is_empty() {
                                d.location.clone()
                            } else {
                                dd.device.friendly_name.clone()
                            },
                            find_control_url_with_fallback(
                                dd,
                                "AVTransport",
                                &base,
                                "/upnp/control/AVTransport1",
                            ),
                            find_control_url_with_fallback(
                                dd,
                                "ConnectionManager",
                                &base,
                                "/upnp/control/ConnectionManager1",
                            ),
                        ),
                        Err(_) => (d.location.clone(), String::new(), String::new()),
                    };
                    ScannedDevice {
                        usn: d.usn,
                        name,
                        av_url: av,
                        cm_url: cm,
                    }
                })
                .collect::<Vec<_>>()
        });
        tx.send(devices).ok();
    });
    rx
}

// ── Main entry point ──────────────────────────────────────────────────────────

pub async fn run_app(
    terminal: &mut TerminalGuard,
    media_files: Vec<MediaFile>,
    config: Config,
    soap: SoapClient,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut state = AppState::new(media_files);
    state.scan_rx = Some(start_scan(&config));
    state.phase = AppPhase::Scanning;

    let mut events = EventStream::new();
    let mut tick_timer = interval(Duration::from_millis(100));

    loop {
        terminal.draw(|f| render_app(f, &mut state))?;

        // Check scan channel on every tick (non-blocking)
        if state.phase == AppPhase::Scanning {
            if let Some(rx) = &state.scan_rx {
                if let Ok(devices) = rx.try_recv() {
                    state.devices = devices;
                    state.scan_rx = None;
                    state.phase = AppPhase::Idle;
                }
            }
        }

        // Auto-clear transient status messages
        if state.status_ticks > 0 {
            state.status_ticks -= 1;
            if state.status_ticks == 0 {
                state.status_msg = None;
            }
        }

        // Event select — poll_rx is optional so we use an if/else
        let evt = if let Some(ref mut rx) = state.poll_rx {
            tokio::select! {
                maybe = events.next() => key_or_tick(maybe),
                _ = tick_timer.tick() => TuiEvent::Tick,
                changed = rx.changed() => {
                    if changed.is_ok() {
                        TuiEvent::Poll(rx.borrow().clone())
                    } else {
                        TuiEvent::Tick
                    }
                }
            }
        } else {
            tokio::select! {
                maybe = events.next() => key_or_tick(maybe),
                _ = tick_timer.tick() => TuiEvent::Tick,
            }
        };

        match evt {
            TuiEvent::Tick => {
                state.tick = state.tick.wrapping_add(1);
            }
            TuiEvent::Poll(signal) => {
                if handle_poll(&mut state, signal, &soap, &config).await {
                    break; // quit requested from poll handler
                }
            }
            TuiEvent::Key(key) => {
                if handle_key(&mut state, key, &config, &soap).await? {
                    break; // quit
                }
            }
        }
    }

    state.clear_playback();
    Ok(())
}

fn key_or_tick(maybe: Option<Result<Event, std::io::Error>>) -> TuiEvent {
    match maybe {
        Some(Ok(Event::Key(k))) if k.kind == KeyEventKind::Press => TuiEvent::Key(k),
        _ => TuiEvent::Tick,
    }
}

// ── Poll signal handler ───────────────────────────────────────────────────────

/// Returns true if the app should quit.
async fn handle_poll(
    state: &mut AppState,
    signal: PollSignal,
    soap: &SoapClient,
    config: &Config,
) -> bool {
    match signal {
        PollSignal::Paused => {
            state.transport_state = "PAUSED_PLAYBACK".to_string();
        }
        PollSignal::Resumed => {
            state.transport_state = "PLAYING".to_string();
        }
        PollSignal::Stopped => {
            advance_playlist(state, soap, config).await;
        }
        PollSignal::DeviceOffline => {
            handle_device_offline(state, soap, config).await;
        }
        PollSignal::Running => {}
    }
    false
}

/// Advance to the next track or reset to Idle when playlist is done.
async fn advance_playlist(state: &mut AppState, soap: &SoapClient, config: &Config) {
    if let Some(h) = state.poll_task.take() {
        h.abort();
    }
    state.poll_rx = None;

    let next = state.playlist_pos + 1;
    if next < state.playlist.len() {
        state.playlist_pos = next;
        start_track(state, soap, config).await;
    } else {
        state.clear_playback();
        state.focus = FocusPanel::Media;
        state.set_status(
            "Playlist finished — select files and press Enter to play again",
            50,
        );
    }
}

/// Try to reconnect after device goes offline. Restarts track on success.
async fn handle_device_offline(state: &mut AppState, soap: &SoapClient, config: &Config) {
    if let Some(h) = state.poll_task.take() {
        h.abort();
    }
    state.poll_rx = None;
    state.set_status("Device offline — reconnecting...", 5);

    let usn = state
        .active_device
        .and_then(|i| state.devices.get(i))
        .map(|d| d.usn.clone())
        .unwrap_or_default();
    match reconnect_device(&config.multicast_address, config.multicast_port, &usn).await {
        Some((av, cm)) => {
            state.av_url = av;
            state.cm_url = cm;
            start_track(state, soap, config).await;
        }
        None => {
            state.clear_playback();
            state.active_device = None;
            state.av_url.clear();
            state.cm_url.clear();
            state.set_status("Device lost — select a new device", 50);
        }
    }
}

/// Call stream_media + spawn_poll_task for the current playlist position.
async fn start_track(state: &mut AppState, soap: &SoapClient, config: &Config) {
    let media_file = match state
        .playlist
        .get(state.playlist_pos)
        .and_then(|&i| state.media_files.get(i))
    {
        Some(f) => f,
        None => {
            state.clear_playback();
            return;
        }
    };

    let subtitle_url = find_subtitle(media_file, &config.http_address, config.http_port);

    if let Err(e) = stream_media(
        soap,
        config,
        &state.av_url,
        &state.cm_url,
        media_file,
        subtitle_url.as_deref(),
    )
    .await
    {
        state.set_status(format!("Stream error: {} — skipping", e), 30);
        // Try to advance to next track
        let next = state.playlist_pos + 1;
        if next < state.playlist.len() {
            state.playlist_pos = next;
            // Recurse via Box::pin to avoid infinite stack growth on repeated errors
            Box::pin(start_track(state, soap, config)).await;
        } else {
            state.clear_playback();
        }
        return;
    }

    state.transport_state = "PLAYING".to_string();
    state.phase = AppPhase::Playing;

    let (poll_task, poll_rx) = spawn_poll_task(soap.clone(), state.av_url.clone());
    state.poll_task = Some(poll_task);
    state.poll_rx = Some(poll_rx);
}

// ── Key handler ───────────────────────────────────────────────────────────────

/// Returns true if the app should quit.
async fn handle_key(
    state: &mut AppState,
    key: crossterm::event::KeyEvent,
    config: &Config,
    soap: &SoapClient,
) -> Result<bool, Box<dyn std::error::Error>> {
    // Global quit
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        if matches!(state.phase, AppPhase::Playing | AppPhase::SeekInput) {
            crate::dlna::av_transport::stop(soap, &state.av_url)
                .await
                .ok();
        }
        return Ok(true);
    }

    // Seek input mode — capture all chars
    if state.phase == AppPhase::SeekInput {
        match key.code {
            KeyCode::Char(c) => state.seek_input.push(c),
            KeyCode::Backspace => {
                state.seek_input.pop();
            }
            KeyCode::Enter => {
                let pos = state.seek_input.clone();
                state.seek_input.clear();
                state.phase = AppPhase::Playing;
                match crate::dlna::av_transport::seek(soap, &state.av_url, &pos).await {
                    Ok(()) => state.set_status(format!("Seeked to {}", pos), 20),
                    Err(e) => state.set_status(format!("Seek failed: {}", e), 20),
                }
            }
            KeyCode::Esc => {
                state.seek_input.clear();
                state.phase = AppPhase::Playing;
            }
            _ => {}
        }
        return Ok(false);
    }

    match key.code {
        // ── Focus switch ─────────────────────────────────────────────────────
        KeyCode::Tab => {
            state.focus = match state.focus {
                FocusPanel::Devices => FocusPanel::Media,
                FocusPanel::Media => FocusPanel::Devices,
            };
        }

        // ── Navigation ───────────────────────────────────────────────────────
        KeyCode::Up | KeyCode::Char('k') => match state.focus {
            FocusPanel::Devices => {
                if state.device_cursor > 0 {
                    state.device_cursor -= 1;
                }
            }
            FocusPanel::Media => {
                if state.media_cursor > 0 {
                    state.media_cursor -= 1;
                    state.scroll_media_to_cursor();
                }
            }
        },
        KeyCode::Down | KeyCode::Char('j') => match state.focus {
            FocusPanel::Devices => {
                if !state.devices.is_empty() && state.device_cursor < state.devices.len() - 1 {
                    state.device_cursor += 1;
                }
            }
            FocusPanel::Media => {
                if state.media_cursor + 1 < state.media_files.len() {
                    state.media_cursor += 1;
                    state.scroll_media_to_cursor();
                }
            }
        },

        // ── Enter ─────────────────────────────────────────────────────────────
        KeyCode::Enter => match state.focus {
            FocusPanel::Devices => {
                if !state.devices.is_empty() {
                    let idx = state.device_cursor;
                    let dev = &state.devices[idx];
                    state.active_device = Some(idx);
                    state.av_url = dev.av_url.clone();
                    state.cm_url = dev.cm_url.clone();
                    let name = dev.name.clone();
                    state.set_status(format!("Connected to {}", name), 20);
                }
            }
            FocusPanel::Media => {
                if state.active_device.is_none() {
                    state.set_status("Connect to a device first (Tab → Devices, Enter)", 30);
                } else if state.media_selected.is_empty() {
                    state.set_status("Select files with Space first", 20);
                } else {
                    let mut indices: Vec<usize> = state.media_selected.iter().copied().collect();
                    indices.sort_unstable();
                    state.playlist = indices;
                    state.playlist_pos = 0;

                    // Stop any existing playback
                    if matches!(state.phase, AppPhase::Playing | AppPhase::SeekInput) {
                        crate::dlna::av_transport::stop(soap, &state.av_url)
                            .await
                            .ok();
                        if let Some(h) = state.poll_task.take() {
                            h.abort();
                        }
                        state.poll_rx = None;
                    }

                    start_track(state, soap, config).await;
                }
            }
        },

        // ── Media panel selection ─────────────────────────────────────────────
        KeyCode::Char(' ') if state.focus == FocusPanel::Media => {
            let cur = state.media_cursor;
            if state.media_selected.contains(&cur) {
                state.media_selected.remove(&cur);
            } else {
                state.media_selected.insert(cur);
            }
        }
        KeyCode::Char('a') | KeyCode::Char('A') if state.focus == FocusPanel::Media => {
            if state.media_selected.len() == state.media_files.len() {
                state.media_selected.clear();
            } else {
                state.media_selected = (0..state.media_files.len()).collect();
            }
        }

        // ── Rescan ────────────────────────────────────────────────────────────
        KeyCode::Char('r') | KeyCode::Char('R') => {
            state.devices.clear();
            state.device_cursor = 0;
            state.active_device = None;
            state.scan_rx = Some(start_scan(config));
            state.phase = if matches!(state.phase, AppPhase::Playing | AppPhase::SeekInput) {
                AppPhase::Playing
            } else {
                AppPhase::Scanning
            };
        }

        // ── Playback controls (only when Playing) ─────────────────────────────
        KeyCode::Char('p') | KeyCode::Char('P') if matches!(state.phase, AppPhase::Playing) => {
            match state.transport_state.as_str() {
                "PLAYING" => match crate::dlna::av_transport::pause(soap, &state.av_url).await {
                    Ok(()) => {
                        state.transport_state = "PAUSED_PLAYBACK".to_string();
                    }
                    Err(e) => state.set_status(format!("Pause failed: {}", e), 20),
                },
                "PAUSED_PLAYBACK" => {
                    match crate::dlna::av_transport::play(soap, &state.av_url).await {
                        Ok(()) => {
                            state.transport_state = "PLAYING".to_string();
                        }
                        Err(e) => state.set_status(format!("Resume failed: {}", e), 20),
                    }
                }
                _ => {}
            }
        }
        KeyCode::Char('n') | KeyCode::Char('N') if matches!(state.phase, AppPhase::Playing) => {
            crate::dlna::av_transport::stop(soap, &state.av_url)
                .await
                .ok();
            if let Some(h) = state.poll_task.take() {
                h.abort();
            }
            state.poll_rx = None;
            advance_playlist(state, soap, config).await;
        }
        KeyCode::Char('s') | KeyCode::Char('S') if matches!(state.phase, AppPhase::Playing) => {
            crate::dlna::av_transport::stop(soap, &state.av_url)
                .await
                .ok();
            state.clear_playback();
        }
        KeyCode::Char('f') | KeyCode::Char('F') if matches!(state.phase, AppPhase::Playing) => {
            state.seek_input.clear();
            state.phase = AppPhase::SeekInput;
        }

        // ── Quit ──────────────────────────────────────────────────────────────
        KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
            if matches!(state.phase, AppPhase::Playing | AppPhase::SeekInput) {
                crate::dlna::av_transport::stop(soap, &state.av_url)
                    .await
                    .ok();
            }
            return Ok(true);
        }

        _ => {}
    }

    Ok(false)
}

// ── Rendering ─────────────────────────────────────────────────────────────────

fn render_app(f: &mut Frame, state: &mut AppState) {
    let area = f.area();

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(5),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .split(area);

    let panels = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(rows[0]);

    // Update viewport height for media scroll tracking
    state.media_viewport_h = (panels[1].height as usize).saturating_sub(2);
    // Clamp scan spinner to avoid overflow issues
    let scan_spin = SPIN[(state.tick / 3) as usize % SPIN.len()];

    render_devices(f, state, panels[0], scan_spin);
    render_media(f, state, panels[1]);
    render_now_playing(f, state, rows[1], scan_spin);
    render_hints(f, state, rows[2]);

    if state.phase == AppPhase::SeekInput {
        render_seek_popup(f, state, area);
    }
}

fn render_devices(f: &mut Frame, state: &AppState, area: Rect, spin: char) {
    let focused = state.focus == FocusPanel::Devices;
    let border_style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let title = match state.phase {
        AppPhase::Scanning => format!(" Devices  {} Scanning... ", spin),
        _ => format!(
            " Devices ({}) ",
            if state.devices.is_empty() {
                "none — press R".to_string()
            } else {
                state.devices.len().to_string()
            }
        ),
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);

    if state.devices.is_empty() {
        let msg = if state.phase == AppPhase::Scanning {
            format!("\n  {} Scanning for devices...", spin)
        } else {
            "\n  No devices found. Press R to rescan.".to_string()
        };
        f.render_widget(
            Paragraph::new(msg)
                .style(Style::default().fg(Color::DarkGray))
                .block(block),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = state
        .devices
        .iter()
        .enumerate()
        .map(|(i, dev)| {
            let is_active = state.active_device == Some(i);
            let is_cursor = i == state.device_cursor && focused;

            let prefix = if is_cursor { "→ " } else { "  " };
            let dot = if is_active { "● " } else { "  " };

            let (fg, bold) = if is_cursor {
                (Color::Yellow, true)
            } else if is_active {
                (Color::Green, false)
            } else {
                (Color::Reset, false)
            };

            let mut style = Style::default().fg(fg);
            if bold {
                style = style.add_modifier(Modifier::BOLD);
            }

            let suffix = if is_active { " [connected]" } else { "" };
            ListItem::new(Line::from(Span::styled(
                format!("{}{}{}{}", prefix, dot, dev.name, suffix),
                style,
            )))
        })
        .collect();

    f.render_widget(List::new(items).block(block), area);
}

fn render_media(f: &mut Frame, state: &AppState, area: Rect) {
    let focused = state.focus == FocusPanel::Media;
    let border_style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let title = format!(
        " Media ({} files, {} selected) ",
        state.media_files.len(),
        state.media_selected.len()
    );

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);

    if state.media_files.is_empty() {
        f.render_widget(
            Paragraph::new("\n  No media files found.")
                .style(Style::default().fg(Color::DarkGray))
                .block(block),
            area,
        );
        return;
    }

    let viewport = state.media_viewport_h.max(1);
    let items: Vec<ListItem> = state
        .media_files
        .iter()
        .enumerate()
        .skip(state.media_scroll)
        .take(viewport)
        .map(|(i, file)| {
            let check = if state.media_selected.contains(&i) {
                "x"
            } else {
                " "
            };
            let cursor = if focused && i == state.media_cursor {
                "→"
            } else {
                " "
            };

            let (fg, bold) = if focused && i == state.media_cursor {
                (Color::Yellow, true)
            } else if state.media_selected.contains(&i) {
                (Color::Green, false)
            } else {
                (Color::Reset, false)
            };

            let mut style = Style::default().fg(fg);
            if bold {
                style = style.add_modifier(Modifier::BOLD);
            }

            ListItem::new(Line::from(Span::styled(
                format!("{} [{}] {}", cursor, check, file.relative_path),
                style,
            )))
        })
        .collect();

    f.render_widget(List::new(items).block(block), area);
}

fn render_now_playing(f: &mut Frame, state: &AppState, area: Rect, spin: char) {
    let block = Block::default()
        .title(" Now Playing ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let line = if let Some(track) = state.current_track() {
        let (icon, color) = match state.transport_state.as_str() {
            "PLAYING" => ("▶", Color::Green),
            "PAUSED_PLAYBACK" => ("⏸", Color::Yellow),
            _ => ("■", Color::DarkGray),
        };
        let track_label = format!(
            "  {} {}  [{}/{}]  {}",
            icon,
            spin,
            state.playlist_pos + 1,
            state.playlist.len(),
            track.relative_path
        );
        Line::from(vec![
            Span::styled(
                track_label,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("   {}", state.transport_state),
                Style::default().fg(Color::DarkGray),
            ),
        ])
    } else {
        Line::from(Span::styled(
            "  — idle —",
            Style::default().fg(Color::DarkGray),
        ))
    };

    f.render_widget(Paragraph::new(line).block(block), area);
}

fn render_hints(f: &mut Frame, state: &AppState, area: Rect) {
    let text = match state.phase {
        AppPhase::Scanning => {
            " R rescan   Tab switch panel   ↑↓/jk nav   Q quit ".to_string()
        }
        AppPhase::Playing | AppPhase::SeekInput => {
            " Tab panels   ↑↓ nav   Space select   P pause   N next   S stop   F seek   Q quit "
                .to_string()
        }
        _ => {
            " Tab panels   ↑↓/jk nav   Space select   A all   Enter confirm/play   R rescan   Q quit ".to_string()
        }
    };

    let (content, style) = if let Some(ref msg) = state.status_msg {
        (format!(" ⚠ {} ", msg), Style::default().fg(Color::Yellow))
    } else {
        (text, Style::default().fg(Color::DarkGray))
    };

    f.render_widget(
        Paragraph::new(content).style(style).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        ),
        area,
    );
}

fn render_seek_popup(f: &mut Frame, state: &AppState, area: Rect) {
    let popup = centered_fixed(42, 5, area);
    let block = Block::default()
        .title(" Seek position ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(popup);
    f.render_widget(Clear, popup);
    f.render_widget(block, popup);
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "  Enter position (HH:MM:SS):",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                format!("  > {}_", state.seek_input),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
        ]),
        inner,
    );
}

fn centered_fixed(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}
