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
