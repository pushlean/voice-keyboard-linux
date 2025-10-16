# D-Bus Integration for Voice Keyboard

## Overview

Voice Keyboard exposes a D-Bus interface for controlling STT (Speech-to-Text) toggling on GNOME Wayland and other Linux desktop environments. This is the recommended approach for keyboard shortcut integration on Wayland, where traditional global hotkey libraries have limited support.

## Why D-Bus?

On Wayland, security restrictions prevent applications from directly registering global keyboard shortcuts. The `global-hotkey` crate doesn't work reliably on GNOME Wayland. Instead, we use D-Bus, which allows:

1. **Desktop Integration**: GNOME Settings can run commands when shortcuts are pressed
2. **Command-Line Control**: Toggle STT from terminal or scripts
3. **IPC**: Other applications can control Voice Keyboard
4. **Standard Linux Pattern**: Follows freedesktop.org conventions

## D-Bus Interface

### Service Details

- **Bus Name**: `com.voicekeyboard.App`
- **Object Path**: `/com/voicekeyboard/Control`
- **Interface**: `com.voicekeyboard.Control`

### Available Methods

#### `Toggle() -> bool`

Toggles the STT state (on/off). Returns the new state.

```bash
dbus-send --session --type=method_call --print-reply \
  --dest=com.voicekeyboard.App \
  /com/voicekeyboard/Control \
  com.voicekeyboard.Control.Toggle
```

#### `IsActive() -> bool`

Returns the current STT state without changing it.

```bash
dbus-send --session --type=method_call --print-reply \
  --dest=com.voicekeyboard.App \
  /com/voicekeyboard/Control \
  com.voicekeyboard.Control.IsActive
```

#### `SetActive(bool active) -> bool`

Sets the STT state explicitly. Returns the new state.

```bash
# Turn on
dbus-send --session --type=method_call --print-reply \
  --dest=com.voicekeyboard.App \
  /com/voicekeyboard/Control \
  com.voicekeyboard.Control.SetActive \
  boolean:true

# Turn off
dbus-send --session --type=method_call --print-reply \
  --dest=com.voicekeyboard.App \
  /com/voicekeyboard/Control \
  com.voicekeyboard.Control.SetActive \
  boolean:false
```

## Setting Up Keyboard Shortcuts

### GNOME (Ubuntu 24.04 Wayland)

1. Open **Settings** → **Keyboard** → **View and Customize Shortcuts**
2. Scroll to the bottom and click **"Custom Shortcuts"**
3. Click the **"+"** button to add a new shortcut
4. Fill in the details:
   - **Name**: `Toggle Voice Keyboard`
   - **Command**: `dbus-send --session --type=method_call --dest=com.voicekeyboard.App /com/voicekeyboard/Control com.voicekeyboard.Control.Toggle`
   - **Shortcut**: Press `Super+X` (or your preferred key combination)
5. Click **Add**

The shortcut will now work system-wide on Wayland!

### KDE Plasma

1. Open **System Settings** → **Shortcuts** → **Custom Shortcuts**
2. Click **Edit** → **New** → **Global Shortcut** → **Command/URL**
3. In the **Trigger** tab, set your desired shortcut (e.g., `Super+X`)
4. In the **Action** tab, set the command:
   ```bash
   dbus-send --session --type=method_call --dest=com.voicekeyboard.App /com/voicekeyboard/Control com.voicekeyboard.Control.Toggle
   ```
5. Click **Apply**

### i3/Sway

Add to your config file (`~/.config/i3/config` or `~/.config/sway/config`):

```
bindsym $mod+x exec dbus-send --session --type=method_call --dest=com.voicekeyboard.App /com/voicekeyboard/Control com.voicekeyboard.Control.Toggle
```

Then reload your config (`$mod+Shift+r` in i3, `$mod+Shift+c` in Sway).

## Shell Scripts

### Simple Toggle Script

Create `~/bin/voice-keyboard-toggle`:

```bash
#!/bin/bash
dbus-send --session --type=method_call \
  --dest=com.voicekeyboard.App \
  /com/voicekeyboard/Control \
  com.voicekeyboard.Control.Toggle
```

Make it executable: `chmod +x ~/bin/voice-keyboard-toggle`

### Status Script

Create `~/bin/voice-keyboard-status`:

```bash
#!/bin/bash
result=$(dbus-send --session --type=method_call --print-reply \
  --dest=com.voicekeyboard.App \
  /com/voicekeyboard/Control \
  com.voicekeyboard.Control.IsActive 2>/dev/null | \
  grep boolean | awk '{print $2}')

if [ "$result" = "true" ]; then
    echo "Voice Keyboard: ACTIVE 🎤"
else
    echo "Voice Keyboard: INACTIVE 🔇"
fi
```

Make it executable: `chmod +x ~/bin/voice-keyboard-status`

## Integration with Other Tools

### Polybar

Add to your polybar config:

```ini
[module/voicekeyboard]
type = custom/script
exec = ~/bin/voice-keyboard-status
interval = 1
click-left = dbus-send --session --type=method_call --dest=com.voicekeyboard.App /com/voicekeyboard/Control com.voicekeyboard.Control.Toggle
```

### Waybar

Add to your waybar config:

```json
"custom/voicekeyboard": {
    "exec": "~/bin/voice-keyboard-status",
    "interval": 1,
    "on-click": "dbus-send --session --type=method_call --dest=com.voicekeyboard.App /com/voicekeyboard/Control com.voicekeyboard.Control.Toggle"
}
```

## Troubleshooting

### Service Not Found

If you get `org.freedesktop.DBus.Error.ServiceUnknown`, the voice-keyboard application is not running. Start it with:

```bash
sudo ./voice-keyboard
```

### Permission Denied

The D-Bus service runs on the **session bus**, not the system bus. Make sure you're not using `--system` flag in your dbus-send commands.

### Shortcut Not Working

1. Verify the D-Bus command works manually in a terminal
2. Check that your desktop environment allows the key combination (it might be already bound)
3. Try a different key combination
4. Check system logs: `journalctl --user -f` while testing

## Technical Implementation

The D-Bus service is implemented in `src/dbus_service.rs` using the `zbus` crate. It:

1. Registers on the session bus at startup
2. Shares state with the tray icon via `Arc<Mutex<bool>>`
3. Uses callbacks to trigger STT start/stop commands
4. Runs asynchronously in the tokio runtime

The main event loop polls for state changes and updates the tray icon accordingly, ensuring thread-safe communication between the D-Bus interface, tray icon, and STT processing thread.

## Dependencies

- `zbus = "4.0"` - D-Bus client/server library for Rust
- Session D-Bus daemon (pre-installed on most Linux desktops)

## Future Enhancements

Potential future improvements:

- Add D-Bus signals for state change notifications
- Expose additional methods (get transcript, configure STT URL, etc.)
- Add introspection support for GUI D-Bus browsers
- Support for MPRIS-style media key integration

