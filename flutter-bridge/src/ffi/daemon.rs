//! Daemon FFI module
//! Bundled networking daemon lifecycle (I2P, Freenet, Reticulum): asset
//! extraction, spawn, stop, liveness. Owns spawned child processes.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

use flutter_rust_bridge::frb;

const DAEMONS: [&str; 3] = ["i2pd", "freenet", "rnsd"];

const WATCHDOG_INTERVAL_SECS: u64 = 30;

static CHILDREN: LazyLock<Mutex<HashMap<&'static str, Child>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

type SpawnerFn = Box<dyn Fn() -> bool + Send>;

/// Respawn closures per daemon; re-invoked by the watchdog when a child has
/// exited (e.g. OOM kill) while the foreground service is active.
static SPAWNERS: LazyLock<Mutex<HashMap<&'static str, SpawnerFn>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

static WATCHDOG_ACTIVE: AtomicBool = AtomicBool::new(false);
static WATCHDOG_STOP: AtomicBool = AtomicBool::new(false);

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
        let bytes = match crate::platform::read_asset(&asset_name(name)) {
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

/// Asset path for a daemon binary: i2pd is bundled per-ABI on Android
/// (build.sh packs all three arch statics under assets/daemons/<abi>/), the
/// others live at assets/daemons/<name>.
fn asset_name(name: &'static str) -> String {
    #[cfg(target_os = "android")]
    if name == "i2pd" {
        if let Ok(abi) = crate::platform::supported_abi() {
            return format!("daemons/{abi}/i2pd");
        }
    }
    format!("daemons/{name}")
}

/// Absolute path to an extracted daemon binary (empty if missing).
#[frb(sync, serialize)]
pub fn daemon_get_daemon_path(daemon_name: String) -> Result<String, String> {
    if !DAEMONS.contains(&daemon_name.as_str()) {
        return Err(format!("unknown daemon {daemon_name}"));
    }
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
            crate::ffi::util::lock(&CHILDREN).insert(name, child);
            true
        }
        Err(_) => false,
    }
}

fn is_running(name: &'static str) -> bool {
    crate::ffi::util::lock(&CHILDREN)
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
    let ok = spawn_all()?;
    // Keep the process (and the daemon children) alive while the app is
    // backgrounded. No-op off-Android.
    let _ = crate::platform::daemon_service_start();
    WATCHDOG_STOP.store(false, Ordering::Relaxed);
    if !WATCHDOG_ACTIVE.swap(true, Ordering::Relaxed) {
        std::thread::Builder::new()
            .name("daemon-watchdog".to_string())
            .spawn(watchdog_loop)
            .map_err(|e| format!("watchdog spawn failed: {e}"))?;
    }
    Ok(ok)
}

/// Whether a daemon binary is a real bundle rather than a build-time stub
/// (build.sh writes shell stubs <100KB when no binary is available). rnsd
/// on Android runs in the Chaquopy Python runtime — always "real".
fn is_real_binary(name: &str) -> bool {
    #[cfg(target_os = "android")]
    if name == "rnsd" {
        return true;
    }
    let path = daemons_dir().ok().map(|d| d.join(name));
    match path {
        Some(p) => p.metadata().map(|m| m.len() > 100_000).unwrap_or(false),
        None => false,
    }
}

fn spawn_all() -> Result<bool, String> {
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
        return Err("failed to write i2pd.conf".to_string());
    }
    let dir_i2pd = dir.clone();
    register_spawner("i2pd", move || {
        let mut cmd = Command::new(dir_i2pd.join("i2pd"));
        cmd.arg(format!("--datadir={}", i2pd_data.display()))
            .arg(format!("--conf={}", i2pd_conf.display()));
        spawn("i2pd", cmd, "i2pd.stdout.log", &i2pd_data)
    });

    let freenet_data = files.join("freenet-data");
    let dir_freenet = dir.clone();
    register_spawner("freenet", move || {
        let mut cmd = Command::new(dir_freenet.join("freenet"));
        cmd.current_dir(&freenet_data);
        spawn("freenet", cmd, "freenet.log", &freenet_data)
    });

    let rnsd_data = files.join("reticulum-data");
    #[cfg_attr(target_os = "android", allow(unused_variables))]
    let dir_rnsd = dir;
    register_spawner("rnsd", move || {
        #[cfg(target_os = "android")]
        {
            // rnsd has no Android binary — runs in the Chaquopy Python
            // runtime inside the app process (RnsdRunner).
            let _ = std::fs::create_dir_all(&rnsd_data);
            let dir_str = rnsd_data.to_string_lossy().into_owned();
            crate::platform::rnsd_start(&dir_str).unwrap_or(false)
        }
        #[cfg(not(target_os = "android"))]
        {
            let mut cmd = Command::new(dir_rnsd.join("rnsd"));
            cmd.current_dir(&rnsd_data);
            spawn("rnsd", cmd, "rnsd.log", &rnsd_data)
        }
    });

    let mut all_ok = true;
    for name in DAEMONS {
        let spawned = {
            let spawners = crate::ffi::util::lock(&SPAWNERS);
            spawners.get(name).map(|s| s()).unwrap_or(false)
        };
        if !spawned {
            all_ok = false;
        }
    }
    Ok(all_ok)
}

fn register_spawner<F>(name: &'static str, f: F)
where
    F: Fn() -> bool + Send + 'static,
{
    crate::ffi::util::lock(&SPAWNERS).insert(name, Box::new(f));
}

/// Periodic liveness sweep: respawn exited daemons while the foreground
/// service holds the process alive.
fn watchdog_loop() {
    while !WATCHDOG_STOP.load(Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_secs(WATCHDOG_INTERVAL_SECS));
        if WATCHDOG_STOP.load(Ordering::Relaxed) {
            break;
        }
        let daemons: Vec<&'static str> = DAEMONS.to_vec();
        for name in daemons {
            if WATCHDOG_STOP.load(Ordering::Relaxed) {
                break;
            }
            if is_running(name) || !is_real_binary(name) {
                continue;
            }
            let spawned = {
                let spawners = crate::ffi::util::lock(&SPAWNERS);
                if WATCHDOG_STOP.load(Ordering::Relaxed) {
                    drop(spawners);
                    break;
                }
                spawners.get(name).map(|s| s()).unwrap_or(false)
            };
            if spawned {
                eprintln!("daemon watchdog: respawned {name}");
            }
        }
    }
    WATCHDOG_ACTIVE.store(false, Ordering::Relaxed);
}

/// Kill any spawned daemons and stop the foreground service.
#[frb(sync, serialize)]
pub fn daemon_stop_daemons() -> Result<bool, String> {
    WATCHDOG_STOP.store(true, Ordering::Relaxed);
    let _ = crate::platform::daemon_service_stop();
    #[cfg(target_os = "android")]
    let _ = crate::platform::rnsd_stop();
    let mut children = crate::ffi::util::lock(&CHILDREN);
    for (_, child) in children.iter_mut() {
        child.kill().ok();
        child.wait().ok();
    }
    children.clear();
    Ok(true)
}

/// Whether the daemon foreground service is active (Android). Off-Android
/// this reports false and is a no-op.
#[frb(sync, serialize)]
pub fn daemon_service_running() -> Result<bool, String> {
    crate::platform::daemon_service_running().or(Ok(false))
}

/// Fire the OS "ignore battery optimizations" dialog for this app, so OEM
/// battery managers don't kill the daemon foreground service. Off-Android
/// returns false.
#[frb(sync, serialize)]
pub fn daemon_request_battery_exemption() -> Result<bool, String> {
    match crate::platform::request_ignore_battery_optimizations() {
        Ok(()) => Ok(true),
        Err(_) => Ok(false),
    }
}

/// Liveness of the spawned i2pd process.
#[frb(sync, serialize)]
pub fn daemon_is_i2pd_running() -> Result<bool, String> {
    Ok(crate::ffi::util::lock(&CHILDREN)
        .get_mut("i2pd")
        .map(|c| c.try_wait().ok().map(|s| s.is_none()).unwrap_or(false))
        .unwrap_or(false))
}

/// Liveness of the spawned rnsd process (Chaquopy thread on Android).
#[frb(sync, serialize)]
pub fn daemon_is_rnsd_running() -> Result<bool, String> {
    #[cfg(target_os = "android")]
    {
        crate::platform::rnsd_running().or(Ok(false))
    }
    #[cfg(not(target_os = "android"))]
    {
        Ok(crate::ffi::util::lock(&CHILDREN)
            .get_mut("rnsd")
            .map(|c| c.try_wait().ok().map(|s| s.is_none()).unwrap_or(false))
            .unwrap_or(false))
    }
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

    #[test]
    fn test_service_running_off_android() {
        // Off-Android the JNI stub reports false; the ffi fn must not Err.
        if cfg!(target_os = "android") {
            return;
        }
        assert!(!daemon_service_running().unwrap());
        assert!(!daemon_request_battery_exemption().unwrap());
    }

    #[test]
    fn test_is_real_binary_stub_heuristic() {
        // files/daemons doesn't exist in the test env → no binary → stub.
        let _g = crate::ffi::test_lock::DB_TEST_LOCK.lock().unwrap();
        let _path = super::super::db::tmp_db("daemon-real", "daemon");
        assert!(!is_real_binary("i2pd"));
        assert!(!is_real_binary("freenet"));
        assert!(!is_real_binary("rnsd"));
    }
}
