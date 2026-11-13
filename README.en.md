# Rewind

**Instant clips for Windows.** Rewind records your screen continuously and keeps the last seconds
when you press a hotkey. In Counter-Strike 2, it clips your best rounds on its own.

[Download for Windows](https://github.com/depre99ed/Rewind/releases/latest) · Windows 11 and recent Windows 10, 64-bit · English and French · [Version française](README.md)

![The Rewind library: latest clip featured, clips grouped by day](docs/bibliotheque.png)

## What Rewind does

**Record without thinking about it**
- One hotkey keeps the last 30 seconds, another the last 2 minutes. Both lengths are adjustable.
- Custom clip: press once to start, once to stop.
- Full session: the whole match in a single file, with kills marked on the timeline.
- Separate audio tracks: system sound, microphone, Discord. Any application can get its own track.

**Counter-Strike 2**
- Automatic clips through Valve's official API (Game State Integration). No injection, no reading of game memory.
- One clip per round, triggered by what you choose: kill count, one tap, headshot, knife, Zeus, collateral, ace.
- Two profiles, Legit and HvH, each with its own settings.

![Counter-Strike 2 automatic clip settings](docs/counter-strike-2.png)

**Edit and share**
- Studio: timeline with thumbnails and waveform, trimming, transitions, background music, per-track volume.
- Ready-made formats: 16:9, 9:16 with blurred background or cropped, 10 or 20 MB files for Discord.
- "Copy for Discord" compresses the clip to the right size and puts it on the clipboard, ready to paste.

![The Studio: timeline, preview and project settings](docs/studio.png)

The screenshots show the French interface. The language can be changed in Settings.

## Install

1. Download `Rewind_<version>_x64-setup.exe` from the [latest release](https://github.com/depre99ed/Rewind/releases/latest).
2. Run it. Rewind installs for your Windows account only and does not need administrator rights.
3. Windows may show "Windows protected your PC", because the installer does not have a publisher certificate yet.
   Click **More info**, then **Run anyway**.

Rewind then updates itself. It checks for a new version at startup and every 4 hours. It installs only when you
click "Install and restart". Every update is signed: a version that is not signed with the project key is rejected.

## Default hotkeys

All of them can be changed in Settings → Hotkeys.

| Hotkey | Action |
|---|---|
| `Alt+F10` | Keep the last 30 seconds |
| `Alt+Shift+F10` | Keep the last 2 minutes |
| `Alt+F11` | Custom clip: start, then stop |
| `Alt+F7` | Full session: start, then stop |
| `Alt+F8` | Add a marker to the running session |
| `Alt+F12` | Screenshot |
| `Alt+F9` | Pause or resume recording |

## Requirements

- **Graphics card**: NVIDIA (NVENC), AMD (AMF) or Intel (Quick Sync). Without a hardware encoder, Rewind falls back to 720p at 30 fps on the CPU.
- **Defaults**: 1080p at 60 fps. On a 100 Hz screen, the frame rate is locked to 50 fps to keep frames evenly spaced.
- **Capture**: Windows Graphics Capture, the same technique as the Windows Snipping Tool. Nothing is injected into games, so anti-cheat systems are not affected.

## Privacy

Your clips stay on your PC, in `Videos\Rewind\<game>\`. Settings and the log are in `%LOCALAPPDATA%\com.rewind.app\`.
Rewind has no account and no usage statistics. It only goes online in two cases:
- to check for updates on this GitHub page;
- to send a clip to a Discord webhook, if you set one up.

## Build from source

The application lives in [`rewind/`](rewind/): engine in Rust (Tauri 2), interface in Svelte 5, recording through FFmpeg.
Build steps, tests and architecture are described in [`rewind/README.md`](rewind/README.md) (in French).

## License

FFmpeg ships with Rewind under its own license ([`rewind/src-tauri/bin/FFMPEG-LICENSE.txt`](rewind/src-tauri/bin/FFMPEG-LICENSE.txt)).
The license for Rewind's own code has not been chosen yet.
