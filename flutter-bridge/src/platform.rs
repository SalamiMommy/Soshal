//! Android platform bridge: JavaVM handle, MainActivity reference, native
//! asset reads (AAssetManager via libandroid). Stubbed on other platforms.
//! Shared foundation for daemon lifecycle, permissions, and power sampling.

#[cfg(target_os = "android")]
#[allow(unsafe_code)]
mod android {
    use std::ffi::c_void;
    use std::sync::Mutex;

    use jni::objects::{JObject, JString};
    use jni::{JNIEnv, JavaVM};

    const PLATFORM_BRIDGE_CLASS: &str = "com/example/soshal_flutter/PlatformBridge";
    const MAIN_ACTIVITY_FIELD: &str = "activity";
    const MAIN_ACTIVITY_SIG: &str = "Lcom/example/soshal_flutter/MainActivity;";

    type JniGetCreatedJavaVMs =
        unsafe extern "C" fn(*mut *mut jni::sys::JavaVM, i32, *mut i32) -> i32;

    static JAVA_VM: Mutex<Option<&'static JavaVM>> = Mutex::new(None);

    fn dlsym<T>(lib: &str, symbol: &str) -> Option<T> {
        unsafe {
            let handle = libc::dlopen(std::ffi::CString::new(lib).ok()?.as_ptr(), libc::RTLD_NOW);
            if handle.is_null() {
                return None;
            }
            let ptr = libc::dlsym(handle, std::ffi::CString::new(symbol).ok()?.as_ptr());
            if ptr.is_null() {
                return None;
            }
            Some(std::mem::transmute_copy(&ptr))
        }
    }

    fn find_get_created_vms() -> Option<JniGetCreatedJavaVMs> {
        for lib in ["libnativehelper.so", "libart.so"] {
            if let Some(f) = dlsym::<JniGetCreatedJavaVMs>(lib, "JNI_GetCreatedJavaVMs") {
                return Some(f);
            }
        }
        None
    }

    fn ensure_jvm() -> Result<&'static JavaVM, String> {
        if let Some(vm) = *JAVA_VM.lock().unwrap_or_else(|e| e.into_inner()) {
            return Ok(vm);
        }
        let get_vms =
            find_get_created_vms().ok_or("JNI_GetCreatedJavaVMs not found".to_string())?;
        let mut vm_ptr: *mut jni::sys::JavaVM = std::ptr::null_mut();
        let mut count: i32 = 0;
        let status = unsafe { get_vms(&mut vm_ptr, 1, &mut count) };
        if status != 0 || count < 1 || vm_ptr.is_null() {
            return Err("no created JavaVM".to_string());
        }
        let vm = unsafe { JavaVM::from_raw(vm_ptr) }.map_err(|e| e.to_string())?;
        let leaked: &'static JavaVM = Box::leak(Box::new(vm));
        *JAVA_VM.lock().unwrap_or_else(|e| e.into_inner()) = Some(leaked);
        Ok(leaked)
    }

    fn attach() -> Result<JNIEnv<'static>, String> {
        let vm = ensure_jvm()?;
        match vm.get_env() {
            Ok(env) => Ok(env),
            Err(_) => vm
                .attach_current_thread_permanently()
                .map_err(|e| e.to_string()),
        }
    }

    /// Android SDK level (Build.VERSION.SDK_INT). Codec features gate on 26.
    pub fn sdk_int() -> Result<i32, String> {
        let mut env = attach()?;
        let class = env
            .find_class("android/os/Build$VERSION")
            .map_err(|e| e.to_string())?;
        let value = env
            .get_static_field(class, "SDK_INT", "I")
            .map_err(|e| e.to_string())?;
        match value {
            jni::objects::JValueOwned::Int(i) => Ok(i),
            _ => Err("SDK_INT not readable".to_string()),
        }
    }

    fn activity<'local>(env: &mut JNIEnv<'local>) -> Result<JObject<'local>, String> {
        let class = env
            .find_class(PLATFORM_BRIDGE_CLASS)
            .map_err(|e| e.to_string())?;
        let value = env
            .get_static_field(class, MAIN_ACTIVITY_FIELD, MAIN_ACTIVITY_SIG)
            .map_err(|e| e.to_string())?;
        match value {
            jni::objects::JValueOwned::Object(obj) if !obj.is_null() => Ok(obj),
            _ => Err("PlatformBridge.activity not set".to_string()),
        }
    }

    /// Absolute app files directory, from the Activity's Context.
    pub fn files_dir() -> Result<String, String> {
        let mut env = attach()?;
        let activity = activity(&mut env)?;
        let file = env
            .call_method(&activity, "getFilesDir", "()Ljava/io/File;", &[])
            .map_err(|e| e.to_string())?;
        let file_obj = file.l().map_err(|e| e.to_string())?;
        let path = env
            .call_method(&file_obj, "getAbsolutePath", "()Ljava/lang/String;", &[])
            .map_err(|e| e.to_string())?;
        let path_obj = path.l().map_err(|e| e.to_string())?;
        let jstr = JString::from(path_obj);
        let s = env.get_string(&jstr).map_err(|e| e.to_string())?;
        Ok(s.into())
    }

    /// Java `LiveRecorder` singleton (Kotlin object) — DVR muxer for live
    /// broadcasts. MediaMuxer has no NDK API, so the muxer stays Kotlin and
    /// the Rust codec drains call into it via JNI.
    pub fn live_recorder_start() -> Result<String, String> {
        l_string(live_recorder_call("start", "()Ljava/lang/String;", &[])?)
    }

    pub fn live_recorder_stop() -> Result<String, String> {
        l_string(live_recorder_call("stop", "()Ljava/lang/String;", &[])?)
    }

    pub fn live_recorder_write_video(
        nal: &[u8],
        is_key: bool,
        is_config: bool,
        width: i32,
        height: i32,
    ) -> Result<(), String> {
        live_recorder_call(
            "writeVideo",
            "([BZZII)V",
            &[
                jni::objects::JValue::Object(&jbytes(nal)),
                jni::objects::JValue::Bool(is_key as u8),
                jni::objects::JValue::Bool(is_config as u8),
                jni::objects::JValue::Int(width),
                jni::objects::JValue::Int(height),
            ],
        )?;
        Ok(())
    }

    pub fn live_recorder_write_audio(blob: &[u8], is_config: bool) -> Result<(), String> {
        live_recorder_call(
            "writeAudio",
            "([BZ)V",
            &[
                jni::objects::JValue::Object(&jbytes(blob)),
                jni::objects::JValue::Bool(is_config as u8),
            ],
        )?;
        Ok(())
    }

    fn jbytes(data: &[u8]) -> jni::objects::JByteArray<'_> {
        // attached env used below via live_recorder_call; re-attach here
        let env = attach().expect("jvm attached");
        env.byte_array_from_slice(data).expect("byte array")
    }

    fn live_recorder_call(
        method: &str,
        sig: &str,
        args: &[jni::objects::JValue<'_, '_>],
    ) -> Result<jni::objects::JValueOwned<'static>, String> {
        let mut env = attach()?;
        let class = env
            .find_class("com/example/soshal_flutter/LiveRecorder")
            .map_err(|e| e.to_string())?;
        let instance = env
            .get_static_field(
                class,
                "INSTANCE",
                "Lcom/example/soshal_flutter/LiveRecorder;",
            )
            .map_err(|e| e.to_string())?;
        let obj = match instance {
            jni::objects::JValueOwned::Object(o) if !o.is_null() => o,
            _ => return Err("LiveRecorder.INSTANCE not set".to_string()),
        };
        env.call_method(&obj, method, sig, args)
            .map_err(|e| e.to_string())
    }

    fn l_string(value: jni::objects::JValueOwned<'_>) -> Result<String, String> {
        let mut env = attach()?;
        let obj = value.l().map_err(|e| e.to_string())?;
        if obj.is_null() {
            return Err("null result".to_string());
        }
        let jstr = jni::objects::JString::from(obj);
        let s = env.get_string(&jstr).map_err(|e| e.to_string())?;
        Ok(s.into())
    }

    const PERMISSION_GRANTED: i32 = 0;
    const PERMISSION_REQUEST_CODE: i32 = 7001;

    /// Runtime permission state via Activity.checkSelfPermission (API 23+).
    pub fn permission_granted(name: &str) -> Result<bool, String> {
        let mut env = attach()?;
        let activity = activity(&mut env)?;
        let name_j = env.new_string(name).map_err(|e| e.to_string())?;
        let value = env
            .call_method(
                &activity,
                "checkSelfPermission",
                "(Ljava/lang/String;)I",
                &[jni::objects::JValue::Object(&name_j)],
            )
            .map_err(|e| e.to_string())?;
        match value {
            jni::objects::JValueOwned::Int(i) => Ok(i == PERMISSION_GRANTED),
            _ => Err("checkSelfPermission unreadable".to_string()),
        }
    }

    /// Fire a runtime permission dialog (Activity.requestPermissions).
    pub fn request_permissions(names: &[&str]) -> Result<(), String> {
        let mut env = attach()?;
        let activity = activity(&mut env)?;
        let string_class = env
            .find_class("java/lang/String")
            .map_err(|e| e.to_string())?;
        let array = env
            .new_object_array(
                names.len() as i32,
                &string_class,
                &jni::objects::JObject::null(),
            )
            .map_err(|e| e.to_string())?;
        for (i, n) in names.iter().enumerate() {
            let s = env.new_string(*n).map_err(|e| e.to_string())?;
            env.set_object_array_element(&array, i as i32, &s)
                .map_err(|e| e.to_string())?;
        }
        env.call_method(
            &activity,
            "requestPermissions",
            "([Ljava/lang/String;I)V",
            &[
                jni::objects::JValue::Object(&array),
                jni::objects::JValue::Int(PERMISSION_REQUEST_CODE),
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// False for a *denied* permission when the user chose "don't ask again"
    /// (Activity.shouldShowRequestPermissionRationale, API 23+).
    pub fn should_show_rationale(name: &str) -> Result<bool, String> {
        let mut env = attach()?;
        let activity = activity(&mut env)?;
        let name_j = env.new_string(name).map_err(|e| e.to_string())?;
        let value = env
            .call_method(
                &activity,
                "shouldShowRequestPermissionRationale",
                "(Ljava/lang/String;)Z",
                &[jni::objects::JValue::Object(&name_j)],
            )
            .map_err(|e| e.to_string())?;
        match value {
            jni::objects::JValueOwned::Bool(b) => Ok(b != 0),
            _ => Err("shouldShowRequestPermissionRationale unreadable".to_string()),
        }
    }

    /// Open the OS app-settings page for this app.
    pub fn open_app_settings() -> Result<(), String> {
        let mut env = attach()?;
        let activity = activity(&mut env)?;
        let pkg = {
            let value = env
                .call_method(&activity, "getPackageName", "()Ljava/lang/String;", &[])
                .map_err(|e| e.to_string())?;
            l_string(value)?
        };
        let action = env
            .new_string("android.settings.APPLICATION_DETAILS_SETTINGS")
            .map_err(|e| e.to_string())?;
        let intent = env
            .new_object(
                "android/content/Intent",
                "(Ljava/lang/String;)V",
                &[jni::objects::JValue::Object(&action)],
            )
            .map_err(|e| e.to_string())?;
        let uri_spec = env
            .new_string(format!("package:{pkg}"))
            .map_err(|e| e.to_string())?;
        let uri = env
            .call_static_method(
                "android/net/Uri",
                "parse",
                "(Ljava/lang/String;)Landroid/net/Uri;",
                &[jni::objects::JValue::Object(&uri_spec)],
            )
            .map_err(|e| e.to_string())?
            .l()
            .map_err(|e| e.to_string())?;
        env.call_method(
            &intent,
            "setData",
            "(Landroid/net/Uri;)Landroid/content/Intent;",
            &[jni::objects::JValue::Object(&uri)],
        )
        .map_err(|e| e.to_string())?;
        env.call_method(
            &intent,
            "addFlags",
            "(I)Landroid/content/Intent;",
            &[jni::objects::JValue::Int(0x1000_0000)], // FLAG_ACTIVITY_NEW_TASK
        )
        .map_err(|e| e.to_string())?;
        env.call_method(
            &activity,
            "startActivity",
            "(Landroid/content/Intent;)V",
            &[jni::objects::JValue::Object(&intent)],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Any location provider enabled (GPS or network).
    pub fn location_enabled() -> Result<bool, String> {
        let mut env = attach()?;
        let activity = activity(&mut env)?;
        let svc = env.new_string("location").map_err(|e| e.to_string())?;
        let manager = env
            .call_method(
                &activity,
                "getSystemService",
                "(Ljava/lang/String;)Ljava/lang/Object;",
                &[jni::objects::JValue::Object(&svc)],
            )
            .map_err(|e| e.to_string())?
            .l()
            .map_err(|e| e.to_string())?;
        for provider in ["gps", "network"] {
            let p = env.new_string(provider).map_err(|e| e.to_string())?;
            let value = env
                .call_method(
                    &manager,
                    "isProviderEnabled",
                    "(Ljava/lang/String;)Z",
                    &[jni::objects::JValue::Object(&p)],
                )
                .map_err(|e| e.to_string())?;
            if let jni::objects::JValueOwned::Bool(b) = value {
                if b != 0 {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    const BATTERY_PROPERTY_CAPACITY: i32 = 4;
    const BATTERY_PROPERTY_STATUS: i32 = 5;
    const BATTERY_STATUS_CHARGING: i32 = 2;
    const BATTERY_STATUS_FULL: i32 = 5;
    const CONNECTIVITY_TYPE_MOBILE: i32 = 0;
    const CONNECTIVITY_TYPE_WIMAX: i32 = 6;

    /// Battery charge state via BatteryManager: (charging, capacity %).
    pub fn battery_state() -> Result<(bool, i32), String> {
        let mut env = attach()?;
        let activity = activity(&mut env)?;
        let svc = env
            .new_string("batterymanager")
            .map_err(|e| e.to_string())?;
        let manager = env
            .call_method(
                &activity,
                "getSystemService",
                "(Ljava/lang/String;)Ljava/lang/Object;",
                &[jni::objects::JValue::Object(&svc)],
            )
            .map_err(|e| e.to_string())?
            .l()
            .map_err(|e| e.to_string())?;
        let capacity = match env
            .call_method(
                &manager,
                "getIntProperty",
                "(I)I",
                &[jni::objects::JValue::Int(BATTERY_PROPERTY_CAPACITY)],
            )
            .map_err(|e| e.to_string())?
        {
            jni::objects::JValueOwned::Int(i) => i,
            _ => return Err("capacity unreadable".to_string()),
        };
        let status = match env
            .call_method(
                &manager,
                "getIntProperty",
                "(I)I",
                &[jni::objects::JValue::Int(BATTERY_PROPERTY_STATUS)],
            )
            .map_err(|e| e.to_string())?
        {
            jni::objects::JValueOwned::Int(i) => i,
            _ => return Err("status unreadable".to_string()),
        };
        let charging = status == BATTERY_STATUS_CHARGING || status == BATTERY_STATUS_FULL;
        Ok((charging, if capacity < 0 { 100 } else { capacity }))
    }

    /// OS battery-save mode via PowerManager.isPowerSaveMode.
    pub fn power_save_mode() -> Result<bool, String> {
        let mut env = attach()?;
        let activity = activity(&mut env)?;
        let svc = env.new_string("power").map_err(|e| e.to_string())?;
        let manager = env
            .call_method(
                &activity,
                "getSystemService",
                "(Ljava/lang/String;)Ljava/lang/Object;",
                &[jni::objects::JValue::Object(&svc)],
            )
            .map_err(|e| e.to_string())?
            .l()
            .map_err(|e| e.to_string())?;
        match env
            .call_method(&manager, "isPowerSaveMode", "()Z", &[])
            .map_err(|e| e.to_string())?
        {
            jni::objects::JValueOwned::Bool(b) => Ok(b != 0),
            _ => Err("power save mode unreadable".to_string()),
        }
    }

    /// True when the active network is cellular (mobile/wimax).
    pub fn cellular_connection() -> Result<bool, String> {
        let mut env = attach()?;
        let activity = activity(&mut env)?;
        let svc = env.new_string("connectivity").map_err(|e| e.to_string())?;
        let manager = env
            .call_method(
                &activity,
                "getSystemService",
                "(Ljava/lang/String;)Ljava/lang/Object;",
                &[jni::objects::JValue::Object(&svc)],
            )
            .map_err(|e| e.to_string())?
            .l()
            .map_err(|e| e.to_string())?;
        let info = env
            .call_method(
                &manager,
                "getActiveNetworkInfo",
                "()Landroid/net/NetworkInfo;",
                &[],
            )
            .map_err(|e| e.to_string())?
            .l()
            .map_err(|e| e.to_string())?;
        if info.is_null() {
            return Ok(false);
        }
        let net_type = match env
            .call_method(&info, "getType", "()I", &[])
            .map_err(|e| e.to_string())?
        {
            jni::objects::JValueOwned::Int(i) => i,
            _ => return Err("network type unreadable".to_string()),
        };
        Ok(net_type == CONNECTIVITY_TYPE_MOBILE || net_type == CONNECTIVITY_TYPE_WIMAX)
    }

    type AAssetManagerFromJava = unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void;
    type AAssetManagerOpen =
        unsafe extern "C" fn(*mut c_void, *const libc::c_char, i32) -> *mut c_void;
    type AAssetGetLength = unsafe extern "C" fn(*mut c_void) -> libc::off_t;
    type AAssetRead = unsafe extern "C" fn(*mut c_void, *mut c_void, usize) -> isize;
    type AAssetClose = unsafe extern "C" fn(*mut c_void);

    /// Read a bundled asset ("daemons/i2pd") via the native AssetManager.
    pub fn read_asset(name: &str) -> Result<Vec<u8>, String> {
        let mut env = attach()?;
        let activity = activity(&mut env)?;
        let assets = env
            .call_method(
                &activity,
                "getAssets",
                "()Landroid/content/res/AssetManager;",
                &[],
            )
            .map_err(|e| e.to_string())?;
        let assets_obj = assets.l().map_err(|e| e.to_string())?;

        let from_java = dlsym::<AAssetManagerFromJava>("libandroid.so", "AAssetManager_fromJava")
            .ok_or("AAssetManager_fromJava missing".to_string())?;
        let open_fn = dlsym::<AAssetManagerOpen>("libandroid.so", "AAssetManager_open")
            .ok_or("AAssetManager_open missing".to_string())?;
        let len_fn = dlsym::<AAssetGetLength>("libandroid.so", "AAsset_getLength")
            .ok_or("AAsset_getLength missing".to_string())?;
        let read_fn = dlsym::<AAssetRead>("libandroid.so", "AAsset_read")
            .ok_or("AAsset_read missing".to_string())?;
        let close_fn = dlsym::<AAssetClose>("libandroid.so", "AAsset_close")
            .ok_or("AAsset_close missing".to_string())?;

        let c_name = std::ffi::CString::new(name).map_err(|e| e.to_string())?;
        let env_raw = env.get_native_interface();
        let manager =
            unsafe { from_java(env_raw as *mut c_void, assets_obj.as_raw() as *mut c_void) };
        if manager.is_null() {
            return Err("AssetManager_fromJava failed".to_string());
        }
        let asset = unsafe { open_fn(manager, c_name.as_ptr(), 0) };
        if asset.is_null() {
            return Err(format!("asset not found: {name}"));
        }
        let len = unsafe { len_fn(asset) };
        if len <= 0 {
            unsafe { close_fn(asset) };
            return Err(format!("asset empty: {name}"));
        }
        let mut buf = vec![0u8; len as usize];
        let read = unsafe { read_fn(asset, buf.as_mut_ptr() as *mut c_void, buf.len()) };
        unsafe { close_fn(asset) };
        if read != len as isize {
            return Err(format!("short asset read for {name}: {read}/{len}"));
        }
        Ok(buf)
    }
}

#[cfg(not(target_os = "android"))]
#[allow(dead_code)] // host stubs only used by android-target code
mod android {
    pub fn files_dir() -> Result<String, String> {
        Err("platform bridge unavailable off-Android".to_string())
    }

    pub fn read_asset(_name: &str) -> Result<Vec<u8>, String> {
        Err("platform bridge unavailable off-Android".to_string())
    }

    pub fn sdk_int() -> Result<i32, String> {
        Err("platform bridge unavailable off-Android".to_string())
    }

    pub fn live_recorder_start() -> Result<String, String> {
        Err("platform bridge unavailable off-Android".to_string())
    }

    pub fn live_recorder_stop() -> Result<String, String> {
        Err("platform bridge unavailable off-Android".to_string())
    }

    pub fn live_recorder_write_video(
        _nal: &[u8],
        _is_key: bool,
        _is_config: bool,
        _width: i32,
        _height: i32,
    ) -> Result<(), String> {
        Err("platform bridge unavailable off-Android".to_string())
    }

    pub fn live_recorder_write_audio(_blob: &[u8], _is_config: bool) -> Result<(), String> {
        Err("platform bridge unavailable off-Android".to_string())
    }

    pub fn permission_granted(_name: &str) -> Result<bool, String> {
        Err("platform bridge unavailable off-Android".to_string())
    }

    pub fn request_permissions(_names: &[&str]) -> Result<(), String> {
        Err("platform bridge unavailable off-Android".to_string())
    }

    pub fn should_show_rationale(_name: &str) -> Result<bool, String> {
        Err("platform bridge unavailable off-Android".to_string())
    }

    pub fn open_app_settings() -> Result<(), String> {
        Err("platform bridge unavailable off-Android".to_string())
    }

    pub fn location_enabled() -> Result<bool, String> {
        Err("platform bridge unavailable off-Android".to_string())
    }

    pub fn battery_state() -> Result<(bool, i32), String> {
        Err("platform bridge unavailable off-Android".to_string())
    }

    pub fn power_save_mode() -> Result<bool, String> {
        Err("platform bridge unavailable off-Android".to_string())
    }

    pub fn cellular_connection() -> Result<bool, String> {
        Err("platform bridge unavailable off-Android".to_string())
    }
}

#[allow(unused_imports)] // host: consumed only by cfg(android) code
pub use android::{
    battery_state, cellular_connection, files_dir, live_recorder_start, live_recorder_stop,
    live_recorder_write_audio, live_recorder_write_video, location_enabled, open_app_settings,
    permission_granted, power_save_mode, read_asset, request_permissions, sdk_int,
    should_show_rationale,
};
