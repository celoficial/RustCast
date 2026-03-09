# Getting Started

This guide is for users who just want to run RustCast — no programming knowledge required.

## 1. Download

Go to the **[Releases page](https://github.com/celoficial/RustCast/releases/latest)** and download the file for your operating system:

| System | File to download |
|---|---|
| Linux (most PCs) | `rustcast-linux-x86_64` |
| Linux (Raspberry Pi / ARM) | `rustcast-linux-aarch64` |
| macOS (Intel) | `rustcast-macos-x86_64` |
| macOS (Apple Silicon — M1/M2/M3) | `rustcast-macos-aarch64` |
| Windows | `rustcast-windows-x86_64.exe` |

Not sure which macOS you have? Click the  menu → **About This Mac**. If it says "Apple M…" choose Apple Silicon; otherwise choose Intel.

---

## 2. Prepare your media folder

Create a folder called `media` in the same location as the downloaded file and place your videos or music inside it.

```
Downloads/
  rustcast-macos-aarch64   ← the binary you downloaded
  media/
    movie.mp4
    concert.mp4
    movie.srt              ← subtitle (same name as the video)
```

**Supported formats:** mp4, mkv, avi, mp3

**Subtitles:** place a `.srt` file with the exact same name as the video in the same folder — it loads automatically.

---

## 3. Configure

Create a file named `.env` in the same folder as the binary with the following content:

```env
HTTP_PORT=8085
DLNA_FRIENDLY_NAME="RustCast"
MEDIA_DIRECTORY="./media"
```

Change `DLNA_FRIENDLY_NAME` to whatever name you want to appear on your TV's device list.

> **Windows note**: Windows hides files that start with a dot by default. Use Notepad and save the file as `.env` (include the dot, no `.txt` extension).

---

## 4. Run

**Linux / macOS** — open a terminal in the folder and run:

```bash
chmod +x rustcast-*   # make it executable (first time only)
./rustcast-macos-aarch64
```

> **macOS Gatekeeper**: if you see _"cannot be opened because the developer cannot be verified"_, go to **System Settings → Privacy & Security** and click **Open Anyway**.

**Windows** — double-click `rustcast-windows-x86_64.exe`, or open Command Prompt in that folder and run:

```cmd
rustcast-windows-x86_64.exe
```

---

## 5. Cast to your TV

Once running, you'll see a list of devices found on your network:

```
MediaRenderer devices found:
1) Living Room TV  (http://192.168.1.100:52235/...)
2) Bedroom Speaker (http://192.168.1.101:8200/...)

Choose a device by number (or '0' to exit):
```

Type the number of your TV and press Enter. Then choose which files to play:

```
Media files found:
1) movie.mp4
2) concert.mp4

Select media (e.g. 1  1,3  2-4  all) or '0' to exit:
```

You can pick one file (`1`), several (`1,3`), a range (`2-4`), or everything (`all`).

---

## 6. Playback controls

Once streaming starts, use these keys:

| Key | Action |
|---|---|
| `p` | Pause / Resume |
| `s` | Stop |
| `f` | Seek to a time (e.g. `0:01:30`) |
| `n` | Skip to next file in the playlist |
| `q` | Quit |

---

## Troubleshooting

**No devices found** — make sure your computer and TV are on the same Wi-Fi network. Some routers block multicast traffic; try connecting both to the same network band (2.4 GHz or 5 GHz).

**TV shows an error playing the file** — check that the format is supported by your TV. MP4 (H.264) works on virtually all DLNA devices.

**Subtitles not showing** — confirm the `.srt` filename matches the video exactly (e.g. `movie.srt` for `movie.mp4`) and that your TV's renderer supports external subtitles over DLNA.
