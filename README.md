# RustCast

[![License](https://img.shields.io/badge/license-Apache%202.0-blue)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-2021-orange)](https://www.rust-lang.org/)
[![Contributions](https://img.shields.io/badge/contributions-welcome-brightgreen)](CONTRIBUTING.md)

RustCast is a lightweight **DLNA (Digital Living Network Alliance) server**, written in **Rust**, that allows you to stream media from your computer to compatible devices, such as Smart TVs, speakers, and other devices.

## Features

- **Device discovery** via **SSDP (Simple Service Discovery Protocol)**.
- **Media streaming** to DLNA/UPnP-compatible devices.
- Integration with Samsung TVs, LG TVs, and other devices.
- Open-source code under the **Apache 2.0 license**.

## Demo

### Example Output

After running the software, you will see something like this in the terminal:

```bash
Starting the DLNA Server: RustCast
Starting HTTP server on port 8080
UDP Socket created at: Ok(192.168.0.97:50980)
Sending SSDP request to 239.255.255.250:1900...
Discovered MediaRenderer devices:
1) [TV] Samsung Q80 - http://192.168.0.109:9197/dmr
Select a device by its number (or type '0' to exit):
```

After selecting a device, the software connects and streams media!

## How It Works

RustCast uses the following protocols and technologies:

1. **SSDP** for discovering devices on the local network.
2. **HTTP** for communication and control with DLNA devices.
3. **Rust** for security, speed, and reliability.
4. **UPnP (Universal Plug and Play)** for device control and integration.

## Project Structure

```plaintext
dlna-server/
├── src/
│   ├── config/         # Configuration (environment variable parsing and structs)
│   ├── discovery/      # SSDP and UPnP logic for device discovery
│   ├── media/          # Media management (videos, subtitles, transcoding)
│   ├── server/         # HTTP server for XML responses and media streaming
│   ├── utils/          # Generic utilities (logging, etc.)
│   ├── main.rs         # Application entry point
├── .env                # Environment variables
├── .env.example        # Environment variable template
├── Cargo.toml          # Dependencies and metadata
├── LICENSE             # Apache 2.0 license file
```

## Requirements

- **Rust** (version 1.70 or later)
- Operating system supporting multicast networking (Linux, macOS, Windows)
- DLNA/UPnP-compatible devices

## Installation

### 1. Clone the repository

```bash
git clone https://github.com/your-username/RustCast.git
cd RustCast
```

### 2. Configure the environment

Rename the `.env.example` file to `.env` and configure the variables:

```env
HTTP_PORT=8080
DLNA_FRIENDLY_NAME=RustCast
MULTICAST_ADDRESS=239.255.255.250
MULTICAST_PORT=1900
```

### 3. Build the project

```bash
cargo build --release
```

### 4. Run the project

```bash
cargo run --release
```

## Contribution

Contributions are welcome! Please read the [CONTRIBUTING.md](CONTRIBUTING.md) file for detailed information on how to contribute.

### Examples of Contributions

- Fixing bugs
- Adding new features
- Improving documentation
- Writing tests and benchmarks

## Roadmap

- [x] Device discovery via SSDP
- [x] Fetching device XML description
- [ ] Media streaming with subtitle support
- [ ] Graphical user interface for end users
- [ ] Integration with streaming services (optional)

## License

This project is licensed under the terms of the [Apache License 2.0](LICENSE). Feel free to use, modify, and distribute the software as permitted by the license.

---

Developed with ❤️ and **Rust**.
