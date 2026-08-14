pub fn is_valid_notification_key(key: &str) -> bool {
    if key.is_empty() {
        return false;
    }
    key.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub fn is_valid_notification_key_json(input: &str) -> String {
    let valid = is_valid_notification_key(input);
    format!("{}", valid)
}
