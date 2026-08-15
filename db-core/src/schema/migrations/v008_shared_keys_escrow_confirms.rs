//! Migration 8: shared group keys table + escrow buyer/seller confirmation
//! flags. Keys are stored hex-encoded for FFI symmetry with signer surfaces;
//! confirmation columns default to 0 (unconfirmed) so existing escrows stay
//! pending until both parties confirm.

use libsql::Connection;

pub fn v8_shared_keys_escrow_confirms(conn: &Connection) -> Result<(), libsql::Error> {
    crate::block_on(conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS group_shared_keys (
            group_id TEXT PRIMARY KEY,
            key_hex TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        );

        ALTER TABLE escrows ADD COLUMN buyer_confirmed INTEGER NOT NULL DEFAULT 0;
        ALTER TABLE escrows ADD COLUMN seller_confirmed INTEGER NOT NULL DEFAULT 0;

        INSERT OR IGNORE INTO _migrations (version) VALUES (8);
    ",
    ))?;
    Ok(())
}
