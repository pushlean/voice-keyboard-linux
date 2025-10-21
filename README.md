# Voice Keyboard

Voice keyboard is a demo application showcasing Deepgram's new turn-taking speech-to-text API: **Flux**.

A voice-controlled Linux virtual keyboard that converts speech to text and types it into any application.

As a result of directly targeting Linux as a driver, this works with all Linux applications.

## Features

- **Voice-to-Text**: Speech recognition using either:
  - **Deepgram Flux** (WebSocket mode) - Real-time streaming with turn-taking STT
  - **OpenAI Whisper** (REST mode) - Record and transcribe complete utterances
- **Virtual Keyboard**: Creates a virtual input device that works with all applications
- **Incremental Typing**: Smart transcript updates with minimal backspacing for real-time corrections (WebSocket mode)
- **Toggle Control**: Enable/disable listening with keyboard shortcut (via D-Bus) or system tray icon
- **Auto-Toggle Off**: Automatically deactivates after a configurable period of silence (default: 30 seconds)
- **System Tray Icon**: Visual indicator showing active (green) or inactive (red) state
- **D-Bus Integration**: Control via D-Bus for GNOME Wayland and other desktop environments
- **Audio Recording**: Save audio input to WAV files for debugging and analysis

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

### Acquire an API key

#### For Deepgram (WebSocket mode - default)

You'll need a Deepgram API key to authenticate with Flux.

- Create or manage keys in the Deepgram console: [Create additional API keys](https://developers.deepgram.com/docs/create-additional-api-keys)
- Export the key so the app can pick it up (recommended):
  ```bash
  export DEEPGRAM_API_KEY="dg_your_api_key_here"
  ```
- The client sends the header `Authorization: Token <DEEPGRAM_API_KEY>`.

#### For OpenAI Whisper (REST mode)

You'll need an OpenAI API key to use the Whisper API.

- Get your API key from [OpenAI Platform](https://platform.openai.com/api-keys)
- Export the key:
  ```bash
  export OPENAI_API_KEY="sk-your_api_key_here"
  ```
- Use the `--stt-provider rest` flag to enable REST mode

**Security tip**: Treat API keys like passwords. Prefer env vars over committing keys to files.

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

- **Keyboard Shortcut** (Recommended for Wayland):
  - Configure a custom keyboard shortcut in your desktop environment settings
  - Command: `dbus-send --session --type=method_call --dest=com.voicekeyboard.App /com/voicekeyboard/Control com.voicekeyboard.Control.Toggle`
  - See [DBUS_INTEGRATION.md](DBUS_INTEGRATION.md) for detailed setup instructions for GNOME, KDE, i3/Sway, and more
  
- **System Tray Icon**: 
  - Click the tray icon menu and select "Toggle STT"
  - Green icon = actively listening
  - Red icon = inactive (not listening)
  - Right-click the icon to access the menu

- **Command Line**:
  ```bash
  dbus-send --session --type=method_call --dest=com.voicekeyboard.App /com/voicekeyboard/Control com.voicekeyboard.Control.Toggle
  ```

When inactive, audio recording is completely stopped to conserve system resources.

For complete D-Bus integration guide including desktop-specific setup instructions, see [DBUS_INTEGRATION.md](DBUS_INTEGRATION.md).

### Auto-Toggle Off

The application includes an automatic toggle-off feature to conserve resources when you stop speaking:

- **Default Behavior**: Automatically deactivates after **30 seconds** of silence
- **Customization**: Use `--inactivity-timeout <SECONDS>` to adjust the timeout
- **Activity Detection**: The timer resets whenever transcription results are received
- **Examples**:
  ```bash
  # Use a 60-second timeout
  sudo -E ./target/debug/voice-keyboard --inactivity-timeout 60
  
  # Disable auto-toggle by setting a very long timeout
  sudo -E ./target/debug/voice-keyboard --inactivity-timeout 3600
  ```

This feature helps ensure the microphone isn't left on indefinitely, improving both privacy and system resource usage.

## Speech-to-Text Service

This application supports two STT modes:

### WebSocket Mode (Default) - Deepgram Flux

Uses **Deepgram Flux**, the company's new turn‑taking STT API for real-time streaming transcription.

- **URL**: `wss://api.deepgram.com/v2/listen`
- **Behavior**: Streams audio continuously, receives incremental transcription updates
- **Best for**: Real-time typing as you speak, conversational interfaces
- **Command**: `sudo -E ./target/debug/voice-keyboard --stt-provider websocket`

### REST Mode - OpenAI Whisper

Uses **OpenAI Whisper API** for batch transcription of complete recordings.

- **URL**: `https://api.openai.com/v1/audio/transcriptions`
- **Behavior**: Records entire utterance when listening is active, sends complete audio when toggled off
- **Best for**: Dictation, complete sentences or paragraphs, potentially better accuracy for longer utterances
- **Command**: `sudo -E ./target/debug/voice-keyboard --stt-provider rest`

**Key Difference**: 
- WebSocket mode types text in real-time as you speak
- REST mode buffers your speech and types it all at once when you toggle off

## Command Line Options

```bash
voice-keyboard [OPTIONS]

OPTIONS:
    --test-audio                    Test audio input and show levels
    --test-stt                      Test speech-to-text functionality (default if no other mode specified)
    --debug-stt                     Debug speech-to-text (print transcripts without typing)
    --stt-provider <PROVIDER>       STT provider: 'websocket' (Deepgram) or 'rest' (OpenAI Whisper)
                                    (default: websocket)
    --stt-url <URL>                 Custom STT service URL 
                                    (WebSocket default: wss://api.deepgram.com/v2/listen)
                                    (REST default: https://api.openai.com/v1/audio/transcriptions)
    --save-audio <FILE_PATH>        Save audio to a WAV file (works with --test-audio)
    --live-mode                     Type text immediately as it's transcribed 
                                    (default: wait until end of turn, WebSocket mode only)
    --eager-eot-threshold <N>       Eager end-of-turn threshold (0.3-0.9, omit to disable, WebSocket mode only)
    --eot-threshold <N>             Standard end-of-turn threshold (0.5-0.9, default: 0.8, WebSocket mode only)
    --inactivity-timeout <SECONDS>  Auto-toggle off after this many seconds of silence (default: 30)
    -h, --help                      Print help information
    -V, --version                   Print version information
```

**Note**: If no mode is specified, the application defaults to `--test-stt` behavior.

### Usage Examples

**WebSocket mode (Deepgram Flux):**
```bash
export DEEPGRAM_API_KEY="your_key_here"
sudo -E ./target/debug/voice-keyboard --stt-provider websocket
```

**REST mode (OpenAI Whisper):**
```bash
export OPENAI_API_KEY="your_key_here"
sudo -E ./target/debug/voice-keyboard --stt-provider rest
```

**Debug mode to see transcriptions without typing:**
```bash
sudo -E ./target/debug/voice-keyboard --stt-provider rest --debug-stt
```

### Audio Recording Examples

**Option 1: Using the test-audio mode (requires sudo)**
```bash
sudo -E ./target/debug/voice-keyboard --test-audio --save-audio recording.wav
```

**Option 2: Using the standalone example (no sudo required)**
```bash
cargo run --example record_audio
```

Both options will record for 5 seconds and save the audio in 32-bit float WAV format. The example saves to `example_recording.wav` in the current directory.

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

### Keyboard Shortcut Setup

The application uses D-Bus for keyboard shortcut integration, which works on both X11 and Wayland:

1. **Configure in your desktop environment**: Follow the instructions in [DBUS_INTEGRATION.md](DBUS_INTEGRATION.md) for your specific desktop (GNOME, KDE, i3/Sway, etc.)
2. **Test the D-Bus command manually** before setting up the shortcut:
   ```bash
   dbus-send --session --type=method_call --dest=com.voicekeyboard.App /com/voicekeyboard/Control com.voicekeyboard.Control.Toggle
   ```
3. **Alternative**: Use the system tray icon to toggle listening

## Development

### Project Structure

```
src/
├── main.rs              # Main application and privilege dropping
├── virtual_keyboard.rs  # Virtual keyboard device management
├── audio_input.rs       # Audio capture and processing
├── stt_client.rs        # WebSocket STT client (Deepgram)
├── whisper_client.rs    # REST STT client (OpenAI Whisper)
├── tray_icon.rs         # System tray icon management
├── dbus_service.rs      # D-Bus interface for external control
└── input_event.rs       # Linux input event constants
```

### Key Components

- **OriginalUser**: Captures and restores user context
- **VirtualKeyboard**: Manages uinput device lifecycle with smart transcript updates
- **AudioInput**: Cross-platform audio capture with optional WAV file recording
- **SttClient**: WebSocket-based speech-to-text client (Deepgram Flux)
- **WhisperClient**: REST-based speech-to-text client (OpenAI Whisper)
- **AudioBuffer**: Manages audio chunking for STT streaming
- **DbusService**: D-Bus interface for external control and desktop integration
- **TrayManager**: System tray icon with state visualization

## License

ISC License. See LICENSE.txt

