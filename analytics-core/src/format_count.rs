//! Human-readable count formatting.

pub fn format_count_json(json_input: &str) -> Result<String, String> {
    let input: crate::FormatCountInput =
        serde_json::from_str(json_input).map_err(|e| e.to_string())?;
    Ok(format_count(input.n))
}

pub fn format_count(n: f64) -> String {
    if n >= 1_000_000.0 {
        format!("{:.1}M", n / 1_000_000.0)
    } else if n >= 1_000.0 {
        format!("{:.1}K", n / 1_000.0)
    } else {
        format!("{}", n as u64)
    }
}
