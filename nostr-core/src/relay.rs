use nostr::event::Event;
use nostr::filter::Filter;
use nostr::key::Keys;
use nostr_sdk::authenticator::SignerAuthenticator;
use nostr_sdk::client::Client;
use std::time::Duration;

pub struct NostrClient {
    client: Client,
}

impl NostrClient {
    pub fn new(keys: Keys) -> Self {
        let authenticator = SignerAuthenticator::new(keys);
        let client = Client::builder().authenticator(authenticator).build();
        Self { client }
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    pub async fn add_relay(&self, url: &str) -> Result<bool, nostr_sdk::error::Error> {
        let (valid, _) = soshal_common_core::url::is_valid_relay_url(url);
        if !valid {
            return Err(nostr_sdk::error::Error::policy(format!(
                "invalid or disallowed relay url: {url}"
            )));
        }
        self.client.add_relay(url).await
    }

    pub async fn connect(&self) {
        self.client.connect().await;
    }

    pub async fn publish(&self, event: &Event) -> Result<(), nostr_sdk::error::Error> {
        self.client.send_event(event).await?;
        Ok(())
    }

    pub async fn fetch_events(&self, filter: Filter, timeout_secs: u64) -> Vec<Event> {
        self.client
            .fetch_events(filter)
            .timeout(Duration::from_secs(timeout_secs))
            .await
            .map(|e| e.into_iter().collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::NostrClient;
    use crate::keys::from_nsec;
    use crate::models::verify_event;
    use nostr::event::{Event, EventBuilder, EventId, FinalizeEvent, Signature, Tag};
    use nostr::key::{Keys, PublicKey};

    const SK_HEX: &str = "0000000000000000000000000000000000000000000000000000000000000001";

    fn text_note(keys: &Keys, content: &str, tags: Vec<Tag>) -> Result<Event, nostr::error::Error> {
        let mut builder = EventBuilder::new(nostr::event::Kind::TextNote, content);
        for tag in tags {
            builder = builder.tag(tag);
        }
        builder.finalize(keys)
    }

    fn addressed_to(evt: &Event, me: &PublicKey) -> bool {
        let me_hex = me.to_hex();
        evt.tags.iter().any(|t| {
            let s = t.as_slice();
            s.first().map(String::as_str) == Some("p")
                && s.get(1).map(String::as_str) == Some(me_hex.as_str())
        })
    }

    fn url_host(url: &str) -> Option<&str> {
        let rest = url.split("://").nth(1)?;
        let host_port = rest.split(['/', '?', '#']).next()?;
        Some(
            host_port
                .strip_prefix('[')
                .and_then(|h| h.split(']').next())
                .unwrap_or_else(|| host_port.split(':').next().unwrap_or(host_port)),
        )
    }

    fn outgoing_url_blocked(url: &str) -> bool {
        url_host(url)
            .map(soshal_common_core::url::is_private_ip_str)
            .unwrap_or(false)
    }

    #[test]
    fn relay_event_bad_signature_rejected() {
        let keys = from_nsec(SK_HEX).unwrap();
        let good = text_note(&keys, "relay payload", vec![]).unwrap();
        assert!(good.verify().is_ok());
        assert!(verify_event(&good));
        let mut forged = good;
        forged.content = "tampered".to_string();
        forged.sig = Signature::from_slice(&[0u8; 64]).unwrap();
        forged.id = EventId::compute(
            &forged.pubkey,
            &forged.created_at,
            &forged.kind,
            &forged.tags,
            &forged.content,
        );
        assert!(forged.verify().is_err());
        assert!(!verify_event(&forged));
    }

    #[test]
    fn p_tag_addressed_to_me_passes_other_pubkey_fails() {
        let me = from_nsec(SK_HEX).unwrap();
        let other = from_nsec("02".repeat(32).as_str()).unwrap();
        let to_me = text_note(&other, "hi", vec![Tag::public_key(me.public_key())]).unwrap();
        assert!(addressed_to(&to_me, &me.public_key()));
        let to_other = text_note(&other, "hi", vec![Tag::public_key(other.public_key())]).unwrap();
        assert!(!addressed_to(&to_other, &me.public_key()));
        let no_tag = text_note(&other, "hi", vec![]).unwrap();
        assert!(!addressed_to(&no_tag, &me.public_key()));
    }

    #[test]
    fn outgoing_url_blocks_private_loopback_allows_public() {
        for url in [
            "http://127.0.0.1",
            "http://127.0.0.2:8080/x",
            "http://10.0.0.1",
            "http://10.255.255.255",
            "http://192.168.1.1",
            "http://172.16.0.1",
            "wss://[::1]:7447",
            "http://0.0.0.0",
        ] {
            assert!(outgoing_url_blocked(url), "should block {url}");
        }
        for url in [
            "https://8.8.8.8",
            "https://1.1.1.1:443/path",
            "wss://relay.damus.io",
            "https://example.com:8443/x?q=1",
            "relay.example.com",
        ] {
            assert!(!outgoing_url_blocked(url), "should allow {url}");
        }
    }

    #[test]
    fn relay_url_host_parsing() {
        assert_eq!(
            url_host("wss://relay.example.com"),
            Some("relay.example.com")
        );
        assert_eq!(url_host("https://127.0.0.1:8080/x"), Some("127.0.0.1"));
        assert_eq!(url_host("wss://[::1]:7447"), Some("::1"));
        assert_eq!(url_host("not a url"), None);
        assert_eq!(url_host(""), None);
    }

    #[tokio::test]
    async fn add_relay_rejects_invalid_url() {
        let client = NostrClient::new(from_nsec(SK_HEX).unwrap());
        assert!(client.add_relay("not a url").await.is_err());
        assert!(client.add_relay("wss://").await.is_err());
        assert!(client.add_relay("ws://127.0.0.1:8080").await.is_err());
        assert!(client.add_relay("ws://192.168.1.1:8080").await.is_err());
    }

    #[tokio::test]
    async fn add_relay_accepts_valid_url_without_network() {
        let client = NostrClient::new(from_nsec(SK_HEX).unwrap());
        assert!(client.add_relay("wss://relay.example.com").await.is_ok());
    }
}
