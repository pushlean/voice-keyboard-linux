#![allow(dead_code)]

use anyhow::{Context, Result};
use nix::fcntl::{open, OFlag};
use nix::sys::stat::Mode;
use nix::unistd::close;
use regex::Regex;
use std::io::Write;
use std::os::unix::io::FromRawFd;
use tracing::{debug, error, info, warn};

use crate::input_event::*;

// Define ioctl macros for uinput
// The nix ioctl_write_int! macro requires the ioctl type and number
nix::ioctl_write_int!(ui_set_evbit, b'U', 100);
nix::ioctl_write_int!(ui_set_keybit, b'U', 101);
nix::ioctl_none!(ui_dev_create, b'U', 1);
nix::ioctl_none!(ui_dev_destroy, b'U', 2);

/// Hardware abstraction trait for keyboard operations
pub trait KeyboardHardware {
    fn type_text(&mut self, text: &str) -> Result<()>;
    fn press_backspace(&mut self) -> Result<()>;
    fn press_enter(&mut self) -> Result<()>;
    fn press_key(&mut self, keycode: u16) -> Result<()>;
}

/// Real hardware implementation using Linux uinput
pub struct RealKeyboardHardware {
    fd: i32,
    name: String,
}

impl RealKeyboardHardware {
    pub fn new(device_name: &str) -> Result<Self> {
        info!("Creating virtual keyboard device: {}", device_name);

        // Open uinput device
        let fd = open(
            "/dev/uinput",
            OFlag::O_WRONLY | OFlag::O_NONBLOCK,
            Mode::empty(),
        )
        .context("Failed to open /dev/uinput")?;

        debug!("Opened uinput device with fd: {}", fd);

        // Enable key events
        unsafe {
            ui_set_evbit(fd, EV_KEY as u64).context("Failed to enable key events")?;
        }

        // Enable all required key codes
        let keycodes = get_all_keycodes();
        debug!("Enabling {} key codes", keycodes.len());

        for keycode in keycodes {
            unsafe {
                ui_set_keybit(fd, keycode as u64)
                    .context(format!("Failed to enable key code {keycode}"))?;
            }
        }

        // Set up device using the legacy uinput_user_dev interface
        let mut uidev = crate::input_event::UInputUserDev::default();
        // Set device name
        let name_bytes = device_name.as_bytes();
        let copy_len = std::cmp::min(name_bytes.len(), uidev.name.len() - 1);
        uidev.name[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
        // Set device id
        uidev.id.bustype = 0x03; // USB
        uidev.id.vendor = 0x1234;
        uidev.id.product = 0x5678;
        uidev.id.version = 1;
        uidev.ff_effects_max = 0;

        // Write device info
        let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
        let uidev_bytes = unsafe {
            std::slice::from_raw_parts(
                &uidev as *const _ as *const u8,
                std::mem::size_of::<crate::input_event::UInputUserDev>(),
            )
        };

        file.write_all(uidev_bytes)
            .context("Failed to write device info")?;

        // Prevent the file from being closed when it goes out of scope
        std::mem::forget(file);

        // Create the device
        unsafe {
            ui_dev_create(fd).context("Failed to create uinput device")?;
        }

        info!("Virtual keyboard '{}' created successfully", device_name);

        Ok(Self {
            fd,
            name: device_name.to_string(),
        })
    }

    fn send_event(&self, event: InputEvent) -> Result<()> {
        let event_bytes = unsafe {
            std::slice::from_raw_parts(
                &event as *const _ as *const u8,
                std::mem::size_of::<InputEvent>(),
            )
        };

        let bytes_written = unsafe {
            libc::write(
                self.fd,
                event_bytes.as_ptr() as *const libc::c_void,
                event_bytes.len(),
            )
        };

        if bytes_written != event_bytes.len() as isize {
            return Err(anyhow::anyhow!(
                "Partial write: expected {} bytes, wrote {}",
                event_bytes.len(),
                bytes_written
            ));
        }

        Ok(())
    }

    fn send_key(&self, keycode: u16, pressed: bool) -> Result<()> {
        debug!("Sending key: {} (pressed: {})", keycode, pressed);

        // Send key event
        let key_event = InputEvent::key_event(keycode, pressed);
        self.send_event(key_event)?;

        // Send synchronization event
        let syn_event = InputEvent::syn_event();
        self.send_event(syn_event)?;

        Ok(())
    }
}

impl KeyboardHardware for RealKeyboardHardware {
    fn type_text(&mut self, text: &str) -> Result<()> {
        debug!("Typing text: '{}'", text);

        for c in text.chars() {
            if let Some((keycode, needs_shift)) = char_to_keycode(c) {
                if needs_shift {
                    // Press shift
                    self.send_key(KEY_LEFTSHIFT, true)?;

                    // Press the key
                    self.send_key(keycode, true)?;
                    self.send_key(keycode, false)?;

                    // Release shift
                    self.send_key(KEY_LEFTSHIFT, false)?;
                } else {
                    // Just press the key
                    self.press_key(keycode)?;
                }

                // Small delay between characters for more natural typing
                std::thread::sleep(std::time::Duration::from_millis(10));
            } else {
                warn!("Unsupported character: '{}'", c);
            }
        }

        Ok(())
    }

    fn press_backspace(&mut self) -> Result<()> {
        self.press_key(KEY_BACKSPACE)
    }

    fn press_enter(&mut self) -> Result<()> {
        self.press_key(KEY_ENTER)
    }

    fn press_key(&mut self, keycode: u16) -> Result<()> {
        self.send_key(keycode, true)?;
        self.send_key(keycode, false)?;
        Ok(())
    }
}

impl Drop for RealKeyboardHardware {
    fn drop(&mut self) {
        info!("Destroying virtual keyboard '{}'", self.name);

        // Destroy the device
        unsafe {
            if let Err(e) = ui_dev_destroy(self.fd) {
                error!("Failed to destroy uinput device: {}", e);
            }
        }

        // Close the file descriptor
        if let Err(e) = close(self.fd) {
            error!("Failed to close uinput fd: {}", e);
        }

        debug!("Virtual keyboard '{}' destroyed", self.name);
    }
}

// Safety: RealKeyboardHardware only contains a file descriptor and a string,
// both of which are safe to send between threads
unsafe impl Send for RealKeyboardHardware {}
unsafe impl Sync for RealKeyboardHardware {}

/// Business logic layer that handles transcript processing and enter command detection
pub struct VirtualKeyboard<H: KeyboardHardware> {
    hardware: H,
    current_text: String,
    delay_input: bool,
    eager_eot_finalized: bool,  // Track if we've finalized due to eager EOT
}

impl<H: KeyboardHardware> VirtualKeyboard<H> {
    pub fn new(hardware: H, delay_input: bool) -> Self {
        Self {
            hardware,
            current_text: String::new(),
            delay_input,
            eager_eot_finalized: false,
        }
    }

    /// Update the transcript incrementally, handling smart backspacing
    /// 1. Type new characters if the new transcript extends the current one
    /// 2. Only backspace the characters that actually changed, then type the new ending
    pub fn update_transcript(&mut self, new_transcript: &str) -> Result<()> {
        // Trim whitespace from the new transcript
        let new_transcript = new_transcript.trim();
        
        debug!(
            "Updating transcript from '{}' to '{}'",
            self.current_text, new_transcript
        );

        // If delay_input is enabled, just store the transcript without typing
        if self.delay_input {
            self.current_text = new_transcript.to_string();
            return Ok(());
        }

        // If the new transcript is empty, clear everything
        if new_transcript.is_empty() {
            self.clear_current_text()?;
            return Ok(());
        }

        // Check if the new transcript is just an extension of the current one
        if new_transcript.starts_with(&self.current_text) {
            // Just type the new characters
            let new_chars = &new_transcript[self.current_text.len()..];
            if !new_chars.is_empty() {
                debug!("Typing new characters: '{}'", new_chars);
                self.hardware.type_text(new_chars)?;
                self.current_text = new_transcript.to_string();
            }
        } else {
            // Find the common prefix between current and new transcript
            let common_prefix_len = self
                .current_text
                .chars()
                .zip(new_transcript.chars())
                .take_while(|(a, b)| a == b)
                .count();

            let current_chars: Vec<char> = self.current_text.chars().collect();
            let chars_to_backspace = current_chars.len() - common_prefix_len;

            debug!(
                "Common prefix length: {}, need to backspace {} characters",
                common_prefix_len, chars_to_backspace
            );

            // Only backspace the characters that differ
            if chars_to_backspace > 0 {
                debug!("Backspacing {} characters", chars_to_backspace);
                for _ in 0..chars_to_backspace {
                    self.hardware.press_backspace()?;
                }
            }

            // Type the new ending (everything after the common prefix)
            let new_chars: Vec<char> = new_transcript.chars().collect();
            if common_prefix_len < new_chars.len() {
                let new_ending: String = new_chars[common_prefix_len..].iter().collect();
                debug!("Typing new ending: '{}'", new_ending);
                self.hardware.type_text(&new_ending)?;
            }

            self.current_text = new_transcript.to_string();
        }

        Ok(())
    }

    /// Finalize the current transcript
    /// If the transcript ends with "enter" (with optional punctuation/whitespace),
    /// backspace that portion and press the ENTER key
    /// Otherwise, just finalize without pressing enter
    pub fn finalize_transcript(&mut self) -> Result<()> {
        // Trim whitespace from the current text before finalizing
        self.current_text = self.current_text.trim().to_string();
        
        debug!("Finalizing transcript: '{}'", self.current_text);

        // If delay_input is enabled, type the complete text and clear
        if self.delay_input {
            // If we already finalized due to eager EOT, skip typing again
            if self.eager_eot_finalized {
                debug!("Skipping finalize - already finalized on eager EOT");
                self.eager_eot_finalized = false;  // Reset for next turn
                self.current_text.clear();
                return Ok(());
            }
            
            // Only type if there's actual text (non-empty after trimming)
            if !self.current_text.is_empty() {
                self.hardware.type_text(&self.current_text)?;
                self.current_text.clear();
                self.hardware.type_text(" ")?;
            } else {
                self.current_text.clear();
            }
            return Ok(());
        }

        // Regex to match "enter" (case-insensitive) at the end, optionally followed by
        // punctuation and/or whitespace: (?i)\s*\benter\b[[:punct:]\s]*$
        // (?i) = case insensitive
        // \s* = optional leading whitespace
        // \benter\b = the word "enter" with word boundaries
        // [[:punct:]\s]* = optional trailing punctuation or whitespace
        // $ = end of string
        let enter_regex = Regex::new(r"(?i)\s*\benter\b[[:punct:]\s]*$").unwrap();

        // Find the match and extract the information we need before mutating self
        let match_info = enter_regex.find(&self.current_text).map(|m| {
            (
                m.start(),
                m.as_str().chars().count(),
                m.as_str().to_string(),
            )
        });

        if let Some((start_pos, chars_to_backspace, matched_str)) = match_info {
            debug!(
                "Found 'enter' command at end of transcript: '{}'",
                matched_str
            );
            debug!(
                "Backspacing {} characters for 'enter' command",
                chars_to_backspace
            );

            // Backspace the matched portion
            for _ in 0..chars_to_backspace {
                self.hardware.press_backspace()?;
                // Small delay between backspaces for reliability
                std::thread::sleep(std::time::Duration::from_millis(5));
            }

            // Update our internal tracking to remove the backspaced characters
            self.current_text = self.current_text[..start_pos].to_string();

            // Press the actual ENTER key
            debug!("Pressing ENTER key");
            self.hardware.press_enter()?;
        }

        // Clear the current text tracking
        self.current_text.clear();

        // Add a space after the end of each turn
        self.hardware.type_text(" ")?;

        Ok(())
    }

    /// Clear the current text by backspacing
    fn clear_current_text(&mut self) -> Result<()> {
        if !self.current_text.is_empty() {
            self.backspace_current_text()?;
        }
        Ok(())
    }

    /// Backspace all characters in the current text
    fn backspace_current_text(&mut self) -> Result<()> {
        let char_count = self.current_text.chars().count();
        debug!("Backspacing {} characters", char_count);

        for _ in 0..char_count {
            self.hardware.press_backspace()?;
            // Small delay between backspaces for reliability
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        self.current_text.clear();
        Ok(())
    }

    /// Get the current text that has been typed
    pub fn get_current_text(&self) -> &str {
        &self.current_text
    }

    /// Mark that we've finalized due to eager end of turn
    pub fn mark_eager_eot_finalized(&mut self) {
        self.eager_eot_finalized = true;
    }

    /// Reset eager EOT flag (called when turn is resumed)
    pub fn reset_eager_eot_flag(&mut self) {
        self.eager_eot_finalized = false;
    }
}

/// Mock hardware implementation for testing
pub struct MockKeyboardHardware {
    pub typed_chars: Vec<char>,
    pub backspace_count: usize,
    pub enter_pressed: bool,
}

impl MockKeyboardHardware {
    pub fn new() -> Self {
        Self {
            typed_chars: Vec::new(),
            backspace_count: 0,
            enter_pressed: false,
        }
    }
}

impl KeyboardHardware for MockKeyboardHardware {
    fn type_text(&mut self, text: &str) -> Result<()> {
        for c in text.chars() {
            self.typed_chars.push(c);
        }
        Ok(())
    }

    fn press_backspace(&mut self) -> Result<()> {
        self.backspace_count += 1;
        if !self.typed_chars.is_empty() {
            self.typed_chars.pop();
        }
        Ok(())
    }

    fn press_enter(&mut self) -> Result<()> {
        self.enter_pressed = true;
        Ok(())
    }

    fn press_key(&mut self, _keycode: u16) -> Result<()> {
        // Mock implementation - could log the keycode if needed
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_incremental_typing_extension() {
        let mut kb = VirtualKeyboard::new(MockKeyboardHardware::new(), false);

        // Start with "hello"
        kb.update_transcript("hello").unwrap();
        assert_eq!(kb.current_text, "hello");
        assert_eq!(kb.hardware.typed_chars, ['h', 'e', 'l', 'l', 'o']);
        assert_eq!(kb.hardware.backspace_count, 0);

        // Extend to "hello world"
        kb.update_transcript("hello world").unwrap();
        assert_eq!(kb.current_text, "hello world");
        assert_eq!(
            kb.hardware.typed_chars,
            ['h', 'e', 'l', 'l', 'o', ' ', 'w', 'o', 'r', 'l', 'd']
        );
        assert_eq!(kb.hardware.backspace_count, 0);
    }

    #[test]
    fn test_incremental_typing_change() {
        let mut kb = VirtualKeyboard::new(MockKeyboardHardware::new(), false);

        // Start with "hello"
        kb.update_transcript("hello").unwrap();
        assert_eq!(kb.current_text, "hello");
        assert_eq!(kb.hardware.typed_chars, ['h', 'e', 'l', 'l', 'o']);

        // Change to "hi there" - should only backspace "ello" and type "i there"
        kb.update_transcript("hi there").unwrap();
        assert_eq!(kb.current_text, "hi there");
        // Should have backspaced 4 characters ("ello") and typed 7 new ones ("i there")
        assert_eq!(kb.hardware.backspace_count, 4);
        assert_eq!(
            kb.hardware.typed_chars,
            ['h', 'i', ' ', 't', 'h', 'e', 'r', 'e']
        );
    }

    #[test]
    fn test_finalize_transcript() {
        let mut kb = VirtualKeyboard::new(MockKeyboardHardware::new(), false);

        // Type some text
        kb.update_transcript("hello").unwrap();
        assert_eq!(kb.current_text, "hello");
        assert!(!kb.hardware.enter_pressed);

        // Finalize (should not press enter anymore but should add a space)
        kb.finalize_transcript().unwrap();
        assert_eq!(kb.current_text, "");
        assert!(!kb.hardware.enter_pressed); // Should remain false
        assert_eq!(kb.hardware.typed_chars, ['h', 'e', 'l', 'l', 'o', ' ']);
    }

    #[test]
    fn test_empty_transcript() {
        let mut kb = VirtualKeyboard::new(MockKeyboardHardware::new(), false);

        // Start with some text
        kb.update_transcript("hello").unwrap();
        assert_eq!(kb.current_text, "hello");

        // Clear with empty transcript
        kb.update_transcript("").unwrap();
        assert_eq!(kb.current_text, "");
        assert_eq!(kb.hardware.backspace_count, 5);
    }

    #[test]
    fn test_smart_backspacing_partial_change() {
        let mut kb = VirtualKeyboard::new(MockKeyboardHardware::new(), false);

        // Start with "hello world"
        kb.update_transcript("hello world").unwrap();
        assert_eq!(kb.current_text, "hello world");
        assert_eq!(
            kb.hardware.typed_chars,
            ['h', 'e', 'l', 'l', 'o', ' ', 'w', 'o', 'r', 'l', 'd']
        );

        // Change to "hello there" - should only backspace "world" and type "there"
        kb.update_transcript("hello there").unwrap();
        assert_eq!(kb.current_text, "hello there");
        // Should have backspaced 5 characters ("world") and typed 5 new ones ("there")
        assert_eq!(kb.hardware.backspace_count, 5);
        assert_eq!(
            kb.hardware.typed_chars,
            ['h', 'e', 'l', 'l', 'o', ' ', 't', 'h', 'e', 'r', 'e']
        );
    }

    #[test]
    fn test_smart_backspacing_no_common_prefix() {
        let mut kb = VirtualKeyboard::new(MockKeyboardHardware::new(), false);

        // Start with "hello"
        kb.update_transcript("hello").unwrap();
        assert_eq!(kb.current_text, "hello");

        // Change to "goodbye" - no common prefix, should backspace all and retype
        kb.update_transcript("goodbye").unwrap();
        assert_eq!(kb.current_text, "goodbye");
        // Should have backspaced 5 characters and typed 7 new ones
        assert_eq!(kb.hardware.backspace_count, 5);
        assert_eq!(kb.hardware.typed_chars, ['g', 'o', 'o', 'd', 'b', 'y', 'e']);
    }

    #[test]
    fn test_smart_backspacing_shortening() {
        let mut kb = VirtualKeyboard::new(MockKeyboardHardware::new(), false);

        // Start with "hello world"
        kb.update_transcript("hello world").unwrap();
        assert_eq!(kb.current_text, "hello world");

        // Change to "hello" - should only backspace " world"
        kb.update_transcript("hello").unwrap();
        assert_eq!(kb.current_text, "hello");
        // Should have backspaced 6 characters (" world") and typed 0 new ones
        assert_eq!(kb.hardware.backspace_count, 6);
        assert_eq!(kb.hardware.typed_chars, ['h', 'e', 'l', 'l', 'o']);
    }

    #[test]
    fn test_finalize_with_enter_command() {
        let mut kb = VirtualKeyboard::new(MockKeyboardHardware::new(), false);

        // Type text ending with "enter"
        kb.update_transcript("Write a unit test enter").unwrap();
        assert_eq!(kb.current_text, "Write a unit test enter");
        assert!(!kb.hardware.enter_pressed);

        // Finalize - should backspace "enter" and press ENTER key
        kb.finalize_transcript().unwrap();
        assert_eq!(kb.current_text, "");
        assert!(kb.hardware.enter_pressed);
        // Should have backspaced 6 characters (" enter")
        assert_eq!(kb.hardware.backspace_count, 6);
    }

    #[test]
    fn test_finalize_with_enter_only() {
        let mut kb = VirtualKeyboard::new(MockKeyboardHardware::new(), false);

        // Type only "enter"
        kb.update_transcript("enter").unwrap();
        assert_eq!(kb.current_text, "enter");
        assert!(!kb.hardware.enter_pressed);

        // Finalize - should backspace all and press ENTER key
        kb.finalize_transcript().unwrap();
        assert_eq!(kb.current_text, "");
        assert!(kb.hardware.enter_pressed);
        // Should have backspaced 5 characters ("enter")
        assert_eq!(kb.hardware.backspace_count, 5);
    }

    #[test]
    fn test_finalize_with_enter_case_insensitive() {
        let mut kb = VirtualKeyboard::new(MockKeyboardHardware::new(), false);

        // Type text ending with "ENTER" (uppercase)
        kb.update_transcript("hello ENTER").unwrap();
        assert_eq!(kb.current_text, "hello ENTER");
        assert!(!kb.hardware.enter_pressed);

        // Finalize - should still recognize and handle it
        kb.finalize_transcript().unwrap();
        assert_eq!(kb.current_text, "");
        assert!(kb.hardware.enter_pressed);
        // Should have backspaced 6 characters (" ENTER")
        assert_eq!(kb.hardware.backspace_count, 6);
    }

    #[test]
    fn test_finalize_without_enter_command() {
        let mut kb = VirtualKeyboard::new(MockKeyboardHardware::new(), false);

        // Type text not ending with "enter"
        kb.update_transcript("hello world").unwrap();
        assert_eq!(kb.current_text, "hello world");
        assert!(!kb.hardware.enter_pressed);

        // Finalize - should not press ENTER key but should add a space
        kb.finalize_transcript().unwrap();
        assert_eq!(kb.current_text, "");
        assert!(!kb.hardware.enter_pressed);
        // Should not have backspaced anything for the enter command
        assert_eq!(kb.hardware.backspace_count, 0);
        assert_eq!(kb.hardware.typed_chars, ['h', 'e', 'l', 'l', 'o', ' ', 'w', 'o', 'r', 'l', 'd', ' ']);
    }

    #[test]
    fn test_finalize_with_enter_in_middle() {
        let mut kb = VirtualKeyboard::new(MockKeyboardHardware::new(), false);

        // Type text with "enter" in the middle, not at the end
        kb.update_transcript("enter the room").unwrap();
        assert_eq!(kb.current_text, "enter the room");
        assert!(!kb.hardware.enter_pressed);

        // Finalize - should not press ENTER key since "enter" is not the last word but should add a space
        kb.finalize_transcript().unwrap();
        assert_eq!(kb.current_text, "");
        assert!(!kb.hardware.enter_pressed);
        // Should not have backspaced anything for the enter command
        assert_eq!(kb.hardware.backspace_count, 0);
        assert_eq!(kb.hardware.typed_chars, ['e', 'n', 't', 'e', 'r', ' ', 't', 'h', 'e', ' ', 'r', 'o', 'o', 'm', ' ']);
    }

    #[test]
    fn test_enter_capitalization_patterns() {
        // Test various capitalization patterns
        let test_cases = vec![
            ("hello enter", true), // lowercase
            ("hello ENTER", true), // uppercase
            ("hello Enter", true), // title case
            ("hello EnTeR", true), // mixed case
            ("hello eNtEr", true), // mixed case
        ];

        for (input, should_trigger) in test_cases {
            let mut kb = VirtualKeyboard::new(MockKeyboardHardware::new(), false);
            kb.update_transcript(input).unwrap();
            kb.finalize_transcript().unwrap();

            if should_trigger {
                assert!(
                    kb.hardware.enter_pressed,
                    "Should trigger ENTER for: '{}'",
                    input
                );
            } else {
                assert!(
                    !kb.hardware.enter_pressed,
                    "Should NOT trigger ENTER for: '{}'",
                    input
                );
            }
        }
    }

    #[test]
    fn test_enter_with_punctuation() {
        // Test punctuation patterns - these SHOULD work with the regex approach
        let test_cases = vec![
            ("hello enter.", true),   // period attached
            ("hello enter!", true),   // exclamation attached
            ("hello enter?", true),   // question mark attached
            ("hello enter,", true),   // comma attached
            ("hello enter;", true),   // semicolon attached
            ("hello enter:", true),   // colon attached
            ("hello enter...", true), // multiple periods
            ("hello enter!!", true),  // multiple exclamations
        ];

        for (input, should_trigger) in test_cases {
            let mut kb = VirtualKeyboard::new(MockKeyboardHardware::new(), false);
            kb.update_transcript(input).unwrap();
            kb.finalize_transcript().unwrap();

            if should_trigger {
                assert!(
                    kb.hardware.enter_pressed,
                    "Should trigger ENTER for: '{}'",
                    input
                );
            } else {
                assert!(
                    !kb.hardware.enter_pressed,
                    "Should NOT trigger ENTER for: '{}'",
                    input
                );
            }
        }
    }

    #[test]
    fn test_enter_with_whitespace_and_punctuation() {
        // Test whitespace and punctuation combinations
        let test_cases = vec![
            ("hello enter ", true),   // trailing space
            ("hello enter  ", true),  // multiple trailing spaces
            ("hello enter\t", true),  // trailing tab
            ("hello enter\n", true),  // trailing newline
            ("hello enter .", true),  // space then period
            ("hello enter !", true),  // space then exclamation
            ("hello enter ?", true),  // space then question mark
            ("hello enter . ", true), // period with spaces
            ("hello enter ! ", true), // exclamation with spaces
        ];

        for (input, should_trigger) in test_cases {
            let mut kb = VirtualKeyboard::new(MockKeyboardHardware::new(), false);
            kb.update_transcript(input).unwrap();
            kb.finalize_transcript().unwrap();

            if should_trigger {
                assert!(
                    kb.hardware.enter_pressed,
                    "Should trigger ENTER for: '{}'",
                    input
                );
            } else {
                assert!(
                    !kb.hardware.enter_pressed,
                    "Should NOT trigger ENTER for: '{}'",
                    input
                );
            }
        }
    }

    #[test]
    fn test_enter_with_leading_whitespace() {
        // Test cases with whitespace before "enter"
        let test_cases = vec![
            ("hello  enter", true), // double space before enter
            ("hello\tenter", true), // tab before enter
            ("hello\nenter", true), // newline before enter (unlikely but possible)
        ];

        for (input, should_trigger) in test_cases {
            let mut kb = VirtualKeyboard::new(MockKeyboardHardware::new(), false);
            kb.update_transcript(input).unwrap();
            kb.finalize_transcript().unwrap();

            if should_trigger {
                assert!(
                    kb.hardware.enter_pressed,
                    "Should trigger ENTER for: '{}'",
                    input
                );
            } else {
                assert!(
                    !kb.hardware.enter_pressed,
                    "Should NOT trigger ENTER for: '{}'",
                    input
                );
            }
        }
    }

    #[test]
    fn test_enter_negative_cases() {
        // Test cases that should NOT trigger enter
        let test_cases = vec![
            ("hello world", false),      // no enter
            ("enter the room", false),   // enter not at end
            ("hello entered", false),    // "entered" is not "enter"
            ("hello entering", false),   // "entering" is not "enter"
            ("hello center", false),     // "center" contains "enter" but is not "enter"
            ("hello enterprise", false), // "enterprise" contains "enter" but is not "enter"
            ("hello enter now", false),  // "enter" not at the very end
        ];

        for (input, should_trigger) in test_cases {
            let mut kb = VirtualKeyboard::new(MockKeyboardHardware::new(), false);
            kb.update_transcript(input).unwrap();
            kb.finalize_transcript().unwrap();

            if should_trigger {
                assert!(
                    kb.hardware.enter_pressed,
                    "Should trigger ENTER for: '{}'",
                    input
                );
            } else {
                assert!(
                    !kb.hardware.enter_pressed,
                    "Should NOT trigger ENTER for: '{}'",
                    input
                );
            }
        }
    }

    #[test]
    fn test_delayed_input_no_incremental_typing() {
        let mut kb = VirtualKeyboard::new(MockKeyboardHardware::new(), true);

        // First update should not type anything
        kb.update_transcript("hello").unwrap();
        assert_eq!(kb.current_text, "hello");
        assert_eq!(kb.hardware.typed_chars.len(), 0);

        // Second update should still not type anything
        kb.update_transcript("hello world").unwrap();
        assert_eq!(kb.current_text, "hello world");
        assert_eq!(kb.hardware.typed_chars.len(), 0);

        // Third update - change the text
        kb.update_transcript("goodbye").unwrap();
        assert_eq!(kb.current_text, "goodbye");
        assert_eq!(kb.hardware.typed_chars.len(), 0);
    }

    #[test]
    fn test_delayed_input_finalize() {
        let mut kb = VirtualKeyboard::new(MockKeyboardHardware::new(), true);

        // Update transcript multiple times
        kb.update_transcript("hello").unwrap();
        kb.update_transcript("hello world").unwrap();
        kb.update_transcript("hello world test").unwrap();

        // Nothing should be typed yet
        assert_eq!(kb.hardware.typed_chars.len(), 0);

        // Finalize should type the complete text
        kb.finalize_transcript().unwrap();

        // Should have typed "hello world test "
        assert_eq!(
            kb.hardware.typed_chars,
            ['h', 'e', 'l', 'l', 'o', ' ', 'w', 'o', 'r', 'l', 'd', ' ', 't', 'e', 's', 't', ' ']
        );
        assert_eq!(kb.current_text, "");
    }

    #[test]
    fn test_delayed_input_no_enter_detection() {
        let mut kb = VirtualKeyboard::new(MockKeyboardHardware::new(), true);

        // Type text ending with "enter"
        kb.update_transcript("hello enter").unwrap();
        assert_eq!(kb.hardware.typed_chars.len(), 0);

        // Finalize should type everything including "enter" (no enter detection in delayed mode)
        kb.finalize_transcript().unwrap();

        // Should have typed "hello enter " (not pressing ENTER key)
        assert_eq!(
            kb.hardware.typed_chars,
            ['h', 'e', 'l', 'l', 'o', ' ', 'e', 'n', 't', 'e', 'r', ' ']
        );
        assert!(!kb.hardware.enter_pressed);
    }

    #[test]
    fn test_delayed_input_empty_finalize() {
        let mut kb = VirtualKeyboard::new(MockKeyboardHardware::new(), true);

        // Finalize with empty transcript
        kb.finalize_transcript().unwrap();

        // Should not type anything (not even a space) when transcript is empty
        assert_eq!(kb.hardware.typed_chars, []);
        assert_eq!(kb.current_text, "");
    }

    #[test]
    fn test_eager_eot_prevents_double_typing() {
        let mut kb = VirtualKeyboard::new(MockKeyboardHardware::new(), true);

        // Simulate eager end of turn scenario
        kb.update_transcript("hello world").unwrap();
        assert_eq!(kb.hardware.typed_chars.len(), 0); // Nothing typed yet in delayed mode

        // First finalization (eager EOT)
        kb.finalize_transcript().unwrap();
        assert_eq!(
            kb.hardware.typed_chars,
            ['h', 'e', 'l', 'l', 'o', ' ', 'w', 'o', 'r', 'l', 'd', ' ']
        );
        
        // Mark that we've done eager EOT finalization
        kb.mark_eager_eot_finalized();

        // Second finalization (regular EndOfTurn) should not type anything
        kb.finalize_transcript().unwrap();
        // Should still be the same - no additional typing
        assert_eq!(
            kb.hardware.typed_chars,
            ['h', 'e', 'l', 'l', 'o', ' ', 'w', 'o', 'r', 'l', 'd', ' ']
        );
    }

    #[test]
    fn test_eager_eot_with_turn_resumed() {
        let mut kb = VirtualKeyboard::new(MockKeyboardHardware::new(), true);

        // First part of speech
        kb.update_transcript("hello").unwrap();
        assert_eq!(kb.hardware.typed_chars.len(), 0);

        // Eager EOT - finalize and mark
        kb.finalize_transcript().unwrap();
        kb.mark_eager_eot_finalized();
        assert_eq!(kb.hardware.typed_chars, ['h', 'e', 'l', 'l', 'o', ' ']);

        // Turn resumed - reset flag
        kb.reset_eager_eot_flag();

        // Continue speaking
        kb.update_transcript("world").unwrap();
        
        // Final EndOfTurn should type the new content
        kb.finalize_transcript().unwrap();
        assert_eq!(
            kb.hardware.typed_chars,
            ['h', 'e', 'l', 'l', 'o', ' ', 'w', 'o', 'r', 'l', 'd', ' ']
        );
    }
}
