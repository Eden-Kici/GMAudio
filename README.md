# GMAudio

An audio control center for running tabletop RPG sessions: music, ambience and
sound effects from your own files, on one screen. Windows 10/11.

Status: **Milestone 1 of 5** (engine + library).

## What works now

- Add folders of MP3, OGG (Vorbis), FLAC and WAV files. Files stay where they
  are; new files in those folders are picked up automatically.
- Every file is measured once (length and loudness) so all sounds play at a
  similar level.
- Search by name, folder or `#tag`. Typos and skipped letters still match.
- Tag sounds from the row details.
- Four mixer channels, each with volume, mute and a fade time:
  - **Music**: one track at a time; a new track crossfades in. Loops.
  - **Ambience**: layers that stack. Loops.
  - **Sound effects** and **Stingers**: play once and can overlap.
- Master volume.
- **Esc** fades everything out over one second. Press again to cut instantly.
- Mixer settings and the library are remembered between launches.

Not in yet: scenes, soundboard pads, hotkeys, dynamic music, campaigns and cue
lists (Milestones 2 to 5). Opus files are not supported: the audio decoder has
no Opus codec.

## Run it

Needs [Rust](https://rustup.rs), [Node.js](https://nodejs.org) and the Visual
Studio C++ build tools.

```powershell
npm install
npm run tauri dev
```

The first build takes several minutes. Later builds take seconds.

To make an installer: `npm run tauri build` (output in
`target\release\bundle\nsis`).

## Test it

```powershell
cargo test -p gmaudio-core
```

The tests cover the database, folder scanning, loudness analysis, search
(including the 10,000-file speed check) and the audio engine (crossfades,
layers, panic) against a simulated audio device.

## Layout

| Path | What it is |
| --- | --- |
| `crates/gmaudio-core` | Library and audio engine in Rust. No UI code. |
| `src-tauri` | The Tauri shell: passes commands in and events out. |
| `src` | The React interface. |

The engine owns all playback state and runs on its own thread, so sound keeps
playing if the interface stalls. The interface only sends commands and draws
what the engine reports.

Library data lives in `%APPDATA%\com.edenkici.gmaudio\library.db`.
