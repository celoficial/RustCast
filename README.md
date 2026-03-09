# RustCast

[![License](https://img.shields.io/badge/license-Apache%202.0-blue)](https://www.apache.org/licenses/LICENSE-2.0)
[![Rust](https://img.shields.io/badge/Rust-2021-orange)](https://www.rust-lang.org/)
[![Contributions](https://img.shields.io/badge/contributions-welcome-brightgreen)](.github/CONTRIBUTING.md)

RustCast is a lightweight **DLNA media server** written in **Rust**. It streams local media files to compatible devices on your network — Smart TVs, speakers, and any UPnP/DLNA renderer.

## Roadmap

- [x] Device discovery via SSDP
- [x] Friendly device names on discovery
- [x] Media streaming (mp4, mkv, avi, mp3)
- [x] Playlist / multi-file queue
- [x] Playback controls (pause, resume, stop, seek, skip)
- [x] Subtitle support (.srt, auto-detected)
- [x] Range request support (seek-friendly streaming)
- [ ] Graphical user interface

## Features

- **Automatic device discovery** via SSDP — lists devices by friendly name
- **Playlist support** — select multiple files at once (`1`, `1,3`, `2-4`, `all`)
- **Playback controls** — pause, resume, stop, seek, skip to next, quit
- **Auto-advance** — polls transport state and moves to the next file when playback ends
- **Subtitle auto-detection** — place a `.srt` file alongside the video with the same name and it loads automatically
- **Range requests** — proper 206 Partial Content for seek-compatible streaming
- **Auto IP detection** — no need to configure the server's IP address manually

## Requirements

- **Rust** 1.70 or later
- A DLNA/UPnP-compatible renderer on the same local network

## Installation

### 1. Clone the repository

```bash
git clone https://github.com/celoficial/RustCast.git
cd RustCast/dlna-server
```

### 2. Configure the environment

Copy `.env.example` to `.env` and adjust as needed:

```bash
cp .env.example .env
```

```env
HTTP_PORT=8085
DLNA_FRIENDLY_NAME="RustCast"
MEDIA_DIRECTORY="./media"
```

The server's LAN IP is detected automatically — no need to set it manually.

`MULTICAST_ADDRESS` and `MULTICAST_PORT` are standard UPnP values and should not be changed.

### 3. Add media files

Place your media files in the configured `MEDIA_DIRECTORY`. Supported formats: `mp4`, `mkv`, `avi`, `mp3`.

For subtitles, place a `.srt` file in the same directory with the same base name as the video:

```
media/
  movie.mp4
  movie.srt   ← loaded automatically
```

### 4. Build and run

```bash
cargo run --release
```

## Usage

```
Starting the RustCast server
Starting HTTP server on port 8085

MediaRenderer devices found:
1) Living Room TV  (http://192.168.1.100:52235/description.xml)
2) Bedroom Speaker (http://192.168.1.101:8200/description.xml)

Choose a device by number (or '0' to exit):
> 1

Connected to: Living Room TV

Media files found:
1) movie.mp4
2) documentary.mkv
3) concert.mp4

Select media (e.g. 1  1,3  2-4  all) or '0' to exit:
> 1,3

Playlist (2 file(s)):
  1) movie.mp4
  2) concert.mp4

[1/2] Starting: movie.mp4
Subtitle found: http://192.168.1.50:8085/media/movie.srt
Streaming started!
Controls: [p] Pause/Resume  [s] Stop  [f] Seek  [n] Next  [q] Quit
```

## How It Works

1. **SSDP** — broadcasts an M-SEARCH multicast to discover DLNA renderers on the LAN
2. **Device description** — fetches each device's XML description in parallel to show friendly names
3. **HTTP server** — serves media files with Range support so renderers can seek
4. **SOAP/UPnP** — sends `SetAVTransportURI` + `Play` to the renderer; subtitles are embedded in the DIDL-Lite metadata
5. **Transport polling** — queries `GetTransportInfo` every 3 seconds to detect when a file ends and auto-advance the playlist

## Contribution

Contributions are welcome! See [CONTRIBUTING.md](.github/CONTRIBUTING.md) for details.

- Fixing bugs
- Adding new features
- Improving documentation
- Writing tests and benchmarks

## License

Licensed under the [Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0).

---

Developed with ❤️ and **Rust**.
