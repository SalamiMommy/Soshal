use libsql::params;
use serde::{Deserialize, Serialize};

use crate::Database;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupRoleRow {
    pub id: String,
    pub group_id: String,
    pub name: String,
    pub color: String,
    pub position: i64,
    pub permissions: String,
    pub created_at: i64,
}

pub struct GroupRoleRepo<'a> {
    db: &'a Database,
}

impl<'a> GroupRoleRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn list(&self, group_id: &str) -> Result<Vec<GroupRoleRow>, crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::query(
            &conn,
            "SELECT id, group_id, name, color, position, permissions, created_at
             FROM group_roles WHERE group_id = ?1 ORDER BY position ASC",
            [group_id],
            |row| {
                Ok(GroupRoleRow {
                    id: row.get(0)?,
                    group_id: row.get(1)?,
                    name: row.get(2)?,
                    color: row.get(3)?,
                    position: row.get(4)?,
                    permissions: row.get(5)?,
                    created_at: row.get(6)?,
                })
            },
        )
    }

    pub fn upsert(&self, role: &GroupRoleRow) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "INSERT OR REPLACE INTO group_roles (id, group_id, name, color, position, permissions, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![role.id.as_str(), role.group_id.as_str(), role.name.as_str(), role.color.as_str(), role.position, role.permissions.as_str(), role.created_at],
        )?;
        Ok(())
    }

    pub fn delete(&self, role_id: &str) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(&conn, "DELETE FROM group_roles WHERE id = ?1", [role_id])?;
        Ok(())
    }

    /// Assigns a role id to a group member (role column stores the role id).
    pub fn assign_member_role(
        &self,
        group_id: &str,
        pubkey: &str,
        role_id: &str,
    ) -> Result<(), crate::error::DbError> {
        let conn = self.db.conn()?;
        crate::query::execute(
            &conn,
            "UPDATE group_members SET role = ?1 WHERE group_id = ?2 AND pubkey = ?3",
            params![role_id, group_id, pubkey],
        )?;
        Ok(())
    }
}
