use crate::Database;
use libsql::params;

pub struct ProfileNodeRepo<'a> {
    db: &'a Database,
}

impl<'a> ProfileNodeRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn upsert(&self, n: &ProfileNodeRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT INTO custom_profile_nodes (id, user_pubkey, type, styles, properties, layout_row, layout_col, sort_order) VALUES (?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(id) DO UPDATE SET styles=excluded.styles, properties=excluded.properties, layout_row=excluded.layout_row, layout_col=excluded.layout_col, sort_order=excluded.sort_order",
            params![
                n.id.as_str(),
                n.user_pubkey.as_str(),
                n.node_type.as_str(),
                n.styles.as_str(),
                n.properties.as_str(),
                n.layout_row,
                n.layout_col,
                n.sort_order
            ],
        )?;
        Ok(())
    }

    pub fn list_by_user(
        &self,
        user_pubkey: &str,
    ) -> Result<Vec<ProfileNodeRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT id, user_pubkey, type, styles, properties, layout_row, layout_col, sort_order FROM custom_profile_nodes WHERE user_pubkey=?1 ORDER BY sort_order ASC, layout_row ASC",
            params![user_pubkey],
            Self::map_row,
        )
    }

    pub fn delete(&self, id: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM custom_profile_nodes WHERE id=?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn delete_all_for(&self, user_pubkey: &str) -> Result<u64, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "DELETE FROM custom_profile_nodes WHERE user_pubkey=?1",
            params![user_pubkey],
        )
    }

    fn map_row(r: &libsql::Row) -> libsql::Result<ProfileNodeRow> {
        Ok(ProfileNodeRow {
            id: r.get(0)?,
            user_pubkey: r.get(1)?,
            node_type: r.get(2)?,
            styles: r.get(3)?,
            properties: r.get(4)?,
            layout_row: r.get(5)?,
            layout_col: r.get(6)?,
            sort_order: r.get(7)?,
        })
    }
}

pub struct ProfileNodeRow {
    pub id: String,
    pub user_pubkey: String,
    pub node_type: String,
    pub styles: String,
    pub properties: String,
    pub layout_row: i64,
    pub layout_col: i64,
    pub sort_order: i64,
}
