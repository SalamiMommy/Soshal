use soshal_common_core::json_util::{json_in, json_out};

pub fn dedup_server_list(servers: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for s in servers {
        if seen.insert(s.clone()) {
            result.push(s);
        }
    }
    result
}

pub fn dedup_server_list_json(input: &str) -> String {
    let Some(servers) = json_in::<Option<Vec<String>>>(input, None) else {
        return "[]".to_string();
    };
    json_out(&dedup_server_list(servers), "[]")
}
