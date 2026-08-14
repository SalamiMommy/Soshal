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
