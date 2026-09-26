//! Type-safe bitmask flags for messages, delivery states, and channel permissions.

use bitflags::bitflags;
use serde::{Deserialize, Serialize};

bitflags! {
    /// Bitmask flags tracking status, privacy, and metadata of messages.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
    #[serde(transparent)]
    pub struct MessageFlags: u32 {
        /// Message was sent by the local client.
        const SENT = 1 << 0;
        /// Message was confirmed delivered to at least one relay or peer.
        const DELIVERED = 1 << 1;
        /// Message was read / seen by the recipient.
        const READ = 1 << 2;
        /// Message is pinned in its conversation thread.
        const PINNED = 1 << 3;
        /// Message is end-to-end encrypted (e.g. NIP-44 or hybrid PQC).
        const ENCRYPTED = 1 << 4;
        /// Message is ephemeral / self-destructing.
        const EPHEMERAL = 1 << 5;
        /// Message contains attached media (image, audio, video).
        const HAS_MEDIA = 1 << 6;
        /// Message is a forwarded note or giftwrap.
        const FORWARDED = 1 << 7;
        /// Message has been edited or superseded by a newer version.
        const EDITED = 1 << 8;
        /// Message notifications are muted for this specific item.
        const MUTED = 1 << 9;
    }
}

impl MessageFlags {
    /// Check if marked as read.
    pub fn is_read(&self) -> bool {
        self.contains(Self::READ)
    }

    /// Check if marked as delivered.
    pub fn is_delivered(&self) -> bool {
        self.contains(Self::DELIVERED)
    }

    /// Check if encrypted.
    pub fn is_encrypted(&self) -> bool {
        self.contains(Self::ENCRYPTED)
    }

    /// Check if ephemeral.
    pub fn is_ephemeral(&self) -> bool {
        self.contains(Self::EPHEMERAL)
    }

    /// Convert active flags to a list of human-readable snake_case strings.
    pub fn to_string_list(&self) -> Vec<&'static str> {
        let mut list = Vec::new();
        if self.contains(Self::SENT) {
            list.push("sent");
        }
        if self.contains(Self::DELIVERED) {
            list.push("delivered");
        }
        if self.contains(Self::READ) {
            list.push("read");
        }
        if self.contains(Self::PINNED) {
            list.push("pinned");
        }
        if self.contains(Self::ENCRYPTED) {
            list.push("encrypted");
        }
        if self.contains(Self::EPHEMERAL) {
            list.push("ephemeral");
        }
        if self.contains(Self::HAS_MEDIA) {
            list.push("has_media");
        }
        if self.contains(Self::FORWARDED) {
            list.push("forwarded");
        }
        if self.contains(Self::EDITED) {
            list.push("edited");
        }
        if self.contains(Self::MUTED) {
            list.push("muted");
        }
        list
    }
}

bitflags! {
    /// Permissions bitmask for group chats and community channels.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
    #[serde(transparent)]
    pub struct ChannelPermissions: u32 {
        /// Can post normal text messages.
        const SEND_MESSAGES = 1 << 0;
        /// Can upload media attachments.
        const SEND_MEDIA = 1 << 1;
        /// Can pin/unpin messages.
        const PIN_MESSAGES = 1 << 2;
        /// Can delete/moderate messages from others.
        const DELETE_MESSAGES = 1 << 3;
        /// Can invite new members or generate invite links.
        const INVITE_MEMBERS = 1 << 4;
        /// Superadmin with all administrative rights.
        const ADMIN = 1 << 31;
    }
}

impl ChannelPermissions {
    /// Default permissions granted to standard channel participants.
    pub fn default_member() -> Self {
        Self::SEND_MESSAGES | Self::SEND_MEDIA | Self::INVITE_MEMBERS
    }

    /// Check if the member has administrator rights.
    pub fn is_admin(&self) -> bool {
        self.contains(Self::ADMIN)
    }

    /// Check if the member is permitted to delete messages.
    pub fn can_delete(&self) -> bool {
        self.contains(Self::ADMIN) || self.contains(Self::DELETE_MESSAGES)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_flags_bitwise_ops() {
        let mut flags = MessageFlags::empty();
        assert!(!flags.is_encrypted());
        assert!(!flags.is_read());

        flags |= MessageFlags::ENCRYPTED | MessageFlags::SENT;
        assert!(flags.is_encrypted());
        assert!(flags.contains(MessageFlags::SENT));
        assert!(!flags.is_read());

        flags.insert(MessageFlags::READ);
        assert!(flags.is_read());

        flags.remove(MessageFlags::SENT);
        assert!(!flags.contains(MessageFlags::SENT));

        let strings = flags.to_string_list();
        assert_eq!(strings, vec!["read", "encrypted"]);
    }

    #[test]
    fn test_message_flags_serde() {
        let flags = MessageFlags::ENCRYPTED | MessageFlags::DELIVERED;
        let serialized = serde_json::to_string(&flags).expect("serialize");
        assert_eq!(serialized, "\"DELIVERED | ENCRYPTED\"");

        let deserialized: MessageFlags = serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(deserialized, flags);
    }

    #[test]
    fn test_channel_permissions() {
        let member = ChannelPermissions::default_member();
        assert!(member.contains(ChannelPermissions::SEND_MESSAGES));
        assert!(member.contains(ChannelPermissions::SEND_MEDIA));
        assert!(!member.is_admin());
        assert!(!member.can_delete());

        let admin = member | ChannelPermissions::ADMIN;
        assert!(admin.is_admin());
        assert!(admin.can_delete());
    }
}
