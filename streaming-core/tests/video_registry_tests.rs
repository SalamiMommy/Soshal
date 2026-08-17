use soshal_streaming_core::video_server::{LocalVideoServer, VideoRegistry};

#[test]
fn video_registry_register_and_get() {
    let r = VideoRegistry::new();
    assert_eq!(r.get("v1"), None, "unregistered id is absent");
    r.register("v1".into(), "/tmp/v1.mp4".into());
    assert_eq!(r.get("v1"), Some("/tmp/v1.mp4".to_string()));
    r.register("v1".into(), "/tmp/v1-new.mp4".into());
    assert_eq!(
        r.get("v1"),
        Some("/tmp/v1-new.mp4".to_string()),
        "re-register overwrites"
    );
    assert_eq!(r.get("missing"), None);
}

#[tokio::test]
async fn local_video_server_lifecycle_and_route() {
    let mut server = LocalVideoServer::start().await.unwrap();
    let url = server.register_video("clip1".into(), "/nonexistent/clip1.mp4".into());
    assert_eq!(
        url,
        format!("http://127.0.0.1:{}/video/clip1", server.port())
    );
    let port: u16 = url
        .split('/')
        .nth(2)
        .unwrap()
        .rsplit(':')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(server.port(), port);
    server.stop();
    server.stop();
}
