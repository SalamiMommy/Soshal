//! File extension extraction from URL paths.

/// Extracts the file extension from a URL path.
pub fn get_extension(url_str: &str) -> String {
    let parsed_url = url::Url::parse(url_str).ok();
    let path: &str = match parsed_url.as_ref() {
        Some(u) => u.path(),
        None => {
            let cleaned = url_str.split('?').next().unwrap_or(url_str);
            cleaned.split('#').next().unwrap_or(cleaned)
        }
    };
    if let Some(last_dot) = path.rfind('.') {
        let ext: String = path[last_dot + 1..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        if !ext.is_empty() && ext.len() <= 10 {
            return ext.to_lowercase();
        }
    }
    String::new()
}
