//! File names for what Cratekit writes for the user (exports, #35).

/// Characters no file name may hold on at least one of the three systems.
const FORBIDDEN: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// Names Windows reserves for devices, whatever the extension.
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Longest name kept, in characters (well under every system's limit, with
/// room for a suffix such as " (vocals).wav").
const MAX_CHARS: usize = 120;

/// `title` as a file name that is valid on macOS, Windows and Linux:
/// forbidden and control characters become `_`, trailing dots and spaces go
/// (Windows drops them), a Windows device name gets a `_`, and the result is
/// at most 120 characters. An empty result is `fallback`.
#[must_use]
pub fn safe_file_name(title: &str, fallback: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| {
            if FORBIDDEN.contains(&c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .take(MAX_CHARS)
        .collect();
    let trimmed = cleaned.trim().trim_end_matches(['.', ' ']).to_owned();
    if trimmed.is_empty() {
        return fallback.to_owned();
    }
    let stem = trimmed.split('.').next().unwrap_or_default();
    if RESERVED.iter().any(|name| name.eq_ignore_ascii_case(stem)) {
        return format!("{trimmed}_");
    }
    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_ordinary_titles() {
        assert_eq!(safe_file_name("Galactic Rap", "x"), "Galactic Rap");
        assert_eq!(
            safe_file_name("Café (Extended Mix)", "x"),
            "Café (Extended Mix)"
        );
    }

    #[test]
    fn replaces_what_some_system_forbids() {
        assert_eq!(safe_file_name("AC/DC: Back?", "x"), "AC_DC_ Back_");
        assert_eq!(safe_file_name("a\u{0}b\tc", "x"), "a_b_c");
        assert_eq!(safe_file_name("Intro...", "x"), "Intro");
        assert_eq!(safe_file_name("  ", "track-4"), "track-4");
        assert_eq!(safe_file_name("...", "track-4"), "track-4");
    }

    #[test]
    fn avoids_windows_device_names_and_long_names() {
        assert_eq!(safe_file_name("con", "x"), "con_");
        assert_eq!(safe_file_name("COM1.mix", "x"), "COM1.mix_");
        assert_eq!(safe_file_name("Console", "x"), "Console");
        assert_eq!(safe_file_name(&"a".repeat(300), "x").chars().count(), 120);
    }
}
