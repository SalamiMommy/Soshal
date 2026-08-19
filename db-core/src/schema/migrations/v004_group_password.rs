//! Migration 4: password protection for private groups / communities.

use libsql::Connection;

pub fn v4_group_password(conn: &Connection) -> Result<(), libsql::Error> {
    crate::block_on(conn.execute_batch(
        "
        ALTER TABLE groups ADD COLUMN password_hash TEXT;

        INSERT OR IGNORE INTO _migrations (version) VALUES (4);
        ",
    ))?;
    Ok(())
}
