#!/usr/bin/env bash
set -e

echo "=== Nexus Audio Linux Desktop Installer ==="

# 1. Ensure local bin, icons, and applications directories exist
mkdir -p "$HOME/.local/bin"
mkdir -p "$HOME/.local/share/icons"
mkdir -p "$HOME/.local/share/applications"

# 2. Build the app in release mode
echo "Building Nexus Audio in release mode..."
cargo build --release

# 3. Copy binary
echo "Installing binary to ~/.local/bin/nexus-audio..."
cp target/release/nexus-audio "$HOME/.local/bin/nexus-audio"
chmod +x "$HOME/.local/bin/nexus-audio"

# 4. Extract and copy icon
echo "Extracting icon asset..."
python3 scripts/extract_icon.py
cp icon.png "$HOME/.local/share/icons/nexus-audio.png"

# 5. Create desktop launcher entry
echo "Creating desktop launcher shortcut..."
cat <<EOF > "$HOME/.local/share/applications/nexus-audio.desktop"
[Desktop Entry]
Version=1.0
Type=Application
Name=Nexus Audio
Comment=Retro-Inspired Audio and Audiobook Player
Exec=$HOME/.local/bin/nexus-audio
Icon=$HOME/.local/share/icons/nexus-audio.png
Terminal=false
Categories=AudioVideo;Audio;Player;
StartupWMClass=nexus_audio
EOF

# Make launcher executable
chmod +x "$HOME/.local/share/applications/nexus-audio.desktop"

# Refresh desktop database if tool is present
if command -v update-desktop-database &> /dev/null; then
    update-desktop-database "$HOME/.local/share/applications"
fi

echo "=== Installation Complete! ==="
echo "Nexus Audio is now fully integrated with your desktop environment."
echo "You can search for 'Nexus Audio' in your system menu, and it will show up with the proper icon!"
