# Voice Keyboard

Voice keyboard is a demo application showcasing Deepgram's new turn-taking speech-to-text API: **Flux**.

A voice-controlled Linux virtual keyboard that converts speech to text and types it into any application.

As a result of directly targeting Linux as a driver, this works with all Linux applications.

## Features

- **Voice-to-Text**: Real-time speech recognition using Deepgram's **Flux** API service (turn-taking STT)
- **Virtual Keyboard**: Creates a virtual input device that works with all applications
- **Incremental Typing**: Smart transcript updates with minimal backspacing for real-time corrections
- **Toggle Control**: Enable/disable listening with Super+X keyboard shortcut or system tray icon
- **System Tray Icon**: Visual indicator showing active (green) or inactive (red) state

## Architecture

The application solves a common Linux privilege problem:
- **Virtual keyboard creation** requires root access to `/dev/uinput`
- **Audio input** requires user-space access to PipeWire/PulseAudio

**Solution**: The application starts with root privileges, creates the virtual keyboard, then drops privileges to access the user's audio session.

## Installation

### Prerequisites

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://rustup.rs | sh

# Install required system packages (Fedora/RHEL)
sudo dnf install alsa-lib-devel gtk3-devel libxdo-devel

# Install required system packages (Ubuntu/Debian)
sudo apt install libasound2-dev libgtk-3-dev libxdo-dev pkg-config
```

### Build

```bash
git clone <repository-url>
cd voice-keyboard
cargo build
```

### Acquire a Deepgram API key

You’ll need a Deepgram API key to authenticate with Flux.

- Create or manage keys in the Deepgram console: [Create additional API keys](https://developers.deepgram.com/docs/create-additional-api-keys)
- Export the key so the app can pick it up (recommended):
  ```bash
  export DEEPGRAM_API_KEY="dg_your_api_key_here"
  ```
- The client sends the header `Authorization: Token <DEEPGRAM_API_KEY>`.
- For CI or systemd services, set `DEEPGRAM_API_KEY` in the environment for the service user.
- Security tip: treat API keys like passwords. Prefer env vars over committing keys to files.

## Usage

### Easy Method (Recommended)

Use the provided runner script:

```bash
./run.sh
```

### Manual Method

```bash
# Build and run with proper privilege handling
cargo build
sudo -E ./target/debug/voice-keyboard --test-stt
```

**Important**: Always use `sudo -E` to preserve environment variables needed for audio access.

### Toggle Control

The application starts in **inactive** mode (not listening). You can toggle listening on/off using:

- **Keyboard Shortcut**: Press `Super+X` (Windows/Command key + X)
- **System Tray Icon**: 
  - Click the tray icon menu and select "Toggle STT"
  - Green icon = actively listening
  - Red icon = inactive (not listening)
  - Right-click the icon to access the menu

When inactive, audio recording is completely stopped to conserve system resources.

## Speech-to-Text Service

This application uses **Deepgram Flux**, the company's new turn‑taking STT API. The default WebSocket URL is `wss://api.deepgram.com/v2/listen`.

## Command Line Options

```bash
voice-keyboard [OPTIONS]

OPTIONS:
    --test-audio        Test audio input and show levels
    --test-stt          Test speech-to-text functionality (default if no other mode specified)
    --debug-stt         Debug speech-to-text (print transcripts without typing)
    --stt-url <URL>     Custom STT service URL (default: wss://api.deepgram.com/v2/listen)
    -h, --help          Print help information
    -V, --version       Print version information
```

**Note**: If no mode is specified, the application defaults to `--test-stt` behavior.

## How It Works

1. **Initialization**: Application starts with root privileges
2. **Virtual Keyboard**: Creates `/dev/uinput` device as root
3. **Privilege Drop**: Drops to original user privileges
4. **Audio Access**: Accesses PipeWire/PulseAudio in user space
5. **Speech Recognition**: Streams audio to **Deepgram Flux** STT service
6. **Incremental Typing**: Updates text in real-time with smart backspacing
7. **Turn Finalization**: Clears tracking on "EndOfTurn" events (user presses Enter manually)

### Transcript Handling

The application provides sophisticated real-time transcript updates:

- **Incremental Updates**: As speech is recognized, the application updates the typed text by finding the common prefix between the current and new transcript, backspacing only the changed portion, and typing the new ending
- **Smart Backspacing**: Minimizes cursor movement by only removing characters that actually changed
- **Turn Management**: On "EndOfTurn" events, the application clears its internal tracking but doesn't automatically press Enter, allowing users to review before submitting

## About Deepgram Flux (Early Access)

- **Endpoint**: `wss://api.deepgram.com/v2/listen`
- **What it is**: Flux is Deepgram's turn‑taking, low‑latency STT API designed for conversational experiences.
- **Authentication**: Send an `Authorization` header. Common forms:
  - `Token <DEEPGRAM_API_KEY>` (what this app uses)
  - `token <DEEPGRAM_API_KEY>` or `Bearer <JWT>` are also accepted by the platform
- **Message types** (each server message includes a JSON `type` field):
  - `Connected` — initial connection confirmation
  - `TurnInfo` — streaming transcription updates with fields: `event` (`Update`, `StartOfTurn`, `Preflight`, `SpeechResumed`, `EndOfTurn`), `turn_index`, `audio_window_start`, `audio_window_end`, `transcript`, `words[] { word, confidence }`, `end_of_turn_confidence`
  - `Error` — fatal error with fields: `code`, `description` (may also include a close code)
  - `Configuration` — echoes/acknowledges configuration (e.g., thresholds) when provided
- **Client close protocol**: After sending your final audio, send a control message:
  - `{ "type": "CloseStream" }`
  The server will flush any remaining responses and then close the WebSocket.
- **Update cadence**: Flux produces updates about every **240 ms** with a typical worst‑case latency of ~**500 ms**.
- **Common query parameters** (as supported by the preview spec):
  - `model`, `encoding`, `sample_rate`, `preflight_threshold`, `eot_threshold`, `eot_timeout_ms`, `keyterm`, `mip_opt_out`, `tag`

## Security

- **Minimal Root Time**: Only root during virtual keyboard creation
- **Environment Preservation**: Maintains user's audio session access
- **Clean Privilege Drop**: Properly drops both user and group privileges
- **No System Changes**: No permanent system configuration required

## Troubleshooting

### Audio Issues

If you get "Host is down" or "I/O error" when testing audio:

1. **Use `sudo -E`**: Always preserve environment variables
2. **Check PipeWire**: Ensure PipeWire is running: `systemctl --user status pipewire`
3. **Test without sudo**: Try `./target/debug/voice-keyboard --test-audio` (will fail on keyboard creation but audio should work)

### Permission Issues

If you get "Permission denied" for `/dev/uinput`:

1. **Check uinput module**: `sudo modprobe uinput`
2. **Verify device exists**: `ls -la /dev/uinput`
3. **Use sudo**: The application is designed to run with `sudo -E`

### Global Hotkey Issues (Wayland)

If the Super+X hotkey doesn't work on Wayland:

1. **Compositor Support**: Global hotkeys on Wayland have limited support. Not all compositors support the libei protocol used by the `global-hotkey` crate.
2. **Workaround**: Configure the hotkey in your compositor's settings to send a signal or run a command to toggle the application.
3. **Alternative**: Use the system tray icon to toggle listening instead.
4. **X11**: Global hotkeys work reliably on X11-based systems.

## Development

### Project Structure

```
src/
├── main.rs              # Main application and privilege dropping
├── virtual_keyboard.rs  # Virtual keyboard device management
├── audio_input.rs       # Audio capture and processing
├── stt_client.rs        # WebSocket STT client
├── tray_icon.rs         # System tray icon management
└── input_event.rs       # Linux input event constants
```

### Key Components

- **OriginalUser**: Captures and restores user context
- **VirtualKeyboard**: Manages uinput device lifecycle with smart transcript updates
- **AudioInput**: Cross-platform audio capture
- **SttClient**: WebSocket-based speech-to-text client
- **AudioBuffer**: Manages audio chunking for STT streaming

## License

ISC License. See LICENSE.txt

