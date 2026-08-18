use soshal_media_core::blossom::{BlossomClient, MAX_DOWNLOAD_BYTES, MAX_LIST_BYTES};

#[test]
fn blossom_client_trims_trailing_slash() {
    let c = BlossomClient::new("https://blossom.example/");
    assert_eq!(c.server_url, "https://blossom.example");
    let c2 = BlossomClient::new("https://blossom.example");
    assert_eq!(c2.server_url, "https://blossom.example");
}

#[test]
fn blossom_pinned_client_constructs() {
    let addrs = vec!["93.184.216.34:443".parse().unwrap()];
    let c = BlossomClient::new_pinned("https://blossom.example", "blossom.example", &addrs);
    assert_eq!(c.server_url, "https://blossom.example");
    let empty = BlossomClient::new_pinned("https://blossom.example", "blossom.example", &[]);
    assert_eq!(empty.server_url, "https://blossom.example");
}

#[tokio::test]
async fn blossom_download_rejects_invalid_hash_before_network() {
    let c = BlossomClient::new("http://127.0.0.1:1");
    assert_eq!(c.download("short").await.unwrap_err(), "invalid file hash");
    assert_eq!(
        c.download(&"zz".repeat(32)).await.unwrap_err(),
        "invalid file hash"
    );
    assert_eq!(
        c.download(&format!("{}{}", "a".repeat(32), "G".repeat(32)))
            .await
            .unwrap_err(),
        "invalid file hash"
    );
}

#[tokio::test]
async fn blossom_unreachable_server_errors() {
    let c = BlossomClient::new("http://127.0.0.1:1");
    let e = c.download(&"ab".repeat(32)).await.unwrap_err();
    assert!(e.contains("download failed"), "got {e}");
    let e = c
        .upload(vec![1, 2, 3], "application/octet-stream")
        .await
        .unwrap_err();
    assert!(e.contains("upload failed"), "got {e}");
    let e = c.list("pk").await.unwrap_err();
    assert!(e.contains("list failed"), "got {e}");
}

#[test]
fn blossom_caps_are_sane() {
    assert_eq!(MAX_DOWNLOAD_BYTES, 64 * 1024 * 1024);
    assert_eq!(MAX_LIST_BYTES, 4 * 1024 * 1024);
}

fn spawn_mock_blossom(resp_for: impl Fn(&str) -> Vec<u8> + Send + 'static) -> std::net::SocketAddr {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = Vec::new();
            let mut chunk = [0u8; 1024];
            loop {
                match stream.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&chunk[..n]);
                        if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() > 65536 {
                            break;
                        }
                    }
                }
            }
            let req = String::from_utf8_lossy(&buf);
            let resp = resp_for(&req);
            let _ = stream.write_all(&resp);
            let _ = stream.flush();
        }
    });
    addr
}

fn http_response(status: &str, body: &[u8]) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    out.extend_from_slice(body);
    out
}

fn media_file_json() -> String {
    format!(
        "{{\"url\":\"http://blossom.local/{}u\",\"sha256\":\"{}\",\"size\":3,\
         \"mime_type\":\"application/octet-stream\",\"created_at\":0}}",
        "ab".repeat(32),
        "cd".repeat(32)
    )
}

#[tokio::test]
async fn blossom_upload_and_authenticated_success() {
    let body = media_file_json();
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let log_mock = log.clone();
    let server = spawn_mock_blossom(move |req| {
        log_mock.lock().unwrap().push(req.to_string());
        http_response("200 OK", body.as_bytes())
    });
    let host = "blossom.local";
    let c = BlossomClient::new_pinned(&format!("http://{host}"), host, &[server]);

    let file = c
        .upload(vec![1, 2, 3], "application/octet-stream")
        .await
        .unwrap();
    assert_eq!(file.size, 3);
    assert_eq!(file.mime_type, "application/octet-stream");

    let auth_file = c
        .upload_authenticated(vec![1, 2, 3], "application/octet-stream", "tok123")
        .await
        .unwrap();
    assert_eq!(auth_file.sha256, "cd".repeat(32));

    let requests = log.lock().unwrap();
    assert!(requests.len() >= 2, "got {} requests", requests.len());
    assert!(requests[0].contains("PUT /upload"));
    assert!(!requests[0].to_lowercase().contains("authorization"));
    assert!(requests[1].contains("PUT /upload"));
    assert!(requests[1]
        .to_lowercase()
        .contains("authorization: nostr tok123"));
}

#[tokio::test]
async fn blossom_download_success_and_content_lengths() {
    let hash = "ab".repeat(32);
    let hash_match = hash.clone();
    let server = spawn_mock_blossom(move |req| {
        if req.contains(&format!("/{hash_match} ")) {
            http_response("200 OK", b"hello blossom")
        } else {
            http_response("404 Not Found", b"nope")
        }
    });
    let host = "blossom.local";
    let c = BlossomClient::new_pinned(&format!("http://{host}"), host, &[server]);

    let bytes = c.download(&hash).await.unwrap();
    assert_eq!(bytes, b"hello blossom".to_vec());
}

#[tokio::test]
async fn blossom_list_success_and_declared_too_large() {
    let declared = MAX_LIST_BYTES + 1;
    let server = spawn_mock_blossom(move |req| {
        if req.contains("/list/pk1") {
            let mut out = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {declared}\r\nConnection: close\r\n\r\n"
            )
            .into_bytes();
            out.extend_from_slice(&media_file_json().into_bytes());
            out
        } else {
            http_response("200 OK", b"[]")
        }
    });
    let host = "blossom.local";
    let c = BlossomClient::new_pinned(&format!("http://{host}"), host, &[server]);

    let err = c.list("pk1").await.unwrap_err();
    assert!(err.contains("list response too large"), "got {err}");

    let empty = c.list("pk2").await.unwrap();
    assert!(empty.is_empty());
}
