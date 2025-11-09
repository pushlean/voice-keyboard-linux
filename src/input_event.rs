#![allow(dead_code)]
// Linux input event structure
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct InputEvent {
    pub time: libc::timeval,
    pub type_: u16,
    pub code: u16,
    pub value: i32,
}

impl InputEvent {
    pub fn new(type_: u16, code: u16, value: i32) -> Self {
        let mut tv = libc::timeval {
            tv_sec: 0,
            tv_usec: 0,
        };

        // Get current time
        unsafe {
            libc::gettimeofday(&mut tv, std::ptr::null_mut());
        }

        Self {
            time: tv,
            type_,
            code,
            value,
        }
    }

    pub fn key_event(key: u16, pressed: bool) -> Self {
        Self::new(EV_KEY, key, if pressed { 1 } else { 0 })
    }

    pub fn syn_event() -> Self {
        Self::new(EV_SYN, SYN_REPORT, 0)
    }
}

// Event types
pub const EV_SYN: u16 = 0x00;
pub const EV_KEY: u16 = 0x01;

// Synchronization events
pub const SYN_REPORT: u16 = 0;

// Key codes - basic ASCII and special keys
pub const KEY_ESC: u16 = 1;
pub const KEY_1: u16 = 2;
pub const KEY_2: u16 = 3;
pub const KEY_3: u16 = 4;
pub const KEY_4: u16 = 5;
pub const KEY_5: u16 = 6;
pub const KEY_6: u16 = 7;
pub const KEY_7: u16 = 8;
pub const KEY_8: u16 = 9;
pub const KEY_9: u16 = 10;
pub const KEY_0: u16 = 11;
pub const KEY_MINUS: u16 = 12;
pub const KEY_EQUAL: u16 = 13;
pub const KEY_BACKSPACE: u16 = 14;
pub const KEY_TAB: u16 = 15;
pub const KEY_Q: u16 = 16;
pub const KEY_W: u16 = 17;
pub const KEY_E: u16 = 18;
pub const KEY_R: u16 = 19;
pub const KEY_T: u16 = 20;
pub const KEY_Y: u16 = 21;
pub const KEY_U: u16 = 22;
pub const KEY_I: u16 = 23;
pub const KEY_O: u16 = 24;
pub const KEY_P: u16 = 25;
pub const KEY_LEFTBRACE: u16 = 26;
pub const KEY_RIGHTBRACE: u16 = 27;
pub const KEY_ENTER: u16 = 28;
pub const KEY_LEFTCTRL: u16 = 29;
pub const KEY_A: u16 = 30;
pub const KEY_S: u16 = 31;
pub const KEY_D: u16 = 32;
pub const KEY_F: u16 = 33;
pub const KEY_G: u16 = 34;
pub const KEY_H: u16 = 35;
pub const KEY_J: u16 = 36;
pub const KEY_K: u16 = 37;
pub const KEY_L: u16 = 38;
pub const KEY_SEMICOLON: u16 = 39;
pub const KEY_APOSTROPHE: u16 = 40;
pub const KEY_GRAVE: u16 = 41;
pub const KEY_LEFTSHIFT: u16 = 42;
pub const KEY_BACKSLASH: u16 = 43;
pub const KEY_Z: u16 = 44;
pub const KEY_X: u16 = 45;
pub const KEY_C: u16 = 46;
pub const KEY_V: u16 = 47;
pub const KEY_B: u16 = 48;
pub const KEY_N: u16 = 49;
pub const KEY_M: u16 = 50;
pub const KEY_COMMA: u16 = 51;
pub const KEY_DOT: u16 = 52;
pub const KEY_SLASH: u16 = 53;
pub const KEY_RIGHTSHIFT: u16 = 54;
pub const KEY_LEFTALT: u16 = 56;
pub const KEY_SPACE: u16 = 57;
pub const KEY_CAPSLOCK: u16 = 58;

// Function keys
pub const KEY_F1: u16 = 59;
pub const KEY_F2: u16 = 60;
pub const KEY_F3: u16 = 61;
pub const KEY_F4: u16 = 62;
pub const KEY_F5: u16 = 63;
pub const KEY_F6: u16 = 64;
pub const KEY_F7: u16 = 65;
pub const KEY_F8: u16 = 66;
pub const KEY_F9: u16 = 67;
pub const KEY_F10: u16 = 68;
pub const KEY_F11: u16 = 87;
pub const KEY_F12: u16 = 88;

// uinput constants
pub const UINPUT_MAX_NAME_SIZE: usize = 80;

// UI_SET_* ioctl constants
pub const UI_SET_EVBIT: libc::c_ulong = 0x40045564;
pub const UI_SET_KEYBIT: libc::c_ulong = 0x40045565;

// UI_DEV_* ioctl constants
pub const UI_DEV_CREATE: libc::c_ulong = 0x5501;
pub const UI_DEV_DESTROY: libc::c_ulong = 0x5502;

// uinput device setup structure
#[repr(C)]
#[derive(Debug)]
pub struct UInputSetup {
    pub id: InputId,
    pub name: [u8; UINPUT_MAX_NAME_SIZE],
    pub ff_effects_max: u32,
}

impl UInputSetup {
    pub fn new(name: &str) -> Self {
        let mut setup = Self {
            id: InputId {
                bustype: 0x03, // USB
                vendor: 0x1234,
                product: 0x5678,
                version: 1,
            },
            name: [0; UINPUT_MAX_NAME_SIZE],
            ff_effects_max: 0,
        };

        let name_bytes = name.as_bytes();
        let copy_len = std::cmp::min(name_bytes.len(), UINPUT_MAX_NAME_SIZE - 1);
        setup.name[..copy_len].copy_from_slice(&name_bytes[..copy_len]);

        setup
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct InputId {
    pub bustype: u16,
    pub vendor: u16,
    pub product: u16,
    pub version: u16,
}

// Character to key code mapping
pub fn char_to_keycode(c: char) -> Option<(u16, bool)> {
    match c {
        // Direct letter mappings instead of calculating offsets
        'a' | 'A' => Some((KEY_A, c.is_uppercase())),
        'b' | 'B' => Some((KEY_B, c.is_uppercase())),
        'c' | 'C' => Some((KEY_C, c.is_uppercase())),
        'd' | 'D' => Some((KEY_D, c.is_uppercase())),
        'e' | 'E' => Some((KEY_E, c.is_uppercase())),
        'f' | 'F' => Some((KEY_F, c.is_uppercase())),
        'g' | 'G' => Some((KEY_G, c.is_uppercase())),
        'h' | 'H' => Some((KEY_H, c.is_uppercase())),
        'i' | 'I' => Some((KEY_I, c.is_uppercase())),
        'j' | 'J' => Some((KEY_J, c.is_uppercase())),
        'k' | 'K' => Some((KEY_K, c.is_uppercase())),
        'l' | 'L' => Some((KEY_L, c.is_uppercase())),
        'm' | 'M' => Some((KEY_M, c.is_uppercase())),
        'n' | 'N' => Some((KEY_N, c.is_uppercase())),
        'o' | 'O' => Some((KEY_O, c.is_uppercase())),
        'p' | 'P' => Some((KEY_P, c.is_uppercase())),
        'q' | 'Q' => Some((KEY_Q, c.is_uppercase())),
        'r' | 'R' => Some((KEY_R, c.is_uppercase())),
        's' | 'S' => Some((KEY_S, c.is_uppercase())),
        't' | 'T' => Some((KEY_T, c.is_uppercase())),
        'u' | 'U' => Some((KEY_U, c.is_uppercase())),
        'v' | 'V' => Some((KEY_V, c.is_uppercase())),
        'w' | 'W' => Some((KEY_W, c.is_uppercase())),
        'x' | 'X' => Some((KEY_X, c.is_uppercase())),
        'y' | 'Y' => Some((KEY_Y, c.is_uppercase())),
        'z' | 'Z' => Some((KEY_Z, c.is_uppercase())),
        // Numbers (note: KEY_1-KEY_9 come before KEY_0 in the keycodes)
        '0' => Some((KEY_0, false)),
        '1' => Some((KEY_1, false)),
        '2' => Some((KEY_2, false)),
        '3' => Some((KEY_3, false)),
        '4' => Some((KEY_4, false)),
        '5' => Some((KEY_5, false)),
        '6' => Some((KEY_6, false)),
        '7' => Some((KEY_7, false)),
        '8' => Some((KEY_8, false)),
        '9' => Some((KEY_9, false)),
        // Other special characters
        ' ' => Some((KEY_SPACE, false)),
        '\n' => Some((KEY_ENTER, false)),
        '\t' => Some((KEY_TAB, false)),
        '!' => Some((KEY_1, true)),
        '@' => Some((KEY_2, true)),
        '#' => Some((KEY_3, true)),
        '$' => Some((KEY_4, true)),
        '%' => Some((KEY_5, true)),
        '^' => Some((KEY_6, true)),
        '&' => Some((KEY_7, true)),
        '*' => Some((KEY_8, true)),
        '(' => Some((KEY_9, true)),
        ')' => Some((KEY_0, true)),
        '-' => Some((KEY_MINUS, false)),
        '_' => Some((KEY_MINUS, true)),
        '=' => Some((KEY_EQUAL, false)),
        '+' => Some((KEY_EQUAL, true)),
        '[' => Some((KEY_LEFTBRACE, false)),
        '{' => Some((KEY_LEFTBRACE, true)),
        ']' => Some((KEY_RIGHTBRACE, false)),
        '}' => Some((KEY_RIGHTBRACE, true)),
        '\\' => Some((KEY_BACKSLASH, false)),
        '|' => Some((KEY_BACKSLASH, true)),
        ';' => Some((KEY_SEMICOLON, false)),
        ':' => Some((KEY_SEMICOLON, true)),
        '\'' => Some((KEY_APOSTROPHE, false)),
        '"' => Some((KEY_APOSTROPHE, true)),
        '`' => Some((KEY_GRAVE, false)),
        '~' => Some((KEY_GRAVE, true)),
        ',' => Some((KEY_COMMA, false)),
        '<' => Some((KEY_COMMA, true)),
        '.' => Some((KEY_DOT, false)),
        '>' => Some((KEY_DOT, true)),
        '/' => Some((KEY_SLASH, false)),
        '?' => Some((KEY_SLASH, true)),
        _ => None,
    }
}

// Helper function to get all required key codes for keyboard setup
pub fn get_all_keycodes() -> Vec<u16> {
    let mut keys = Vec::new();

    // Add all keycodes in sequential order from 1 to 255
    // This ensures we don't miss any codes needed for special characters
    for i in 1..=255 {
        keys.push(i);
    }

    keys
}

// Legacy uinput_user_dev struct for old uinput interface
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UInputUserDev {
    pub name: [u8; 80],
    pub id: InputId,
    pub ff_effects_max: u32,
    pub absmax: [i32; 64],
    pub absmin: [i32; 64],
    pub absfuzz: [i32; 64],
    pub absflat: [i32; 64],
}

impl Default for UInputUserDev {
    fn default() -> Self {
        Self {
            name: [0; 80],
            id: InputId::default(),
            ff_effects_max: 0,
            absmax: [0; 64],
            absmin: [0; 64],
            absfuzz: [0; 64],
            absflat: [0; 64],
        }
    }
}
