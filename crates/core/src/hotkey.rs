//! Parsing of hotkey strings such as "Win+Alt+Space".

/// Modifier flags use the Win32 `MOD_*` values so the platform can pass them through.
pub const MOD_ALT: u32 = 0x0001;
pub const MOD_CONTROL: u32 = 0x0002;
pub const MOD_SHIFT: u32 = 0x0004;
pub const MOD_WIN: u32 = 0x0008;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub modifiers: u32,
    /// Win32 virtual-key code.
    pub vk: u32,
}

impl Hotkey {
    pub fn parse(s: &str) -> Option<Hotkey> {
        let mut modifiers = 0;
        let mut vk = None;
        for part in s.split('+').map(|p| p.trim().to_ascii_lowercase()) {
            match part.as_str() {
                "win" | "super" | "meta" => modifiers |= MOD_WIN,
                "alt" => modifiers |= MOD_ALT,
                "ctrl" | "control" => modifiers |= MOD_CONTROL,
                "shift" => modifiers |= MOD_SHIFT,
                key => {
                    if vk.is_some() {
                        return None;
                    }
                    vk = Some(key_code(key)?);
                }
            }
        }
        // A hotkey without modifiers would swallow normal typing.
        if modifiers == 0 {
            return None;
        }
        Some(Hotkey { modifiers, vk: vk? })
    }
}

fn key_code(key: &str) -> Option<u32> {
    let named = match key {
        "space" => 0x20,
        "enter" | "return" => 0x0D,
        "tab" => 0x09,
        "esc" | "escape" => 0x1B,
        "up" => 0x26,
        "down" => 0x28,
        "left" => 0x25,
        "right" => 0x27,
        "home" => 0x24,
        "end" => 0x23,
        "`" | "backtick" => 0xC0,
        _ => 0,
    };
    if named != 0 {
        return Some(named);
    }
    let bytes = key.as_bytes();
    if bytes.len() == 1 && (bytes[0].is_ascii_alphanumeric()) {
        return Some(bytes[0].to_ascii_uppercase() as u32);
    }
    if let Some(n) = key.strip_prefix('f').and_then(|n| n.parse::<u32>().ok()) {
        if (1..=24).contains(&n) {
            return Some(0x70 + n - 1);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_forms() {
        assert_eq!(Hotkey::parse("Win+Alt+Space"), Some(Hotkey { modifiers: MOD_WIN | MOD_ALT, vk: 0x20 }));
        assert_eq!(Hotkey::parse("ctrl + shift + k"), Some(Hotkey { modifiers: MOD_CONTROL | MOD_SHIFT, vk: b'K' as u32 }));
        assert_eq!(Hotkey::parse("Alt+F12").map(|h| h.vk), Some(0x7B));
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(Hotkey::parse("Space"), None);
        assert_eq!(Hotkey::parse("Win+A+B"), None);
        assert_eq!(Hotkey::parse("Win+Banana"), None);
        assert_eq!(Hotkey::parse(""), None);
    }
}
