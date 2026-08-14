#!/usr/bin/env bash

# Voice Keyboard Runner Script
# This script runs the voice-keyboard with proper privilege handling
#
# Usage examples:
#   ./run.sh --test-audio              # Test audio input
#   ./run.sh --test-stt                # Test STT with typing
#   ./run.sh --debug-stt               # Debug STT (print only)
#   ./run.sh --debug-stt --stt-url ... # Debug with custom URL

# Check if we're already running as root
if [ "$EUID" -eq 0 ]; then
  echo "Error: Don't run this script as root. It will handle privileges automatically."
  exit 1
fi

# Build the project first
echo "Building voice-keyboard..."
cargo build

if [ $? -ne 0 ]; then
  echo "Build failed!"
  exit 1
fi

# Run as root long enough to create /dev/uinput, while explicitly passing the
# desktop session variables needed after the program drops back to this user.
# Some sudo policies ignore -E, so do not rely on it here.
echo "Starting voice-keyboard with privilege dropping..."
echo "Note: This will create a virtual keyboard as root, then drop privileges for audio access."
echo ""

sudo /usr/bin/env \
  XDG_RUNTIME_DIR="$XDG_RUNTIME_DIR" \
  DBUS_SESSION_BUS_ADDRESS="$DBUS_SESSION_BUS_ADDRESS" \
  WAYLAND_DISPLAY="$WAYLAND_DISPLAY" \
  DISPLAY="$DISPLAY" \
  PULSE_SERVER="${PULSE_SERVER:-}" \
  PIPEWIRE_REMOTE="${PIPEWIRE_REMOTE:-}" \
  HOME="$HOME" \
  USER="$USER" \
  ./target/debug/voice-keyboard "$@"
