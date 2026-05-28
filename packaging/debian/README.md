# How to build a `.deb` for NEXUS//AUDIO

You already have: `cargo build --release` → `target/release/nexus-audio`.

Two approaches: **quick one-off** (manual `dpkg-deb`) or **maintainable** (`debian/` + `dpkg-buildpackage`). Use the quick path to try it; use `debian/` if you will ship updates.

---

## Option A — Quick `.deb` (no `debian/` tree)

Good for installing on your own machine or sharing a single file.

### 1. Install tools

```bash
sudo apt-get install -y dpkg-dev
```

### 2. Stage the package layout

From the repo root (`NexusAudio/`):

```bash
VERSION=2.5.0
ARCH=amd64   # or: dpkg --print-architecture

ROOT=packaging/debian/stage
rm -rf "$ROOT"
mkdir -p "$ROOT/DEBIAN"
mkdir -p "$ROOT/usr/bin"
mkdir -p "$ROOT/usr/share/applications"
mkdir -p "$ROOT/usr/share/icons/hicolor/256x256/apps"
mkdir -p "$ROOT/usr/share/nexus-audio/fonts"

# Binary (already built)
install -m 755 target/release/nexus-audio "$ROOT/usr/bin/nexus-audio"

# Optional CRT fonts (skip if you don't have the TTFs yet)
# install -m 644 assets/fonts/*.ttf "$ROOT/usr/share/nexus-audio/fonts/" 2>/dev/null || true

# Menu entry
cat > "$ROOT/usr/share/applications/nexus-audio.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=NEXUS//AUDIO
Comment=Retro terminal music and audiobook player
Exec=nexus-audio
Icon=nexus-audio
Terminal=false
Categories=Audio;Player;
StartupWMClass=nexus-audio
EOF

# Icon: convert icon.ico → PNG once (needs ImageMagick)
# convert -background none icon.ico[0] "$ROOT/usr/share/icons/hicolor/256x256/apps/nexus-audio.png"
# Or copy any PNG you already have:
# install -m 644 path/to/nexus-audio.png "$ROOT/usr/share/icons/hicolor/256x256/apps/nexus-audio.png"
```

### 3. Write `DEBIAN/control`

```bash
cat > "$ROOT/DEBIAN/control" <<EOF
Package: nexus-audio
Version: ${VERSION}
Section: sound
Priority: optional
Architecture: ${ARCH}
Maintainer: You <you@example.com>
Description: NEXUS//AUDIO — retro terminal music and audiobook player
Depends: libasound2, libc6
EOF
```

Your release binary currently links **`libasound2`** dynamically (`ldd target/release/nexus-audio`). After you add a `.desktop` icon, you may want `Recommends:` for fonts. Test on a minimal VM and add packages if the app fails to start (GUI stacks sometimes pull extra libs via the display server).

### 4. Build and install the `.deb`

```bash
dpkg-deb --build "$ROOT" "packaging/debian/nexus-audio_${VERSION}_${ARCH}.deb"

sudo apt install ./packaging/debian/nexus-audio_${VERSION}_${ARCH}.deb
# or: sudo dpkg -i ./packaging/debian/nexus-audio_*.deb
```

Launch from the app menu or run `nexus-audio`.

Uninstall: `sudo apt remove nexus-audio`

---

## Option B — Proper `debian/` source package

Better if you will rebuild often or publish to a PPA.

### 1. Install build helpers

```bash
sudo apt-get install -y debhelper devscripts build-essential
```

### 2. Create `debian/` at repo root

Minimum files:

| File | Purpose |
|------|---------|
| `debian/control` | Package metadata + `Depends` |
| `debian/rules` | Copy binary/assets into `debian/nexus-audio/` |
| `debian/changelog` | Version history for `dpkg-buildpackage` |
| `debian/copyright` | MIT license |
| `debian/install` | Optional: paths for `dh_install` |
| `debian/nexus-audio.desktop` | Freedesktop launcher |

Example **`debian/control`**:

```
Source: nexus-audio
Section: sound
Priority: optional
Maintainer: You <you@example.com>
Build-Depends: debhelper (>= 13)
Standards-Version: 4.6.2
Rules-Requires-Root: no

Package: nexus-audio
Architecture: amd64
Depends: ${shlibs:Depends}, ${misc:Depends}
Description: NEXUS//AUDIO — retro terminal music and audiobook player
 Retro terminal music & audiobook player (Linux-first).
```

Example **`debian/rules`** (binary-only — expects `cargo build --release` first):

```makefile
#!/usr/bin/make -f
%:
	dh $@

override_dh_auto_build:
	# Build Rust binary before packaging:
	cargo build --release

override_dh_auto_install:
	install -D -m 755 target/release/nexus-audio debian/nexus-audio/usr/bin/nexus-audio
	install -D -m 644 debian/nexus-audio.desktop debian/nexus-audio/usr/share/applications/nexus-audio.desktop
	# install icon, fonts similarly
```

Example **`debian/changelog`** (first entry):

```
nexus-audio (2.5.0-1) unstable; urgency=medium

  * Initial Debian package.

 -- You <you@example.com>  Tue, 20 May 2026 12:00:00 +0000
```

### 3. Build the `.deb`

```bash
# From repo root, after cargo build --release
dpkg-buildpackage -b -us -uc
```

Output (parent directory): `../nexus-audio_2.5.0-1_amd64.deb`

Install:

```bash
sudo apt install ../nexus-audio_2.5.0-1_amd64.deb
```

`dh_shlibdeps` (via debhelper) will scan the binary and fill in `Depends` automatically — more accurate than hand-writing after GUI/link changes.

---

## Icon note

`icon.ico` is Windows-oriented. For the `.desktop` `Icon=` field on Linux, install a **PNG** under:

`usr/share/icons/hicolor/256x256/apps/nexus-audio.png`

One-liner if ImageMagick is installed:

```bash
convert -background none icon.ico[0] nexus-audio.png
```

---

## Version bumps

Keep these in sync with `Cargo.toml` (`version = "2.5.0"`):

- `DEBIAN/control` `Version:` (Option A)
- `debian/changelog` (Option B)
- Output `.deb` filename

Debian convention: upstream `2.5.0` + package revision `2.5.0-1`, `2.5.0-2`, …

Or from the repo root:

```bash
./scripts/build-deb.sh
```

---

## Sanity checks

```bash
# What libraries does the binary need?
ldd target/release/nexus-audio

# Inspect the package without installing
dpkg-deb -c packaging/debian/nexus-audio_*.deb
dpkg-deb -I packaging/debian/nexus-audio_*.deb

# Test install in a VM or container
sudo apt install ./nexus-audio_*.deb
nexus-audio
```

---

## Related

- Windows installer: `NexusAudio.iss` (Inno Setup, same `target/release` binary)
- Checklist: `TODO.md`
