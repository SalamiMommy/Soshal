pub fn guess_mime_type(extension: &str) -> &'static str {
    let ext = extension.trim_start_matches('.');
    let detected = soshal_common_core::mime::detect_mime_type(ext);
    if detected.is_empty() {
        "application/octet-stream"
    } else {
        match detected.as_str() {
            "image/jpeg" => "image/jpeg",
            "image/png" => "image/png",
            "image/gif" => "image/gif",
            "image/webp" => "image/webp",
            "image/avif" => "image/avif",
            "video/mp4" => "video/mp4",
            "video/webm" => "video/webm",
            "video/quicktime" => "video/quicktime",
            "audio/mpeg" => "audio/mpeg",
            "audio/ogg" => "audio/ogg",
            "audio/wav" => "audio/wav",
            "audio/flac" => "audio/flac",
            "application/pdf" => "application/pdf",
            _ => "application/octet-stream",
        }
    }
}
