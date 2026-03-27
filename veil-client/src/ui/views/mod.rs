mod chat;
mod messages;
mod settings;
mod setup;
mod sidebar;

/// Derive two-character initials from a display name or fingerprint.
pub(crate) fn initials(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return "??".to_string();
    }
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    match words.len() {
        0 => "??".to_string(),
        1 => {
            let mut chars = words[0].chars();
            let first = chars.next().unwrap_or('?');
            let second = chars.next().unwrap_or(first);
            format!("{first}{second}").to_uppercase()
        }
        _ => {
            let first = words[0].chars().next().unwrap_or('?');
            let second = words[1].chars().next().unwrap_or('?');
            format!("{first}{second}").to_uppercase()
        }
    }
}
