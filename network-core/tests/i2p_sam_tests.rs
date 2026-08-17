//! I2P SAM V3 protocol tests against a mock SAM bridge (in-process TCP
//! listener speaking the wire protocol).

use soshal_network_core::i2p_sam::{
    I2PSamClient, I2PSessionManager, I2PTunnelConfig, I2PTunnelManager,
};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;

/// Spawns a mock SAM bridge. Replies are keyed off the command prefix.
fn spawn_mock_sam() -> (u16, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let data_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let data_port = data_listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let data_port = data_port;
        std::thread::spawn(move || {
            for conn in data_listener.incoming() {
                if let Ok(_conn) = conn {
                    std::thread::sleep(std::time::Duration::from_secs(30));
                }
            }
        });
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let handle = std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let reply = if line.starts_with("HELLO") {
                                "HELLO REPLY RESULT=OK VERSION=3.1\n".to_string()
                            } else if line.starts_with("DEST GENERATE") {
                                "DEST REPLY PUB=xyz DEST=transient-dest\n".to_string()
                            } else if line.starts_with("SESSION CREATE") {
                                if line.contains("DESTINATION=TRANSIENT") {
                                    "SESSION STATUS RESULT=OK DESTINATION=transient-dest\n"
                                        .to_string()
                                } else {
                                    "SESSION STATUS RESULT=OK\n".to_string()
                                }
                            } else if line.starts_with("STREAM CONNECT")
                                || line.starts_with("STREAM ACCEPT")
                            {
                                format!("STREAM STATUS RESULT=OK PORT={data_port}\n")
                            } else if line.starts_with("SESSION CLOSE") {
                                "SESSION STATUS RESULT=OK\n".to_string()
                            } else {
                                "UNKNOWN\n".to_string()
                            };
                            stream.write_all(reply.as_bytes()).unwrap();
                            stream.flush().unwrap();
                        }
                    }
                }
            });
            let _ = handle;
        }
    });
    (port, handle)
}

fn client(port: u16) -> I2PSamClient {
    let mut c = I2PSamClient::new("127.0.0.1".into(), port);
    c.connect().unwrap();
    c
}

#[test]
fn sam_connect_handshake_and_disconnect() {
    let (port, server) = spawn_mock_sam();
    let mut c = I2PSamClient::new("127.0.0.1".into(), port);
    assert!(c.handshake().is_err(), "send_command before connect");
    c.connect().unwrap();
    let reply = c.handshake().unwrap();
    assert!(reply.starts_with("HELLO REPLY"));
    c.disconnect().unwrap();
    assert!(c.get_session_id().is_none());
    assert!(c.get_destination().is_none());
    drop(c);
    drop(server);
}

#[test]
fn sam_connect_failure() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut c = I2PSamClient::new("127.0.0.1".into(), port);
    assert!(c.connect().is_err());
    let _ = I2PSamClient::default_client();
}

#[test]
fn sam_handshake_fails_on_bad_reply() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 512];
        let _ = stream.read(&mut buf);
        stream.write_all(b"NOPE\r\n").unwrap();
    });
    let mut c = client(port);
    let err = c.handshake().unwrap_err();
    assert!(err.contains("Handshake failed"), "{err}");
    drop(c);
    drop(server);
}

#[test]
fn sam_destination_generation_and_parse_failure() {
    let (port, server) = spawn_mock_sam();
    let mut c = client(port);
    let dest = c.generate_destination().unwrap();
    assert_eq!(dest, "transient-dest");
    drop(c);
    drop(server);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port2 = listener.local_addr().unwrap().port();
    let server2 = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 512];
        let _ = stream.read(&mut buf);
        stream.write_all(b"DEST REPLY PUB=xyz\n").unwrap();
    });
    let mut c2 = client(port2);
    assert!(c2.generate_destination().is_err(), "missing DEST= token");
    drop(c2);
    server2.join().unwrap();
}

#[test]
fn sam_session_create_transient_and_fixed() {
    let (port, server) = spawn_mock_sam();
    let mut c = client(port);
    c.create_session("s1", None).unwrap();
    assert_eq!(c.get_session_id().as_deref(), Some("s1"));
    assert_eq!(c.get_destination().as_deref(), Some("transient-dest"));
    let fixed = c.create_session("s2", Some("fixed-dest")).unwrap();
    assert!(fixed.starts_with("SESSION STATUS"));
    assert_eq!(c.get_destination().as_deref(), Some("fixed-dest"));
    let stream_err = c.connect_to_destination("some-dest");
    assert!(stream_err.is_ok(), "stream err: {stream_err:?}");
    assert!(c.accept_connection().is_ok());
    drop(c);
    drop(server);
}

#[test]
fn sam_requires_session_for_streams() {
    let (port, server) = spawn_mock_sam();
    let mut c = client(port);
    assert!(c.connect_to_destination("x").is_err(), "no active session");
    assert!(c.accept_connection().is_err(), "no active session");
    drop(c);
    drop(server);
}

#[test]
fn sam_stream_status_failure_propagates() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        loop {
            let n = stream.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            let cmd = String::from_utf8_lossy(&buf[..n]);
            let reply = if cmd.starts_with("HELLO") {
                "HELLO REPLY RESULT=OK VERSION=3.1\n"
            } else if cmd.starts_with("SESSION CREATE") {
                "SESSION STATUS RESULT=OK\n"
            } else {
                "STREAM STATUS RESULT=CANNOT_CONNECT\n"
            };
            stream.write_all(reply.as_bytes()).unwrap();
        }
    });
    let mut c = client(port);
    c.create_session("s", Some("d")).unwrap();
    assert!(c.connect_to_destination("x").is_err(), "status != OK");
    assert!(c.accept_connection().is_err(), "status != OK");
    drop(c);
    drop(server);
}

#[test]
fn tunnel_manager_lifecycle() {
    let (port, server) = spawn_mock_sam();
    let config = I2PTunnelConfig {
        sam_host: "127.0.0.1".into(),
        sam_port: port,
        session_id: "soshal".into(),
        destination: None,
        in_tunnel_count: 2,
        out_tunnel_count: 2,
    };
    let mut manager = I2PTunnelManager::new(config.clone());
    assert!(manager.start().is_ok());
    let json = serde_json::to_string(&config).unwrap();
    let back: I2PTunnelConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.sam_port, port);
    let default = I2PTunnelConfig::default();
    assert_eq!(default.session_id, "soshal");
    assert!(manager.stop().is_ok());
    drop(manager);
    drop(server);
}

#[test]
fn session_manager_error_paths_without_bridge() {
    let mgr = I2PSessionManager::new();
    assert!(!mgr.is_running());
    assert!(mgr.destination().is_none());
    assert!(mgr.connect_to_destination("x").is_err());
    assert!(mgr.accept_connection().is_err());
    mgr.stop();
    assert!(!mgr.is_running());
}
