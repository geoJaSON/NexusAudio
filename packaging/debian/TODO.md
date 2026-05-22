# TODO: `.deb` package for NEXUS//AUDIO

**How-to:** see **[README.md](README.md)** (Option A = quick `dpkg-deb`, Option B = full `debian/` tree).

**Status:** not started (release binary built: `target/release/nexus-audio`)

Windows installer exists: `NexusAudio.iss` (Inno Setup). Linux needs an equivalent.

## Checklist

- [ ] Add `debian/` metadata (`control`, `rules`, `changelog`, `copyright`, `install`)
- [ ] Install binary to `/usr/bin/nexus-audio` (or `/usr/games/…`)
- [ ] Ship `icon.ico` / `.desktop` file for the app menu (`Categories=Audio;Player;`)
- [ ] Optional: bundle CRT fonts under `/usr/share/nexus-audio/fonts/` or depend on font packages
- [ ] Runtime deps in `Depends:` — ALSA/PipeWire stack, GTK3/Wayland libs pulled in by the GUI stack (test on a clean VM)
- [ ] `dpkg-buildpackage` or `debuild` smoke test; install with `sudo apt install ./nexus-audio_*.deb`

## Reference binary

```text
cargo build --release
# → target/release/nexus-audio (~22 MB, mostly static Rust + bundled sqlite)
```
