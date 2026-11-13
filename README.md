# GMAudio

An audio control center for running tabletop RPG sessions: music, ambience and
sound effects from your own files, on one screen. Windows 10/11.

Version 0.7.0 (shown at the bottom of Settings, and in `VERSION`).

Status: **all five milestones built.** Builds and runs on Windows 10 and 11.

## What it does

**Library**
- Add folders of MP3, OGG (Vorbis), FLAC, WAV and M4A files. Files stay where they
  are; new files in those folders are picked up automatically.
- Built-in starter sounds (synthesised) for trying things out.
- Every file is measured once (length, loudness, tempo) so all sounds play at
  a similar level.
- Search by name, folder or `#tag`. Typos and skipped letters still match.
- Channel tags (Music, Ambience, Sound effects, Stinger) decide where a sound
  plays. One tag: one click plays it there. Several: one button each.
  Right-click or drag onto a channel to play anywhere.
- Split the library into up to four panes, each with its own search.
- Tag many sounds at once: Ctrl+click, Shift+click or "Select all".
- Find sounds online: search Freesound, listen, download into the library.
  Credits are kept with each sound.
- Add from YouTube: paste a link, the audio is saved as M4A (uses yt-dlp,
  fetched on first use).

**Mixer**
- Four channels with volume, mute, solo, fade time and level meters.
- Music queue: next, repeat one track, repeat the queue, drag to reorder,
  drag onto the playing track to swap.
- Seek bar and pause (at once or with a fade) on longer sounds.
- Drag a playing or queued sound to another channel; drag from the library
  to a place in the queue.
- Per sound: always loop, and a trim (start, end, parts to skip) that
  leaves the file untouched.
- Stingers and chosen pads lower the music while they sound.
- Audio profiles for headphones, laptop speakers, a table speaker, or custom.
- **Esc** fades everything out. Press again to cut instantly.

**Running a session**
- Scenes: a saved mix of music, ambience layers and random one-shots.
- Pads: boards of one-press sounds, keys 1 to 0.
- Dynamic music: intensity levels that change on the beat, bar or phrase,
  from separate tracks or from stems; stingers that lead into a level.
- Campaigns and sessions, with a cue list fired by **Space**.
- Hotkeys for anything, optionally working while another app has focus.
- Find and fire anything by name with **Ctrl+K**.
- Save a campaign (or everything) to one `.gmaudio` file, with or without
  the sound files, and open it on another computer.

Simple mode is the default. Advanced mode (Settings) shows the finer controls.

Not supported: Opus files (the audio decoder has no Opus codec), Spotify and
YouTube.

## Run it

Needs [Rust](https://rustup.rs), [Node.js](https://nodejs.org) 20.19 or newer
(or 22.12 or newer) and the Visual Studio C++ build tools.

```powershell
npm install
npm run tauri dev
```

The first build takes several minutes. Later builds take seconds. If
PowerShell refuses to run `npm`, use `npm.cmd` instead, or run
`Set-ExecutionPolicy -Scope CurrentUser RemoteSigned` once.

To make an installer: `npm run tauri build` (output in
`target\release\bundle\nsis`).

## Test it

```powershell
cargo test -p gmaudio-core
```

The tests cover the database, folder scanning, loudness and tempo analysis,
search (including the 10,000-file speed check), campaign bundles, and the
audio engine (queue, scenes, ducking, dynamic music, profiles, panic) against
a simulated audio device.

## Layout

| Path | What it is |
| --- | --- |
| `crates/gmaudio-core` | Library and audio engine in Rust. No UI code. |
| `src-tauri` | The Tauri shell: passes commands in and events out. |
| `src` | The React interface. |

The engine owns all playback state and runs on its own thread, so sound keeps
playing if the interface stalls. The interface only sends commands and draws
what the engine reports.

Library data lives in `%APPDATA%\com.edenkici.gmaudio\library.db`. Before a
new version changes its layout, a copy is saved next to it as
`library.backup-v<n>.db`. Freesound downloads go to the `Freesound` folder
there; the Freesound key is kept in Windows Credential Manager.
