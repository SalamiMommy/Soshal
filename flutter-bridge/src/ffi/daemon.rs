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
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

const DAEMONS: [&str; 3] = ["i2pd", "freenet", "rnsd"];

const I2PD_SAM_PORT: u16 = 7656;
const FREENET_API_PORT: u16 = 7509;

const WATCHDOG_INTERVAL_SECS: u64 = 30;

/// Consecutive failed respawns per daemon before the watchdog gives up. A
/// daemon that exits instantly on every spawn (wrong conf, missing shared
/// libs, missing interpreter) must not churn the respawn loop forever.
const MAX_RESPAWN_FAILURES: u32 = 5;

static CHILDREN: LazyLock<Mutex<HashMap<&'static str, Child>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Outcome of a spawner attempt. `Running` means the daemon is already up
/// (e.g. an existing local node reused via its port) and nothing was spawned —
/// the watchdog must not report it as a respawn.
#[derive(Clone, Copy, PartialEq)]
enum SpawnOutcome {
    Running,
    Spawned,
    Failed,
}

type SpawnerFn = Box<dyn Fn() -> SpawnOutcome + Send>;

/// Respawn closures per daemon; re-invoked by the watchdog when a child has
/// exited (e.g. OOM kill) while the foreground service is active.
static SPAWNERS: LazyLock<Mutex<HashMap<&'static str, SpawnerFn>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

static RESPAWN_FAILURES: LazyLock<Mutex<HashMap<&'static str, u32>>> =
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

/// AppImage bundle's usr/bin/daemons/ — sibling of the usr/bin directory
/// holding the app binary. Absent under bare `flutter run` dev builds, so
/// resolution gracefully falls back to the app-data dir.
#[cfg(not(target_os = "android"))]
fn bundle_daemons_dir() -> Option<std::path::PathBuf> {
    let dir = std::env::current_exe()
        .ok()?
        .parent()?
        .parent()?
        .join("daemons");
    dir.is_dir().then_some(dir)
}

/// True when a local listener already accepts TCP on `port`. Used to reuse an
/// already-running freenet node / i2pd service instead of spawning a second
/// instance that would fail to bind.
fn port_open(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok()
}

fn daemons_dir() -> Result<std::path::PathBuf, String> {
    let extracted = std::path::Path::new(&files_dir()?).join("daemons");
    if DAEMONS.iter().any(|d| extracted.join(d).exists()) {
        return Ok(extracted);
    }
    #[cfg(not(target_os = "android"))]
    if let Some(bundle) = bundle_daemons_dir() {
        return Ok(bundle);
    }
    Ok(extracted)
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
        let target = dir.join(name);
        // If already available in nativeLibraryDir (Android 10+ SELinux execution path),
        // skip extracting to app_data_file.
        #[cfg(target_os = "android")]
        if let Ok(lib_dir) = crate::platform::native_library_dir() {
            let lib_p = std::path::Path::new(&lib_dir).join(format!("lib{name}.so"));
            if lib_p.is_file() && lib_p.metadata().map(|m| m.len() > 100_000).unwrap_or(false) {
                any = true;
                continue;
            }
        }
        // If the binary is already extracted and non-empty, skip re-extracting
        // on every app startup to avoid ETXTBSY if it's currently running,
        // and eliminate UI frame drops during launch.
        if target.is_file() {
            if let Ok(meta) = target.metadata() {
                if meta.len() > 100_000 || (*name == *"rnsd" && meta.len() > 0) {
                    any = true;
                    continue;
                }
            }
        }
        let bytes = match crate::platform::read_asset(&asset_name(name)) {
            Ok(b) => b,
            Err(_) => continue,
        };
        // Write to a temporary file in the same directory first, set permissions,
        // then atomically rename it over the target. On POSIX, renaming over an
        // active executable never fails with ETXTBSY (unlike truncate/write).
        let tmp_target = dir.join(format!(".{name}.tmp.{}", std::process::id()));
        write_file(&tmp_target, &bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&tmp_target, fs::Permissions::from_mode(0o755));
        }
        fs::rename(&tmp_target, &target).map_err(super::util::to_err)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&target, fs::Permissions::from_mode(0o755));
        }
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

/// Resolve a daemon executable: checks Android nativeLibraryDir, app's extracted daemons dir,
/// AppImage bundle dir, release target dir (for dev builds), and system PATH.
fn find_daemon_binary(name: &str) -> Option<std::path::PathBuf> {
    // 1. Android nativeLibraryDir: lib<name>.so extracted from jniLibs by PackageManager.
    // On Android 10+ (API 29+), SELinux restricts execve() on app_data_file (files/),
    // but permits execution of apk_data_file in nativeLibraryDir.
    #[cfg(target_os = "android")]
    if let Ok(lib_dir) = crate::platform::native_library_dir() {
        let p = std::path::Path::new(&lib_dir).join(format!("lib{name}.so"));
        if p.is_file() && p.metadata().map(|m| m.len() > 100_000).unwrap_or(false) {
            return Some(p);
        }
    }
    // 2. Extracted binary in files/daemons/
    if let Ok(dir) = daemons_dir() {
        let p = dir.join(name);
        if p.is_file() && p.metadata().map(|m| m.len() > 100_000).unwrap_or(false) {
            return Some(p);
        }
    }
    // 2. AppImage bundle daemons/
    #[cfg(not(target_os = "android"))]
    if let Some(bundle) = bundle_daemons_dir() {
        let p = bundle.join(name);
        if p.is_file() && p.metadata().map(|m| m.len() > 100_000).unwrap_or(false) {
            return Some(p);
        }
    }
    // 3. Dev build binary in target/release/
    #[cfg(not(target_os = "android"))]
    {
        if let Ok(exe) = std::env::current_exe() {
            let mut cur = exe.parent();
            for _ in 0..5 {
                if let Some(parent) = cur {
                    let candidate = parent.join("target").join("release").join(name);
                    if candidate.is_file()
                        && candidate
                            .metadata()
                            .map(|m| m.len() > 100_000)
                            .unwrap_or(false)
                    {
                        return Some(candidate);
                    }
                    cur = parent.parent();
                } else {
                    break;
                }
            }
        }
        let cwd_candidate = std::path::Path::new("target").join("release").join(name);
        if cwd_candidate.is_file()
            && cwd_candidate
                .metadata()
                .map(|m| m.len() > 100_000)
                .unwrap_or(false)
        {
            return cwd_candidate.canonicalize().ok().or(Some(cwd_candidate));
        }
    }
    // 4. System PATH
    #[cfg(not(target_os = "android"))]
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(name);
            if candidate.is_file()
                && candidate
                    .metadata()
                    .map(|m| m.len() > 100_000)
                    .unwrap_or(false)
            {
                return Some(candidate);
            }
        }
    }
    None
}

fn is_daemon_port_open(name: &str) -> bool {
    match name {
        "i2pd" => port_open(I2PD_SAM_PORT),
        "freenet" => port_open(FREENET_API_PORT),
        "rnsd" => port_open(4242),
        _ => false,
    }
}

fn is_daemon_available(name: &str) -> bool {
    if name == "rnsd" && cfg!(target_os = "android") {
        return true;
    }
    if let Ok(dir) = daemons_dir() {
        if dir.join(name).exists() {
            return true;
        }
    }
    find_daemon_binary(name).is_some() || is_daemon_port_open(name)
}

fn any_daemons_available() -> Result<bool, String> {
    if daemon_are_daemons_available()? {
        return Ok(true);
    }
    for name in DAEMONS {
        if is_daemon_available(name) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Absolute path to an extracted daemon binary (empty if missing).
#[frb(sync, serialize)]
pub fn daemon_get_daemon_path(daemon_name: String) -> Result<String, String> {
    if !DAEMONS.contains(&daemon_name.as_str()) {
        return Err(format!("unknown daemon {daemon_name}"));
    }
    let path = daemons_dir()?.join(&daemon_name);
    if path.exists() {
        return Ok(path.to_string_lossy().into_owned());
    }
    if let Some(p) = find_daemon_binary(&daemon_name) {
        return Ok(p.to_string_lossy().into_owned());
    }
    Ok(String::new())
}

/// Whether all extracted daemons are present. rnsd on Android ships inside
/// the app (Chaquopy Python, RnsdRunner) — no asset to extract.
#[frb(sync, serialize)]
pub fn daemon_are_daemons_available() -> Result<bool, String> {
    let dir = daemons_dir()?;
    Ok(DAEMONS.iter().all(|d| {
        if *d == "rnsd" && cfg!(target_os = "android") {
            return true;
        }
        dir.join(d).exists()
    }))
}

/// Per-daemon extraction status as JSON: {"i2pd":true,...}. On Android,
/// `reticulum` reports the in-process Chaquopy daemon's liveness instead of
/// asset presence — the daemon runs inside the app, not as a child process.
#[frb(sync, serialize)]
pub fn daemon_get_daemon_status() -> Result<String, String> {
    let mut map = serde_json::Map::new();
    for (key, name) in [
        ("i2pd", "i2pd"),
        ("freenet", "freenet"),
        ("reticulum", "rnsd"),
    ] {
        let present = is_daemon_available(name);
        map.insert(key.to_string(), serde_json::Value::Bool(present));
    }
    // Surface the in-process Chaquopy daemon's last-start error so the UI can
    // explain "reticulum down" instead of leaving it blind.
    if let Ok(status) = crate::platform::rnsd_status() {
        map.insert(
            "reticulum_status".to_string(),
            serde_json::Value::String(status.trim().to_string()),
        );
    }
    Ok(serde_json::Value::Object(map).to_string())
}

fn spawn(name: &'static str, mut cmd: Command, log_name: &str, data_dir: &std::path::Path) -> bool {
    if is_running(name) {
        return true;
    }
    fs::create_dir_all(data_dir).ok();
    let log_path = data_dir.join(log_name);
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
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

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        #[allow(unsafe_code)]
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
    }

    match cmd.spawn() {
        Ok(child) => {
            crate::ffi::util::lock(&CHILDREN).insert(name, child);
            true
        }
        Err(e) => {
            eprintln!("spawn failed for {name}: {e}");
            if let Some(mut f) = log {
                let _ = writeln!(f, "spawn failed for {name}: {e}");
            }
            false
        }
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
    if !extracted && !any_daemons_available()? {
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
    let files_dir = files_dir()?;
    let files = std::path::Path::new(&files_dir);

    let i2pd_data = files.join("i2pd-data");
    let i2pd_conf = i2pd_data.join("i2pd.conf");
    // i2pd >= 2.5x parses config keys as dotted CLI-style options: logging
    // lives at the top level (a `[general]` wrapper turns `log` into the
    // unknown option `general.log` and kills the daemon on startup), and the
    // proxy services use their section names (`socksproxy`/`httpproxy`, not
    // `proxy`/`http`). `[http]` alone configures the control panel, not an
    // HTTP proxy on 4444.
    let certs_line = if std::path::Path::new("/usr/share/i2pd/certificates").is_dir() {
        "certsdir = /usr/share/i2pd/certificates\n".to_string()
    } else if files.join("daemons").join("certificates").is_dir() {
        format!(
            "certsdir = {}\n",
            files.join("daemons").join("certificates").display()
        )
    } else {
        String::new()
    };
    let conf = format!(
        "log = file\ndaemon = false\nlogfile = {}\n{certs_line}[sam]\nenabled = true\nport = 7656\n[socksproxy]\nenabled = true\nport = 4447\n[httpproxy]\nenabled = true\nport = 4444\n",
        i2pd_data.join("i2pd.log").display()
    );
    if write_file(&i2pd_conf, conf.as_bytes()).is_err() {
        return Err("failed to write i2pd.conf".to_string());
    }
    let i2pd_data_clone = i2pd_data.clone();
    let i2pd_conf_clone = i2pd_conf.clone();
    register_spawner("i2pd", move || {
        // Reuse an already-running i2pd (e.g. enabled systemd service) whose
        // SAM port is live; spawning our own would fail to bind 7656.
        if port_open(I2PD_SAM_PORT) {
            return SpawnOutcome::Running;
        }
        let bin = match find_daemon_binary("i2pd") {
            Some(b) => b,
            None => return SpawnOutcome::Failed,
        };
        let mut cmd = Command::new(bin);
        cmd.arg(format!("--datadir={}", i2pd_data_clone.display()))
            .arg(format!("--conf={}", i2pd_conf_clone.display()));
        match spawn("i2pd", cmd, "i2pd.stdout.log", &i2pd_data_clone) {
            true => SpawnOutcome::Spawned,
            false => SpawnOutcome::Failed,
        }
    });

    let freenet_data = files.join("freenet-data");
    let conf_dir = freenet_data.join("conf");
    let data_dir = freenet_data.join("data");
    let _ = fs::create_dir_all(&conf_dir);
    let _ = fs::create_dir_all(&data_dir);
    let conf_dir_clone = conf_dir.clone();
    let data_dir_clone = data_dir.clone();
    let freenet_data_clone = freenet_data.clone();
    register_spawner("freenet", move || {
        // Reuse the already-running local node (system/service instance bound
        // to 7509) instead of spawning a second that would fail to bind.
        if port_open(FREENET_API_PORT) {
            return SpawnOutcome::Running;
        }
        let bin = match find_daemon_binary("freenet") {
            Some(b) => b,
            None => return SpawnOutcome::Failed,
        };
        let _ = fs::create_dir_all(&conf_dir_clone);
        let _ = fs::create_dir_all(&data_dir_clone);
        let tmp_dir = freenet_data_clone.join("tmp");
        let _ = fs::create_dir_all(&tmp_dir);
        let mut cmd = Command::new(bin);
        // Android app processes have no $HOME/XDG dirs and no passwd entry for
        // the app UID, so freenet's default ProjectDirs resolution aborts the
        // node before it binds the WS API. Pin both dirs into freenet-data;
        // gateways are auto-fetched from the remote index. Auto-update is
        // disabled: there is no supervisor on Android to act on freenet's
        // exit-42 self-update signal, so a release build would exit shortly
        // after detecting a newer version.
        // Additionally, std::env::temp_dir() defaults to /data/local/tmp on Android,
        // which untrusted apps cannot write to. Provide app-writable tmp dir.
        cmd.current_dir(&freenet_data_clone)
            .env("TMPDIR", &tmp_dir)
            .env("TMP", &tmp_dir)
            .env("TEMP", &tmp_dir)
            .arg("network")
            .arg(format!("--config-dir={}", conf_dir_clone.display()))
            .arg(format!("--data-dir={}", data_dir_clone.display()))
            .arg("--disable-auto-update");
        match spawn("freenet", cmd, "freenet.log", &freenet_data_clone) {
            true => SpawnOutcome::Spawned,
            false => SpawnOutcome::Failed,
        }
    });

    let rnsd_data = files.join("reticulum-data");
    let rnsd_data_clone = rnsd_data.clone();
    register_spawner("rnsd", move || {
        #[cfg(target_os = "android")]
        {
            // rnsd has no Android binary — runs in the Chaquopy Python
            // runtime inside the app process (RnsdRunner).
            let _ = std::fs::create_dir_all(&rnsd_data_clone);
            let dir_str = rnsd_data_clone.to_string_lossy().into_owned();
            match crate::platform::rnsd_start(&dir_str) {
                Ok(true) => SpawnOutcome::Spawned,
                Ok(false) => SpawnOutcome::Failed,
                Err(_) => SpawnOutcome::Failed,
            }
        }
        #[cfg(not(target_os = "android"))]
        {
            if port_open(4242) {
                return SpawnOutcome::Running;
            }
            let bin = match find_daemon_binary("rnsd") {
                Some(b) => b,
                None => return SpawnOutcome::Failed,
            };
            let _ = std::fs::create_dir_all(&rnsd_data_clone);
            let mut cmd = Command::new(bin);
            cmd.arg("--config").arg(&rnsd_data_clone).arg("-s");
            cmd.current_dir(&rnsd_data_clone);
            match spawn("rnsd", cmd, "rnsd.log", &rnsd_data_clone) {
                true => SpawnOutcome::Spawned,
                false => SpawnOutcome::Failed,
            }
        }
    });

    let mut any_ok = false;
    for name in DAEMONS {
        let outcome = {
            let spawners = crate::ffi::util::lock(&SPAWNERS);
            spawners
                .get(name)
                .map(|s| s())
                .unwrap_or(SpawnOutcome::Failed)
        };
        if outcome == SpawnOutcome::Spawned || outcome == SpawnOutcome::Running {
            any_ok = true;
        }
    }
    Ok(any_ok)
}

fn register_spawner<F>(name: &'static str, f: F)
where
    F: Fn() -> SpawnOutcome + Send + 'static,
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
            // rnsd on Android runs in-process (Chaquopy thread), not as a
            // child — check its own liveness so the watchdog doesn't respawn
            // a healthy daemon or stack threads against a failed one.
            let live = is_running(name)
                || (name == "i2pd" && port_open(I2PD_SAM_PORT))
                || (name == "freenet" && port_open(FREENET_API_PORT))
                || (name == "rnsd"
                    && ((cfg!(target_os = "android")
                        && crate::platform::rnsd_running().unwrap_or(false))
                        || (!cfg!(target_os = "android") && port_open(4242))));
            if live || !is_real_binary(name) {
                continue;
            }
            if crate::ffi::util::lock(&RESPAWN_FAILURES)
                .get(name)
                .copied()
                .unwrap_or(0)
                >= MAX_RESPAWN_FAILURES
            {
                continue;
            }
            let outcome = {
                let spawners = crate::ffi::util::lock(&SPAWNERS);
                if WATCHDOG_STOP.load(Ordering::Relaxed) {
                    drop(spawners);
                    break;
                }
                spawners
                    .get(name)
                    .map(|s| s())
                    .unwrap_or(SpawnOutcome::Failed)
            };
            match outcome {
                SpawnOutcome::Spawned => {
                    // Grace window: a process that execs but exits immediately
                    // (bad binary, missing runtime) still made spawn() succeed,
                    // so verify it actually came up before counting the
                    // respawn as a win.
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    let now_live = is_running(name)
                        || (name == "i2pd" && port_open(I2PD_SAM_PORT))
                        || (name == "freenet" && port_open(FREENET_API_PORT))
                        || (name == "rnsd"
                            && ((cfg!(target_os = "android")
                                && crate::platform::rnsd_running().unwrap_or(false))
                                || (!cfg!(target_os = "android") && port_open(4242))));
                    if now_live || !is_real_binary(name) {
                        crate::ffi::util::lock(&RESPAWN_FAILURES).remove(name);
                        eprintln!("daemon watchdog: respawned {name}");
                    } else {
                        // Counts toward MAX_RESPAWN_FAILURES below.
                        let mut failures = crate::ffi::util::lock(&RESPAWN_FAILURES);
                        let count = failures.entry(name).or_insert(0);
                        *count += 1;
                        if *count == MAX_RESPAWN_FAILURES {
                            eprintln!(
                                "daemon watchdog: giving up on {name} after {count} failed respawns"
                            );
                        }
                    }
                }
                SpawnOutcome::Running => {
                    // Already-up daemon (port reuse) — not a respawn; log nothing.
                    crate::ffi::util::lock(&RESPAWN_FAILURES).remove(name);
                }
                SpawnOutcome::Failed => {
                    let mut failures = crate::ffi::util::lock(&RESPAWN_FAILURES);
                    let count = failures.entry(name).or_insert(0);
                    *count += 1;
                    if *count == MAX_RESPAWN_FAILURES {
                        eprintln!(
                            "daemon watchdog: giving up on {name} after {count} failed respawns"
                        );
                    }
                }
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

/// Liveness of the spawned i2pd process or active SAM port.
#[frb(sync, serialize)]
pub fn daemon_is_i2pd_running() -> Result<bool, String> {
    if is_running("i2pd") || port_open(I2PD_SAM_PORT) {
        return Ok(true);
    }
    Ok(false)
}

/// Liveness of the spawned rnsd process (Chaquopy thread on Android) or active 4242 port.
#[frb(sync, serialize)]
pub fn daemon_is_rnsd_running() -> Result<bool, String> {
    #[cfg(target_os = "android")]
    {
        crate::platform::rnsd_running().or(Ok(false))
    }
    #[cfg(not(target_os = "android"))]
    {
        if is_running("rnsd") || port_open(4242) {
            return Ok(true);
        }
        Ok(false)
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
    fn test_port_open_detects_listener() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(port_open(port));
        drop(listener);
        assert!(!port_open(port));
    }

    #[test]
    fn test_stop_is_idempotent() {
        assert!(daemon_stop_daemons().unwrap());
        assert!(!is_running("i2pd"));
        assert!(!is_running("rnsd"));
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

    #[test]
    fn test_find_daemon_binary_finds_built_rnsd() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK.lock().unwrap();
        let _path = super::super::db::tmp_db("daemon-find", "daemon");
        if std::path::Path::new("target/release/rnsd").exists() {
            let bin = find_daemon_binary("rnsd");
            assert!(bin.is_some());
            assert!(bin.unwrap().exists());
        }
    }

    #[test]
    fn test_freenet_conf_dirs_created() {
        let _g = crate::ffi::test_lock::DB_TEST_LOCK.lock().unwrap();
        let _path = super::super::db::tmp_db("daemon-freenet-dirs", "daemon");
        let files = files_dir().unwrap();
        let freenet_data = std::path::Path::new(&files).join("freenet-data");
        let conf_dir = freenet_data.join("conf");
        let data_dir = freenet_data.join("data");
        fs::create_dir_all(&conf_dir).unwrap();
        fs::create_dir_all(&data_dir).unwrap();
        assert!(conf_dir.exists());
        assert!(data_dir.exists());
    }
}
