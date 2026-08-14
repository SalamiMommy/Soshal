fn is_all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

pub fn get_subnet_prefix(ip: &str) -> Option<String> {
    if ip.is_empty() || !ip.contains('.') {
        return None;
    }
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    for part in &parts {
        if !is_all_digits(part) {
            return None;
        }
        if part.len() > 3 {
            return None;
        }
        if part.len() > 1 && part.starts_with('0') {
            return None;
        }
        let n: u16 = part.parse().ok()?;
        if n > 255 {
            return None;
        }
    }
    Some(format!("{}.{}.{}.", parts[0], parts[1], parts[2]))
}

pub fn reconcile_event_sets(local_ids: Vec<String>, remote_ids: Vec<String>) -> String {
    use std::collections::HashSet;
    let local_set: HashSet<&str> = local_ids.iter().map(|s| s.as_str()).collect();
    let remote_set: HashSet<&str> = remote_ids.iter().map(|s| s.as_str()).collect();
    let missing_local: Vec<String> = remote_ids
        .iter()
        .filter(|id| !local_set.contains(id.as_str()))
        .cloned()
        .collect();
    let missing_remote: Vec<String> = local_ids
        .iter()
        .filter(|id| !remote_set.contains(id.as_str()))
        .cloned()
        .collect();
    #[derive(serde::Serialize)]
    struct Output {
        missing_local: Vec<String>,
        missing_remote: Vec<String>,
    }
    let out = Output {
        missing_local,
        missing_remote,
    };
    serde_json::to_string(&out).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_subnet_prefix_valid() {
        assert_eq!(
            get_subnet_prefix("192.168.1.42"),
            Some("192.168.1.".to_string())
        );
        assert_eq!(get_subnet_prefix("10.0.0.1"), Some("10.0.0.".to_string()));
        assert_eq!(
            get_subnet_prefix("255.255.255.255"),
            Some("255.255.255.".to_string())
        );
    }

    #[test]
    fn test_get_subnet_prefix_malformed() {
        assert_eq!(get_subnet_prefix(""), None);
        assert_eq!(get_subnet_prefix("192.168.1"), None);
        assert_eq!(get_subnet_prefix("192.168.1.2.3"), None);
        assert_eq!(get_subnet_prefix("192.168.1.a"), None);
        assert_eq!(get_subnet_prefix("256.168.1.2"), None);
        assert_eq!(get_subnet_prefix("192.168.1.300"), None);
        assert_eq!(get_subnet_prefix("01.168.1.2"), None);
    }

    #[test]
    fn test_get_subnet_prefix_ipv6() {
        assert_eq!(get_subnet_prefix("2001:db8::1"), None);
        assert_eq!(get_subnet_prefix("::ffff:1.2.3.4"), None);
    }

    #[test]
    fn test_reconcile_empty_sets() {
        assert_eq!(
            reconcile_event_sets(vec![], vec![]),
            r#"{"missing_local":[],"missing_remote":[]}"#
        );
    }

    #[test]
    fn test_reconcile_disjoint_sets() {
        assert_eq!(
            reconcile_event_sets(
                vec!["a".to_string(), "b".to_string()],
                vec!["c".to_string(), "d".to_string()]
            ),
            r#"{"missing_local":["c","d"],"missing_remote":["a","b"]}"#
        );
    }

    #[test]
    fn test_reconcile_overlapping_sets() {
        assert_eq!(
            reconcile_event_sets(
                vec!["a".to_string(), "b".to_string()],
                vec!["b".to_string(), "c".to_string()]
            ),
            r#"{"missing_local":["c"],"missing_remote":["a"]}"#
        );
    }

    #[test]
    fn test_reconcile_identical_sets() {
        assert_eq!(
            reconcile_event_sets(
                vec!["a".to_string(), "b".to_string()],
                vec!["b".to_string(), "a".to_string()]
            ),
            r#"{"missing_local":[],"missing_remote":[]}"#
        );
    }
}
