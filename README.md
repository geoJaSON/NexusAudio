# NEXUS//AUDIO

Retro terminal music & audiobook player (Linux-first). Rust + egui.

## Quick start (Linux)

### 1. System dependencies

```bash
sudo apt-get update
sudo apt-get install -y \
  pkg-config \
  libasound2-dev \
  libssl-dev \
  libxkbcommon-dev \
  libwayland-dev \
  libxcb-render0-dev \
  libxcb-shape0-dev \
  libxcb-xfixes0-dev \
  libgtk-3-dev
```

### 2. Rust toolchain

If `cargo` is not on your PATH:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
```

### 3. Build and run

```bash
cd /path/to/NexusAudio
cargo run
```

Release build (faster playback UI):

```bash
cargo run --release
```

### 4. Optional CRT fonts

For the full phosphor-terminal look, add these to `assets/fonts/` (exact filenames):

- `ShareTechMono-Regular.ttf` — [Share Tech Mono](https://fonts.google.com/specimen/Share+Tech+Mono)
- `VT323-Regular.ttf` — [VT323](https://fonts.google.com/specimen/VT323)

The app runs without them (egui monospace fallback).

## First use

1. Launch the app (`cargo run`).
2. Open **Folders** in the sidebar and add a music or audiobook directory.
3. Wait for the background scan to finish, then browse **Tracks**, **Albums**, or **Audiobooks**.

Library data is stored under your OS data directory (via the `dirs` crate), not in the repo.

## Developer smoke tests

```bash
# Scan a folder (no GUI)
cargo run -- --scan-smoke /path/to/music

# Audiobook scan
cargo run -- --ab-smoke /path/to/audiobooks

# Playback engine (12 × 500ms status lines)
cargo run -- --play-smoke /path/to/file.m4b 3600

# Phase 1 seek spike
cargo run --bin spike -- /path/to/file.m4b 15120
```

## Requirements

- Linux with ALSA (or PulseAudio/PipeWire via ALSA compatibility)
- A Wayland or X11 desktop session (eframe/winit)
