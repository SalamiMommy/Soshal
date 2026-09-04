//! FFI protocol handler tests (app:// scheme): scheme/host routing errors,
//! media local-cache reads, avatar identicon fallback, traversal guards,
//! relay status without a client.

#[cfg(test)]
mod protocol_handler_gap_tests {
    use soshal_flutter_bridge::*;
    use std::fs;
    use std::path::PathBuf;
    fn cache_file(name: &str) -> PathBuf {
        let dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("soshal_flutter_cache");
        fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }
    #[test]
    fn handle_request_routes_and_rejects() {
        let _g = crate::test_util::lock();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let err = protocol_handler::protocol_handle_request(
                "https".into(),
                "media".into(),
                "/x".into(),
            )
            .await
            .unwrap_err();
            assert!(err.contains("Unsupported scheme"), "{err}");
            let err = protocol_handler::protocol_handle_request(
                "app".into(),
                "bogus".into(),
                "/x".into(),
            )
            .await
            .unwrap_err();
            assert!(err.contains("Unknown app:// host"), "{err}");
            let err = protocol_handler::protocol_handle_request(
                "app".into(),
                "avatar".into(),
                "/".into(),
            )
            .await
            .unwrap_err();
            assert!(err.contains("Missing pubkey"), "{err}");
            let err = protocol_handler::protocol_handle_request(
                "app".into(),
                "media".into(),
                "/blossom".into(),
            )
            .await
            .unwrap_err();
            assert!(err.contains("Missing blossom URL"), "{err}");
            let err = protocol_handler::protocol_handle_request(
                "app".into(),
                "media".into(),
                "/blossom/notaurl".into(),
            )
            .await
            .unwrap_err();
            assert!(
                err.contains("Invalid media URL")
                    || err.contains("Fetch failed")
                    || err.contains("blossom server does not resolve")
                    || err.contains("Blossom blob hash without a server URL is not supported"),
                "{err}"
            );
            let err = protocol_handler::protocol_handle_request(
                "app".into(),
                "media".into(),
                "/nope".into(),
            )
            .await
            .unwrap_err();
            assert!(err.contains("Unknown media source"), "{err}");
            let err = protocol_handler::protocol_handle_request(
                "app".into(),
                "relay".into(),
                "/status".into(),
            )
            .await
            .unwrap_err();
            assert!(err.contains("relay client not initialized"), "{err}");
        });
    }
    #[test]
    fn avatar_identicon_fallback() {
        let _g = crate::test_util::lock();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let bytes = protocol_handler::protocol_handle_request(
                "app".into(),
                "avatar".into(),
                "/a".repeat(64),
            )
            .await
            .unwrap();
            assert_eq!(
                &bytes[..8],
                &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
            );
            let meta = protocol_handler::protocol_get_metadata(
                "app".into(),
                "avatar".into(),
                "/a".repeat(64),
            )
            .await
            .unwrap();
            assert!(meta.contains("\"content_type\":\"image/png\""), "{meta}");
        });
    }
    #[test]
    fn media_local_and_cache_guards() {
        let _g = crate::test_util::lock();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let png = cache_file("cov-test.png");
            fs::write(&png, b"\x89PNG\r\n\x1a\nhello").unwrap();
            let name = png.file_name().unwrap().to_string_lossy().into_owned();
            let bytes = protocol_handler::protocol_handle_request(
                "app".into(),
                "media".into(),
                format!("/local/{name}"),
            )
            .await
            .unwrap();
            assert_eq!(bytes, b"\x89PNG\r\n\x1a\nhello");
            let meta = protocol_handler::protocol_get_metadata(
                "app".into(),
                "media".into(),
                format!("/local/{name}"),
            )
            .await
            .unwrap();
            assert!(meta.contains("\"content_type\":\"image/png\""), "{meta}");
            assert!(meta.contains("\"content_length\":13"), "{meta}");
            let bytes = protocol_handler::protocol_handle_request(
                "app".into(),
                "cache".into(),
                format!("/{name}"),
            )
            .await
            .unwrap();
            assert_eq!(bytes.len(), 13);
            let err = protocol_handler::protocol_handle_request(
                "app".into(),
                "cache".into(),
                "/../etc/passwd".into(),
            )
            .await
            .unwrap_err();
            assert!(err.contains("traversal") || err.contains("Cache"), "{err}");
            let err = protocol_handler::protocol_handle_request(
                "app".into(),
                "media".into(),
                "/local/does-not-exist.bin".into(),
            )
            .await
            .unwrap_err();
            assert!(err.contains("Cache read failed"), "{err}");
            let err = protocol_handler::protocol_get_metadata(
                "app".into(),
                "media".into(),
                "/local/does-not-exist.bin".into(),
            )
            .await
            .unwrap_err();
            assert!(err.contains("Metadata read failed"), "{err}");
            let err =
                protocol_handler::protocol_get_metadata("app".into(), "cache".into(), "/x".into())
                    .await
                    .unwrap_err();
            assert!(err.contains("Unknown app:// host"), "{err}");
            let _ = fs::remove_file(&png);
        });
    }
}
