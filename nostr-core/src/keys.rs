use nostr::key::Keys;

pub fn generate_keys() -> Keys {
    Keys::generate()
}

pub fn from_nsec(nsec: &str) -> Result<Keys, nostr::error::Error> {
    Keys::parse(nsec)
}
