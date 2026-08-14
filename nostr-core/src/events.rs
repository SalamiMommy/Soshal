use nostr::event::{Event, EventBuilder, FinalizeEvent, Kind, Tag};
use nostr::filter::Filter;
use nostr::key::{Keys, PublicKey};

pub fn text_note(keys: &Keys, content: &str, tags: Vec<Tag>) -> Result<Event, nostr::error::Error> {
    let mut builder = EventBuilder::new(Kind::TextNote, content);
    for tag in tags {
        builder = builder.tag(tag);
    }
    builder.finalize(keys)
}

pub fn filter() -> Filter {
    Filter::new()
}

pub fn filter_kinds(kinds: Vec<Kind>) -> Filter {
    Filter::new().kinds(kinds)
}

pub fn filter_authors(pubkeys: Vec<PublicKey>) -> Filter {
    Filter::new().authors(pubkeys)
}
