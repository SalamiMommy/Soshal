//! Headless background sync module for Android WorkManager and iOS BGTaskScheduler.
//!
//! Executes directly from the background task runner without spinning up
//! the Flutter Engine or Dart VM.

#[flutter_rust_bridge::frb(sync, serialize)]
pub fn background_sync_task(db_path: String) -> Result<i32, String> {
    if db_path.is_empty() {
        return Err("Database path cannot be empty".to_string());
    }

    let db = soshal_db_core::Database::open(&db_path)
        .map_err(|e| format!("Failed to open DB for background sync: {}", e))?;

    db.migrate()
        .map_err(|e| format!("Failed to migrate DB during background sync: {}", e))?;

    Ok(0)
}
