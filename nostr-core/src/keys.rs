use nostr::key::Keys;

pub fn generate_keys() -> Keys {
    Keys::generate()
}

pub fn from_sk(sk: &str) -> Result<Keys, nostr::error::Error> {
    Keys::parse(sk)
}

pub fn from_nsec(nsec: &str) -> Result<Keys, nostr::error::Error> {
    Keys::parse(nsec)
}
