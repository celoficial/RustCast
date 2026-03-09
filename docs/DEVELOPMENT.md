# Development Guide

## Requirements

- **Rust** 1.70 or later — [install via rustup](https://rustup.rs/)
- A DLNA/UPnP-compatible renderer on the same local network for manual testing

## Project structure

```
dlna-server/
├── src/
│   ├── config/         # Environment variable parsing
│   ├── discovery/      # SSDP discovery + NOTIFY advertiser
│   ├── media/          # Media listing, streaming, subtitle detection
│   ├── server/         # HTTP server, endpoints, range request handling
│   └── main.rs         # Entry point: device selection, playlist, control loop
├── .env                # Local config (not committed)
├── .env.example        # Config template
└── Cargo.toml
```

## Setup

```bash
git clone https://github.com/celoficial/RustCast.git
cd RustCast/dlna-server
cp .env.example .env
mkdir media   # add some test files here
cargo run
```

## Configuration reference

| Variable | Default | Description |
|---|---|---|
| `HTTP_PORT` | `8080` | Port the HTTP media server listens on |
| `DLNA_FRIENDLY_NAME` | `Rust DLNA Server` | Name shown in device lists |
| `MEDIA_DIRECTORY` | `./media` | Path to the folder with media files |
| `MULTICAST_ADDRESS` | `239.255.255.250` | SSDP multicast address — do not change |
| `MULTICAST_PORT` | `1900` | SSDP multicast port — do not change |
| `UDN` | _(auto-generated)_ | Fix the device UUID to survive restarts |

The server's LAN IP is detected automatically from the outbound network interface.

## Running checks locally

```bash
cargo fmt --check          # formatting
cargo clippy -- -D warnings  # linting
cargo test                   # tests
cargo audit                  # dependency CVE scan (requires cargo-audit)
```

## Architecture notes

**SSDP flow**
- On startup, `start_ssdp_advertiser` sends `ssdp:alive` NOTIFY to `239.255.255.250:1900`, then repeats every 30 seconds so renderers on the LAN can discover RustCast as a MediaServer
- `discover_ssdp` sends an M-SEARCH and collects `MediaRenderer:1` responses
- Device descriptions are fetched in parallel via `join_all` to display friendly names

**Streaming flow**
- `SetAVTransportURI` sends the media URL + DIDL-Lite metadata (including subtitle `<res>` if a `.srt` is found)
- The HTTP server handles `Range` requests with 206 Partial Content so renderers can seek
- A background task polls `GetTransportInfo` every 3 seconds; when the state becomes `STOPPED`, it signals the main loop to advance the playlist

**Stdin**
- A dedicated task reads stdin line by line and forwards to an `mpsc` channel. This avoids dropped-future issues when `tokio::select!` races between user input and the transport state watcher.

## CI / CD

### CI (`.github/workflows/ci.yml`)

Runs on every push to `main` and on pull requests:

| Job | What it checks |
|---|---|
| `fmt` | `cargo fmt --check` |
| `clippy` | `cargo clippy -- -D warnings` |
| `test` | `cargo test` on ubuntu, macos, windows |
| `audit` | `cargo audit` for known CVEs |

### Release (`.github/workflows/release.yml`)

Triggered by pushing a version tag:

```bash
git tag v1.0.0
git push origin v1.0.0
```

Builds binaries for 5 targets (linux x86\_64, linux aarch64 via `cross`, macOS Intel, macOS Apple Silicon, Windows) and publishes them to the GitHub Releases page automatically.

## Contributing

1. Fork the repo and create a branch: `git checkout -b feat/my-feature`
2. Make your changes, add tests where applicable
3. Run `cargo fmt`, `cargo clippy`, `cargo test` — all must pass
4. Open a pull request against `main`

See [CONTRIBUTING.md](../.github/CONTRIBUTING.md) for more detail.
