pub fn can_send_to_channel(role: u8, min_role: u8) -> bool {
    role >= min_role
}

pub fn is_admin(role: u8) -> bool {
    role >= 3
}
