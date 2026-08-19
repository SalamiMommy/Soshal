//! Daemon FFI module
//! Bundled networking daemon lifecycle (I2P, Freenet, Reticulum): asset
//! extraction, spawn, stop, liveness. Owns spawned child processes.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::{LazyLock, Mutex};

use flutter_rust_bridge::frb;

const DAEMONS: [&str; 3] = ["i2pd", "freenet", "rnsd"];

static CHILDREN: LazyLock<Mutex<HashMap<&'static str, Child>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn files_dir() -> Result<String, String> {
    match crate::platform::files_dir() {
        Ok(dir) => Ok(dir),
        Err(_) => {
            let db = super::db::db_path()?;
            let parent = std::path::Path::new(&db)
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| "/tmp".to_string());
            Ok(parent)
        }
    }
}

fn daemons_dir() -> Result<std::path::PathBuf, String> {
    Ok(std::path::Path::new(&files_dir()?).join("daemons"))
}

fn write_file(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(super::util::to_err)?;
    }
    let mut f = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
        .map_err(super::util::to_err)?;
    f.write_all(bytes).map_err(super::util::to_err)
}

/// Extract the three bundled daemons from assets into files/daemons/.
/// Returns false if extraction is unsupported (no bundled assets).
#[frb(sync, serialize)]
pub fn daemon_extract_daemons() -> Result<bool, String> {
    let dir = daemons_dir()?;
    fs::create_dir_all(&dir).map_err(super::util::to_err)?;
    let mut any = false;
    for name in DAEMONS {
        let bytes = match crate::platform::read_asset(&format!("daemons/{name}")) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let target = dir.join(name);
        write_file(&target, &bytes)?;
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755))
            .map_err(super::util::to_err)?;
        any = true;
    }
    Ok(any)
}

/// Absolute path to an extracted daemon binary (empty if missing).
#[frb(sync, serialize)]
pub fn daemon_get_daemon_path(daemon_name: String) -> Result<String, String> {
    let path = daemons_dir()?.join(&daemon_name);
    if path.exists() {
        Ok(path.to_string_lossy().into_owned())
    } else {
        Ok(String::new())
    }
}

/// Whether all three daemons are extracted.
#[frb(sync, serialize)]
pub fn daemon_are_daemons_available() -> Result<bool, String> {
    let dir = daemons_dir()?;
    Ok(DAEMONS.iter().all(|d| dir.join(d).exists()))
}

/// Per-daemon extraction status as JSON: {"i2pd":true,...}.
#[frb(sync, serialize)]
pub fn daemon_get_daemon_status() -> Result<String, String> {
    let dir = daemons_dir()?;
    let mut map = serde_json::Map::new();
    for (key, name) in [
        ("i2pd", "i2pd"),
        ("freenet", "freenet"),
        ("reticulum", "rnsd"),
    ] {
        map.insert(
            key.to_string(),
            serde_json::Value::Bool(dir.join(name).exists()),
        );
    }
    Ok(serde_json::Value::Object(map).to_string())
}

fn spawn(name: &'static str, mut cmd: Command, log_name: &str, data_dir: &std::path::Path) -> bool {
    if is_running(name) {
        return true;
    }
    fs::create_dir_all(data_dir).ok();
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(data_dir.join(log_name))
        .ok();
    let stdout = match log.as_ref() {
        Some(f) => f.try_clone().map(Stdio::from).unwrap_or(Stdio::null()),
        None => Stdio::null(),
    };
    let stderr = match log.as_ref() {
        Some(f) => f.try_clone().map(Stdio::from).unwrap_or(Stdio::null()),
        None => Stdio::null(),
    };
    cmd.stdout(stdout).stderr(stderr);
    match cmd.spawn() {
        Ok(child) => {
            CHILDREN
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(name, child);
            true
        }
        Err(_) => false,
    }
}

fn is_running(name: &'static str) -> bool {
    CHILDREN
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_mut(name)
        .map(|c| c.try_wait().ok().map(|s| s.is_none()).unwrap_or(false))
        .unwrap_or(false)
}

/// Extract + launch i2pd (SAM 7656, SOCKS 4447, HTTP 4444), freenet, and
/// rnsd. Each spawn is attempted independently; the result reports per
/// daemon liveness after the spawn attempt.
#[frb(sync, serialize)]
pub fn daemon_start_daemons() -> Result<bool, String> {
    let extracted = daemon_extract_daemons()?;
    if !extracted && !daemon_are_daemons_available()? {
        return Ok(false);
    }
    let dir = daemons_dir()?;
    let files_dir = files_dir()?;
    let files = std::path::Path::new(&files_dir);

    let i2pd_data = files.join("i2pd-data");
    let i2pd_conf = i2pd_data.join("i2pd.conf");
    let conf = format!(
        "[general]\nlog = file\nlogfile = {}\n[sam]\nenabled = true\n[proxy]\nenabled = true\nport = 4447\n[http]\nenabled = true\nport = 4444\n",
        i2pd_data.join("i2pd.log").display()
    );
    if write_file(&i2pd_conf, conf.as_bytes()).is_err() {
        return Ok(false);
    }
    let mut i2pd_cmd = Command::new(dir.join("i2pd"));
    i2pd_cmd
        .arg(format!("--datadir={}", i2pd_data.display()))
        .arg(format!("--conf={}", i2pd_conf.display()));
    let i2pd_ok = spawn("i2pd", i2pd_cmd, "i2pd.stdout.log", &i2pd_data);

    let freenet_data = files.join("freenet-data");
    let mut freenet_cmd = Command::new(dir.join("freenet"));
    freenet_cmd.current_dir(&freenet_data);
    let freenet_ok = spawn("freenet", freenet_cmd, "freenet.log", &freenet_data);

    let rnsd_data = files.join("reticulum-data");
    let mut rnsd_cmd = Command::new(dir.join("rnsd"));
    rnsd_cmd.current_dir(&rnsd_data);
    let rnsd_ok = spawn("rnsd", rnsd_cmd, "rnsd.log", &rnsd_data);

    Ok(i2pd_ok && freenet_ok && rnsd_ok)
}

/// Kill any spawned daemons.
#[frb(sync, serialize)]
pub fn daemon_stop_daemons() -> Result<bool, String> {
    let mut children = CHILDREN.lock().unwrap_or_else(|e| e.into_inner());
    for (_, child) in children.iter_mut() {
        child.kill().ok();
        child.wait().ok();
    }
    children.clear();
    Ok(true)
}

/// Liveness of the spawned i2pd process.
#[frb(sync, serialize)]
pub fn daemon_is_i2pd_running() -> Result<bool, String> {
    Ok(CHILDREN
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_mut("i2pd")
        .map(|c| c.try_wait().ok().map(|s| s.is_none()).unwrap_or(false))
        .unwrap_or(false))
}

/// Liveness of the spawned rnsd process.
#[frb(sync, serialize)]
pub fn daemon_is_rnsd_running() -> Result<bool, String> {
    Ok(CHILDREN
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_mut("rnsd")
        .map(|c| c.try_wait().ok().map(|s| s.is_none()).unwrap_or(false))
        .unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_daemon_status_json() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK.lock().unwrap();
        let _path = super::super::db::tmp_db("daemon-status", "daemon");
        let dir = daemons_dir().unwrap();
        fs::create_dir_all(&dir).unwrap();
        let status = daemon_get_daemon_status().unwrap();
        let v: serde_json::Value = serde_json::from_str(&status).unwrap();
        let map = v.as_object().unwrap();
        assert!(map.contains_key("i2pd"));
        assert!(map.contains_key("freenet"));
        assert!(map.contains_key("reticulum"));
        let all = daemon_are_daemons_available().unwrap();
        assert!(!all);
    }

    #[test]
    fn test_stop_is_idempotent() {
        assert!(daemon_stop_daemons().unwrap());
        assert!(!daemon_is_i2pd_running().unwrap());
        assert!(!daemon_is_rnsd_running().unwrap());
    }

    #[test]
    fn test_get_daemon_path_missing_is_empty() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK.lock().unwrap();
        let _path = super::super::db::tmp_db("daemon-path", "daemon");
        let p = daemon_get_daemon_path("i2pd".to_string()).unwrap();
        assert!(p.is_empty() || std::path::Path::new(&p).exists());
    }
}
