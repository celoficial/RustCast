# RustCast

[![CI](https://github.com/celoficial/RustCast/actions/workflows/ci.yml/badge.svg)](https://github.com/celoficial/RustCast/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue)](https://www.apache.org/licenses/LICENSE-2.0)
[![Rust](https://img.shields.io/badge/Rust-2024-orange)](https://www.rust-lang.org/)
[![Contributions](https://img.shields.io/badge/contributions-welcome-brightgreen)](.github/CONTRIBUTING.md)

RustCast is a lightweight **DLNA media server** written in **Rust**. Run it on your computer and stream local media files to any compatible device on your network — Smart TVs, speakers, and UPnP/DLNA renderers.

## Quick links

| I want to…                     | Go to                                                              |
| ------------------------------ | ------------------------------------------------------------------ |
| Download and run RustCast      | [Getting Started](docs/GETTING_STARTED.md)                         |
| Build from source / contribute | [Development Guide](docs/DEVELOPMENT.md)                           |
| Latest release + binaries      | [Releases](https://github.com/celoficial/RustCast/releases/latest) |

## Features

- **Automatic device discovery** — lists renderers by friendly name
- **LAN advertisement** via SSDP NOTIFY — your TV sees RustCast without manual setup
- **Playlist support** — select multiple files at once (`1`, `1,3`, `2-4`, `all`)
- **Playback controls** — pause, resume, stop, seek, skip, auto-advance
- **Subtitle auto-detection** — place a `.srt` alongside the video, same name
- **Range requests** — seek-friendly 206 Partial Content streaming
- **Auto IP detection** — no network configuration required

**Supported formats:** mp4, mkv, avi, mp3

## Roadmap

- [x] Device discovery via SSDP
- [x] Friendly device names on discovery
- [x] SSDP NOTIFY — announces itself on the LAN
- [x] Media streaming (mp4, mkv, avi, mp3)
- [x] Playlist / multi-file queue
- [x] Playback controls (pause, resume, stop, seek, skip)
- [x] Subtitle support (.srt, auto-detected)
- [x] Range request support
- [x] Auto IP detection
- [ ] Graphical user interface

## License

Licensed under the [Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0).

---

Developed with ❤️ and **Rust**.
