//! Android platform bridge: JavaVM handle, MainActivity reference, native
//! asset reads (AAssetManager via libandroid). Stubbed on other platforms.
//! Shared foundation for daemon lifecycle, permissions, and power sampling.

#[cfg(target_os = "android")]
#[allow(unsafe_code)]
mod android {
    use std::ffi::c_void;
    use std::sync::Mutex;

    use jni::objects::{GlobalRef, JClass, JObject, JString, JValue};
    use jni::{JNIEnv, JavaVM};

    const PLATFORM_BRIDGE_CLASS: &str = "com/soshal/app/PlatformBridge";
    const MAIN_ACTIVITY_FIELD: &str = "activity";
    const MAIN_ACTIVITY_SIG: &str = "Lcom/soshal/app/MainActivity;";

    static CACHED_ACTIVITY: Mutex<Option<GlobalRef>> = Mutex::new(None);
    static CACHED_CONTEXT: Mutex<Option<GlobalRef>> = Mutex::new(None);
    static CACHED_CLASS_LOADER: Mutex<Option<GlobalRef>> = Mutex::new(None);

    #[no_mangle]
    pub extern "system" fn Java_com_soshal_app_PlatformBridge_nativeInit(
        mut env: JNIEnv,
        _class: JClass,
        activity: JObject,
        context: JObject,
    ) {
        if !activity.is_null() {
            if let Ok(glob) = env.new_global_ref(&activity) {
                *CACHED_ACTIVITY.lock().unwrap_or_else(|e| e.into_inner()) = Some(glob);
            }
        }
        if !context.is_null() {
            if let Ok(loader_val) =
                env.call_method(&context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
            {
                if let Ok(loader_obj) = loader_val.l() {
                    if !loader_obj.is_null() {
                        if let Ok(glob) = env.new_global_ref(&loader_obj) {
                            *CACHED_CLASS_LOADER
                                .lock()
                                .unwrap_or_else(|e| e.into_inner()) = Some(glob);
                        }
                    }
                }
            }
            let _ = env.exception_clear();
            if let Ok(glob) = env.new_global_ref(&context) {
                *CACHED_CONTEXT.lock().unwrap_or_else(|e| e.into_inner()) = Some(glob);
            }
        }
    }

    type JniGetCreatedJavaVMs =
        unsafe extern "C" fn(*mut *mut jni::sys::JavaVM, i32, *mut i32) -> i32;

    static JAVA_VM: Mutex<Option<&'static JavaVM>> = Mutex::new(None);

    // dlsym returns a raw function pointer; cast through usize to the
    // concrete extern "C" type. A bare `transmute` of `*mut c_void -> T`
    // fails to compile on 32-bit armv7 where function pointers differ in
    // size/representation; usize is pointer-sized on every ABI.
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
            // SAFETY: dlsym returns a valid function pointer for `symbol`;
            // the caller's `T` must match its extern "C" signature exactly.
            Some(std::mem::transmute_copy(&(ptr as usize)))
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

    /// Local-frame error: carries our own messages and converts JNI errors,
    /// so `with_local_frame` closures can propagate either with `?`.
    #[derive(Debug)]
    struct JniErr(String);

    impl std::fmt::Display for JniErr {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.0)
        }
    }

    impl std::error::Error for JniErr {}

    impl From<jni::errors::Error> for JniErr {
        fn from(e: jni::errors::Error) -> Self {
            JniErr(e.to_string())
        }
    }

    /// Android SDK level (Build.VERSION.SDK_INT). Codec features gate on 26.
    pub fn sdk_int() -> Result<i32, String> {
        let mut env = attach()?;
        env.with_local_frame(4, |env| -> Result<i32, JniErr> {
            let class = env.find_class("android/os/Build$VERSION")?;
            let value = env.get_static_field(class, "SDK_INT", "I")?;
            match value {
                jni::objects::JValueOwned::Int(i) => Ok(i),
                _ => Err(JniErr("SDK_INT not readable".to_string())),
            }
        })
        .map_err(|e| e.0)
    }

    fn get_context<'local>(env: &mut JNIEnv<'local>) -> Result<JObject<'local>, JniErr> {
        if let Some(ctx_ref) = CACHED_CONTEXT
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            return Ok(env.new_local_ref(ctx_ref.as_obj())?);
        }
        if let Some(act_ref) = CACHED_ACTIVITY
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            return Ok(env.new_local_ref(act_ref.as_obj())?);
        }
        // Fallback: ActivityThread.currentApplication()
        match env.find_class("android/app/ActivityThread") {
            Ok(app_thread_class) => {
                let app_res = env.call_static_method(
                    &app_thread_class,
                    "currentApplication",
                    "()Landroid/app/Application;",
                    &[],
                );
                match app_res {
                    Ok(val) => match val.l() {
                        Ok(obj) if !obj.is_null() => {
                            if let Ok(glob) = env.new_global_ref(&obj) {
                                *CACHED_CONTEXT.lock().unwrap_or_else(|e| e.into_inner()) =
                                    Some(glob);
                            }
                            return Ok(obj);
                        }
                        _ => {
                            let _ = env.exception_clear();
                        }
                    },
                    Err(_) => {
                        let _ = env.exception_clear();
                    }
                }
            }
            Err(_) => {
                let _ = env.exception_clear();
            }
        }
        Err(JniErr("application context not available".to_string()))
    }

    fn get_class_loader<'local>(
        env: &mut JNIEnv<'local>,
    ) -> Result<Option<JObject<'local>>, JniErr> {
        if let Some(loader_ref) = CACHED_CLASS_LOADER
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            return Ok(Some(env.new_local_ref(loader_ref.as_obj())?));
        }
        if let Ok(ctx) = get_context(env) {
            let loader_res =
                env.call_method(&ctx, "getClassLoader", "()Ljava/lang/ClassLoader;", &[]);
            match loader_res {
                Ok(val) => match val.l() {
                    Ok(obj) if !obj.is_null() => {
                        if let Ok(glob) = env.new_global_ref(&obj) {
                            *CACHED_CLASS_LOADER
                                .lock()
                                .unwrap_or_else(|e| e.into_inner()) = Some(glob);
                        }
                        return Ok(Some(obj));
                    }
                    _ => {
                        let _ = env.exception_clear();
                    }
                },
                Err(_) => {
                    let _ = env.exception_clear();
                }
            }
        }
        Ok(None)
    }

    fn find_app_class<'local>(
        env: &mut JNIEnv<'local>,
        class_name: &str,
    ) -> Result<JClass<'local>, JniErr> {
        if let Ok(Some(loader)) = get_class_loader(env) {
            let name_dots = class_name.replace('/', ".");
            if let Ok(name_j) = env.new_string(&name_dots) {
                let call_res = env.call_method(
                    &loader,
                    "loadClass",
                    "(Ljava/lang/String;)Ljava/lang/Class;",
                    &[JValue::Object(&name_j)],
                );
                match call_res {
                    Ok(val) => match val.l() {
                        Ok(obj) if !obj.is_null() => {
                            return Ok(obj.into());
                        }
                        _ => {
                            let _ = env.exception_clear();
                        }
                    },
                    Err(_) => {
                        let _ = env.exception_clear();
                    }
                }
            }
        }
        match env.find_class(class_name) {
            Ok(c) => Ok(c),
            Err(e) => {
                let _ = env.exception_clear();
                Err(JniErr(format!("class {class_name} not found: {e}")))
            }
        }
    }

    fn activity<'local>(env: &mut JNIEnv<'local>) -> Result<JObject<'local>, JniErr> {
        if let Some(act_ref) = CACHED_ACTIVITY
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            return Ok(env.new_local_ref(act_ref.as_obj())?);
        }
        if let Ok(class) = find_app_class(env, PLATFORM_BRIDGE_CLASS) {
            if let Ok(value) = env.get_static_field(&class, MAIN_ACTIVITY_FIELD, MAIN_ACTIVITY_SIG)
            {
                if let Ok(obj) = value.l() {
                    if !obj.is_null() {
                        return Ok(obj);
                    }
                }
            }
            let _ = env.exception_clear();
        }
        get_context(env)
    }

    /// Absolute app files directory, from the Activity's Context.
    pub fn files_dir() -> Result<String, String> {
        let mut env = attach()?;
        env.with_local_frame(8, |env| -> Result<String, JniErr> {
            let activity = activity(env)?;
            let file = env.call_method(&activity, "getFilesDir", "()Ljava/io/File;", &[])?;
            let file_obj = file.l()?;
            let path =
                env.call_method(&file_obj, "getAbsolutePath", "()Ljava/lang/String;", &[])?;
            let path_obj = path.l()?;
            let jstr = JString::from(path_obj);
            let s = env.get_string(&jstr)?;
            Ok(s.into())
        })
        .map_err(|e| e.0)
    }

    /// Absolute app native library directory (where .so files and binaries in jniLibs are extracted).
    /// Untrusted apps on Android 10+ have SELinux rx_file_perms on apk_data_file here.
    pub fn native_library_dir() -> Result<String, String> {
        let mut env = attach()?;
        env.with_local_frame(8, |env| -> Result<String, JniErr> {
            let activity = activity(env)?;
            let app_info = env
                .call_method(
                    &activity,
                    "getApplicationInfo",
                    "()Landroid/content/pm/ApplicationInfo;",
                    &[],
                )?
                .l()?;
            let lib_dir = env
                .get_field(&app_info, "nativeLibraryDir", "Ljava/lang/String;")?
                .l()?;
            let jstr = jni::objects::JString::from(lib_dir);
            let s = env.get_string(&jstr)?;
            Ok(s.into())
        })
        .map_err(|e| e.0)
    }

    /// Java `LiveRecorder` singleton (Kotlin object) — DVR muxer for live
    /// broadcasts. MediaMuxer has no NDK API, so the muxer stays Kotlin and
    /// the Rust codec drains call into it via JNI.
    pub fn live_recorder_start() -> Result<String, String> {
        let mut env = attach()?;
        env.with_local_frame(8, |env| -> Result<String, JniErr> {
            let value = live_recorder_call(env, "start", "()Ljava/lang/String;", &[])?;
            l_string(env, value)
        })
        .map_err(|e| e.0)
    }

    pub fn live_recorder_stop() -> Result<String, String> {
        let mut env = attach()?;
        env.with_local_frame(8, |env| -> Result<String, JniErr> {
            let value = live_recorder_call(env, "stop", "()Ljava/lang/String;", &[])?;
            l_string(env, value)
        })
        .map_err(|e| e.0)
    }

    pub fn live_recorder_write_video(
        nal: &[u8],
        is_key: bool,
        is_config: bool,
        width: i32,
        height: i32,
    ) -> Result<(), String> {
        let mut env = attach()?;
        env.with_local_frame(16, |env| -> Result<(), JniErr> {
            let nal_j = jbytes(env, nal)?;
            live_recorder_call(
                env,
                "writeVideo",
                "([BZZII)V",
                &[
                    jni::objects::JValue::Object(nal_j.as_ref()),
                    jni::objects::JValue::Bool(is_key as u8),
                    jni::objects::JValue::Bool(is_config as u8),
                    jni::objects::JValue::Int(width),
                    jni::objects::JValue::Int(height),
                ],
            )?;
            Ok(())
        })
        .map_err(|e| e.0)
    }

    pub fn live_recorder_write_audio(blob: &[u8], is_config: bool) -> Result<(), String> {
        let mut env = attach()?;
        env.with_local_frame(16, |env| -> Result<(), JniErr> {
            let blob_j = jbytes(env, blob)?;
            live_recorder_call(
                env,
                "writeAudio",
                "([BZ)V",
                &[
                    jni::objects::JValue::Object(blob_j.as_ref()),
                    jni::objects::JValue::Bool(is_config as u8),
                ],
            )?;
            Ok(())
        })
        .map_err(|e| e.0)
    }

    fn jbytes<'local>(
        env: &mut JNIEnv<'local>,
        data: &[u8],
    ) -> Result<jni::objects::JByteArray<'local>, JniErr> {
        Ok(env.byte_array_from_slice(data)?)
    }

    fn live_recorder_call<'local>(
        env: &mut JNIEnv<'local>,
        method: &str,
        sig: &str,
        args: &[jni::objects::JValue<'_, '_>],
    ) -> Result<jni::objects::JValueOwned<'local>, JniErr> {
        let class = find_app_class(env, "com/soshal/app/LiveRecorder")?;
        let instance = env.get_static_field(&class, "INSTANCE", "Lcom/soshal/app/LiveRecorder;")?;
        let obj = match instance {
            jni::objects::JValueOwned::Object(o) if !o.is_null() => o,
            _ => return Err(JniErr("LiveRecorder.INSTANCE not set".to_string())),
        };
        Ok(env.call_method(&obj, method, sig, args)?)
    }

    fn l_string<'local>(
        env: &mut JNIEnv<'local>,
        value: jni::objects::JValueOwned<'local>,
    ) -> Result<String, JniErr> {
        let obj = value.l()?;
        if obj.is_null() {
            return Err(JniErr("null result".to_string()));
        }
        let jstr = jni::objects::JString::from(obj);
        let s = env.get_string(&jstr)?;
        Ok(s.into())
    }

    const PERMISSION_GRANTED: i32 = 0;
    const PERMISSION_REQUEST_CODE: i32 = 7001;

    /// Runtime permission state via Activity.checkSelfPermission (API 23+).
    pub fn permission_granted(name: &str) -> Result<bool, String> {
        let mut env = attach()?;
        env.with_local_frame(8, |env| -> Result<bool, JniErr> {
            let activity = activity(env)?;
            let name_j = env.new_string(name)?;
            let value = env.call_method(
                &activity,
                "checkSelfPermission",
                "(Ljava/lang/String;)I",
                &[jni::objects::JValue::Object(&name_j)],
            )?;
            match value {
                jni::objects::JValueOwned::Int(i) => Ok(i == PERMISSION_GRANTED),
                _ => Err(JniErr("checkSelfPermission unreadable".to_string())),
            }
        })
        .map_err(|e| e.0)
    }

    /// Fire a runtime permission dialog (Activity.requestPermissions).
    pub fn request_permissions(names: &[&str]) -> Result<(), String> {
        let mut env = attach()?;
        let frame = (names.len() + 4) as i32; // array + N strings + call args margin
        env.with_local_frame(frame, |env| -> Result<(), JniErr> {
            let activity = activity(env)?;
            let string_class = env.find_class("java/lang/String")?;
            let array = env.new_object_array(
                names.len() as i32,
                &string_class,
                &jni::objects::JObject::null(),
            )?;
            for (i, n) in names.iter().enumerate() {
                let s = env.new_string(*n)?;
                env.set_object_array_element(&array, i as i32, &s)?;
            }
            env.call_method(
                &activity,
                "requestPermissions",
                "([Ljava/lang/String;I)V",
                &[
                    jni::objects::JValue::Object(array.as_ref()),
                    jni::objects::JValue::Int(PERMISSION_REQUEST_CODE),
                ],
            )?;
            Ok(())
        })
        .map_err(|e| e.0)
    }

    /// False for a *denied* permission when the user chose "don't ask again"
    /// (Activity.shouldShowRequestPermissionRationale, API 23+).
    pub fn should_show_rationale(name: &str) -> Result<bool, String> {
        let mut env = attach()?;
        env.with_local_frame(8, |env| -> Result<bool, JniErr> {
            let activity = activity(env)?;
            let name_j = env.new_string(name)?;
            let value = env.call_method(
                &activity,
                "shouldShowRequestPermissionRationale",
                "(Ljava/lang/String;)Z",
                &[jni::objects::JValue::Object(&name_j)],
            )?;
            match value {
                jni::objects::JValueOwned::Bool(b) => Ok(b != 0),
                _ => Err(JniErr(
                    "shouldShowRequestPermissionRationale unreadable".to_string(),
                )),
            }
        })
        .map_err(|e| e.0)
    }

    /// Location service state via LocationManager.isProviderEnabled; true if
    /// any provider (gps/network/passive) is switched on.
    pub fn location_enabled() -> Result<bool, String> {
        let mut env = attach()?;
        env.with_local_frame(16, |env| -> Result<bool, JniErr> {
            let activity = activity(env)?;
            let service = env.new_string("location")?;
            let manager = env.call_method(
                &activity,
                "getSystemService",
                "(Ljava/lang/String;)Ljava/lang/Object;",
                &[jni::objects::JValue::Object(&service)],
            )?;
            let manager_obj = manager.l()?;
            if manager_obj.is_null() {
                return Ok(false);
            }
            for provider in ["gps", "network", "passive"] {
                let p = env.new_string(provider)?;
                let value = env.call_method(
                    &manager_obj,
                    "isProviderEnabled",
                    "(Ljava/lang/String;)Z",
                    &[jni::objects::JValue::Object(&p)],
                )?;
                match value {
                    jni::objects::JValueOwned::Bool(b) if b != 0 => return Ok(true),
                    _ => {}
                }
            }
            Ok(false)
        })
        .map_err(|e| e.0)
    }

    const DAEMON_SERVICE_CLASS: &str = "com/soshal/app/DaemonForegroundService";

    /// Start the daemon foreground service (keeps the app process — and the
    /// i2pd/freenet/rnsd child processes it spawned — alive while
    /// backgrounded).
    pub fn daemon_service_start() -> Result<bool, String> {
        let mut env = attach()?;
        env.with_local_frame(8, |env| -> Result<bool, JniErr> {
            daemon_service_call(env, "start", "(Landroid/content/Context;)Z", &[])
        })
        .map_err(|e| e.0)
    }

    /// Stop the daemon foreground service.
    pub fn daemon_service_stop() -> Result<bool, String> {
        let mut env = attach()?;
        env.with_local_frame(8, |env| -> Result<bool, JniErr> {
            daemon_service_call(env, "stop", "(Landroid/content/Context;)Z", &[])
        })
        .map_err(|e| e.0)
    }

    /// Whether the daemon foreground service is currently started.
    pub fn daemon_service_running() -> Result<bool, String> {
        let mut env = attach()?;
        env.with_local_frame(8, |env| -> Result<bool, JniErr> {
            let class = find_app_class(env, DAEMON_SERVICE_CLASS)?;
            let value = env.get_static_field(&class, "running", "Z")?;
            match value {
                jni::objects::JValueOwned::Bool(b) => Ok(b != 0),
                _ => Err(JniErr("daemon service running unreadable".to_string())),
            }
        })
        .map_err(|e| e.0)
    }

    fn daemon_service_call<'local>(
        env: &mut JNIEnv<'local>,
        method: &str,
        sig: &str,
        args: &[jni::objects::JValue<'_, '_>],
    ) -> Result<bool, JniErr> {
        let activity = activity(env)?;
        let class = find_app_class(env, DAEMON_SERVICE_CLASS)?;
        let instance = env.get_static_field(
            &class,
            "INSTANCE",
            "Lcom/soshal/app/DaemonForegroundService;",
        )?;
        let obj = match instance {
            jni::objects::JValueOwned::Object(o) if !o.is_null() => o,
            _ => {
                return Err(JniErr(
                    "DaemonForegroundService.INSTANCE not set".to_string(),
                ))
            }
        };
        let mut call_args = Vec::with_capacity(args.len() + 1);
        call_args.push(jni::objects::JValue::Object(&activity));
        call_args.extend_from_slice(args);
        match env.call_method(&obj, method, sig, &call_args)? {
            jni::objects::JValueOwned::Bool(b) => Ok(b != 0),
            _ => Err(JniErr(format!("daemon service {method} unreadable"))),
        }
    }

    const RNSD_RUNNER_CLASS: &str = "com/soshal/app/RnsdRunner";

    fn rnsd_runner<'local>(env: &mut JNIEnv<'local>) -> Result<JObject<'local>, JniErr> {
        let class = find_app_class(env, RNSD_RUNNER_CLASS)?;
        let instance = env.get_static_field(&class, "INSTANCE", "Lcom/soshal/app/RnsdRunner;")?;
        match instance {
            jni::objects::JValueOwned::Object(o) if !o.is_null() => Ok(o),
            _ => Err(JniErr("RnsdRunner.INSTANCE not set".to_string())),
        }
    }

    /// Start the Reticulum daemon (rnsd) in the Chaquopy Python runtime.
    /// `config_dir` receives RNS's auto-generated config + identities.
    pub fn rnsd_start(config_dir: &str) -> Result<bool, String> {
        let mut env = attach()?;
        env.with_local_frame(8, |env| -> Result<bool, JniErr> {
            let runner = rnsd_runner(env)?;
            let activity = activity(env)?;
            let dir_j = env.new_string(config_dir)?;
            match env.call_method(
                &runner,
                "start",
                "(Landroid/content/Context;Ljava/lang/String;)Z",
                &[
                    jni::objects::JValue::Object(&activity),
                    jni::objects::JValue::Object(&dir_j),
                ],
            )? {
                jni::objects::JValueOwned::Bool(b) => Ok(b != 0),
                _ => Err(JniErr("rnsd start unreadable".to_string())),
            }
        })
        .map_err(|e| e.0)
    }

    /// Stop the Reticulum daemon.
    pub fn rnsd_stop() -> Result<bool, String> {
        let mut env = attach()?;
        env.with_local_frame(8, |env| -> Result<bool, JniErr> {
            let runner = rnsd_runner(env)?;
            let activity = activity(env)?;
            match env.call_method(
                &runner,
                "stop",
                "(Landroid/content/Context;)Z",
                &[jni::objects::JValue::Object(&activity)],
            )? {
                jni::objects::JValueOwned::Bool(b) => Ok(b != 0),
                _ => Err(JniErr("rnsd stop unreadable".to_string())),
            }
        })
        .map_err(|e| e.0)
    }

    /// Whether the Reticulum daemon thread is live.
    pub fn rnsd_running() -> Result<bool, String> {
        let mut env = attach()?;
        env.with_local_frame(8, |env| -> Result<bool, JniErr> {
            let runner = rnsd_runner(env)?;
            match env.call_method(&runner, "isRunning", "()Z", &[])? {
                jni::objects::JValueOwned::Bool(b) => Ok(b != 0),
                _ => Err(JniErr("rnsd running unreadable".to_string())),
            }
        })
        .map_err(|e| e.0)
    }

    /// Reticulum daemon status + last-start error fragment from
    /// `RnsdRunner.status()` (e.g. `"running":true,"error":"..."`). Surfaced
    /// through the daemon status JSON so the UI can explain why rnsd is down.
    pub fn rnsd_status() -> Result<String, String> {
        let mut env = attach()?;
        env.with_local_frame(8, |env| -> Result<String, JniErr> {
            let runner = rnsd_runner(env)?;
            match env.call_method(&runner, "status", "()Ljava/lang/String;", &[])? {
                jni::objects::JValueOwned::Object(o) => {
                    Ok(l_string(env, jni::objects::JValueOwned::Object(o))?)
                }
                _ => Err(JniErr("rnsd status unreadable".to_string())),
            }
        })
        .map_err(|e| e.0)
    }

    /// First entry of Build.SUPPORTED_ABIS (e.g. "arm64-v8a") — used to pick
    /// the per-ABI daemon binary asset.
    pub fn supported_abi() -> Result<String, String> {
        let mut env = attach()?;
        env.with_local_frame(8, |env| -> Result<String, JniErr> {
            let class = env.find_class("android/os/Build")?;
            let array = env.get_static_field(class, "SUPPORTED_ABIS", "[Ljava/lang/String;")?;
            let arr = jni::objects::JObjectArray::from(array.l()?);
            if arr.is_null() {
                return Err(JniErr("SUPPORTED_ABIS null".to_string()));
            }
            let len = env.get_array_length(&arr)?;
            if len <= 0 {
                return Err(JniErr("SUPPORTED_ABIS empty".to_string()));
            }
            let first = env.get_object_array_element(&arr, 0)?;
            let jstr = jni::objects::JString::from(first);
            let s = env.get_string(&jstr)?;
            Ok(s.into())
        })
        .map_err(|e| e.0)
    }

    /// Fire the "ignore battery optimizations" request dialog.
    pub fn request_ignore_battery_optimizations() -> Result<(), String> {
        let mut env = attach()?;
        env.with_local_frame(16, |env| -> Result<(), JniErr> {
            let activity = activity(env)?;
            let pkg = {
                let value =
                    env.call_method(&activity, "getPackageName", "()Ljava/lang/String;", &[])?;
                l_string(env, value)?
            };
            let action = env.new_string("android.settings.REQUEST_IGNORE_BATTERY_OPTIMIZATIONS")?;
            let intent = env.new_object(
                "android/content/Intent",
                "(Ljava/lang/String;)V",
                &[jni::objects::JValue::Object(&action)],
            )?;
            let uri_spec = env.new_string(format!("package:{pkg}"))?;
            let uri = env
                .call_static_method(
                    "android/net/Uri",
                    "parse",
                    "(Ljava/lang/String;)Landroid/net/Uri;",
                    &[jni::objects::JValue::Object(&uri_spec)],
                )?
                .l()?;
            env.call_method(
                &intent,
                "setData",
                "(Landroid/net/Uri;)Landroid/content/Intent;",
                &[jni::objects::JValue::Object(&uri)],
            )?;
            env.call_method(
                &intent,
                "addFlags",
                "(I)Landroid/content/Intent;",
                &[jni::objects::JValue::Int(0x1000_0000)], // FLAG_ACTIVITY_NEW_TASK
            )?;
            env.call_method(
                &activity,
                "startActivity",
                "(Landroid/content/Intent;)V",
                &[jni::objects::JValue::Object(&intent)],
            )?;
            Ok(())
        })
        .map_err(|e| e.0)
    }

    /// Open the OS app-settings page for this app.
    pub fn open_app_settings() -> Result<(), String> {
        let mut env = attach()?;
        env.with_local_frame(16, |env| -> Result<(), JniErr> {
            let activity = activity(env)?;
            let pkg = {
                let value =
                    env.call_method(&activity, "getPackageName", "()Ljava/lang/String;", &[])?;
                l_string(env, value)?
            };
            let action = env.new_string("android.settings.APPLICATION_DETAILS_SETTINGS")?;
            let intent = env.new_object(
                "android/content/Intent",
                "(Ljava/lang/String;)V",
                &[jni::objects::JValue::Object(&action)],
            )?;
            let uri_spec = env.new_string(format!("package:{pkg}"))?;
            let uri = env
                .call_static_method(
                    "android/net/Uri",
                    "parse",
                    "(Ljava/lang/String;)Landroid/net/Uri;",
                    &[jni::objects::JValue::Object(&uri_spec)],
                )?
                .l()?;
            env.call_method(
                &intent,
                "setData",
                "(Landroid/net/Uri;)Landroid/content/Intent;",
                &[jni::objects::JValue::Object(&uri)],
            )?;
            env.call_method(
                &intent,
                "addFlags",
                "(I)Landroid/content/Intent;",
                &[jni::objects::JValue::Int(0x1000_0000)], // FLAG_ACTIVITY_NEW_TASK
            )?;
            env.call_method(
                &activity,
                "startActivity",
                "(Landroid/content/Intent;)V",
                &[jni::objects::JValue::Object(&intent)],
            )?;
            Ok(())
        })
        .map_err(|e| e.0)
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
        env.with_local_frame(16, |env| -> Result<(bool, i32), JniErr> {
            let activity = activity(env)?;
            let svc = env.new_string("batterymanager")?;
            let manager = env
                .call_method(
                    &activity,
                    "getSystemService",
                    "(Ljava/lang/String;)Ljava/lang/Object;",
                    &[jni::objects::JValue::Object(&svc)],
                )?
                .l()?;
            if manager.is_null() {
                return Err(JniErr("batterymanager unavailable".to_string()));
            }
            let capacity = match env.call_method(
                &manager,
                "getIntProperty",
                "(I)I",
                &[jni::objects::JValue::Int(BATTERY_PROPERTY_CAPACITY)],
            )? {
                jni::objects::JValueOwned::Int(i) => i,
                _ => return Err(JniErr("capacity unreadable".to_string())),
            };
            let status = match env.call_method(
                &manager,
                "getIntProperty",
                "(I)I",
                &[jni::objects::JValue::Int(BATTERY_PROPERTY_STATUS)],
            )? {
                jni::objects::JValueOwned::Int(i) => i,
                _ => return Err(JniErr("status unreadable".to_string())),
            };
            let charging = status == BATTERY_STATUS_CHARGING || status == BATTERY_STATUS_FULL;
            Ok((charging, if capacity < 0 { 100 } else { capacity }))
        })
        .map_err(|e| e.0)
    }

    /// OS battery-save mode via PowerManager.isPowerSaveMode.
    pub fn power_save_mode() -> Result<bool, String> {
        let mut env = attach()?;
        env.with_local_frame(8, |env| -> Result<bool, JniErr> {
            let activity = activity(env)?;
            let svc = env.new_string("power")?;
            let manager = env
                .call_method(
                    &activity,
                    "getSystemService",
                    "(Ljava/lang/String;)Ljava/lang/Object;",
                    &[jni::objects::JValue::Object(&svc)],
                )?
                .l()?;
            if manager.is_null() {
                return Ok(false);
            }
            match env.call_method(&manager, "isPowerSaveMode", "()Z", &[])? {
                jni::objects::JValueOwned::Bool(b) => Ok(b != 0),
                _ => Err(JniErr("power save mode unreadable".to_string())),
            }
        })
        .map_err(|e| e.0)
    }

    /// True when the active network is cellular (mobile/wimax).
    pub fn cellular_connection() -> Result<bool, String> {
        let mut env = attach()?;
        env.with_local_frame(16, |env| -> Result<bool, JniErr> {
            let activity = activity(env)?;
            let svc = env.new_string("connectivity")?;
            let manager = env
                .call_method(
                    &activity,
                    "getSystemService",
                    "(Ljava/lang/String;)Ljava/lang/Object;",
                    &[jni::objects::JValue::Object(&svc)],
                )?
                .l()?;
            if manager.is_null() {
                return Ok(false);
            }
            let info = env
                .call_method(
                    &manager,
                    "getActiveNetworkInfo",
                    "()Landroid/net/NetworkInfo;",
                    &[],
                )?
                .l()?;
            if info.is_null() {
                return Ok(false);
            }
            let net_type = match env.call_method(&info, "getType", "()I", &[])? {
                jni::objects::JValueOwned::Int(i) => i,
                _ => return Err(JniErr("network type unreadable".to_string())),
            };
            Ok(net_type == CONNECTIVITY_TYPE_MOBILE || net_type == CONNECTIVITY_TYPE_WIMAX)
        })
        .map_err(|e| e.0)
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
        env.with_local_frame(8, |env| -> Result<Vec<u8>, JniErr> {
            let activity = activity(env)?;
            let assets = env.call_method(
                &activity,
                "getAssets",
                "()Landroid/content/res/AssetManager;",
                &[],
            )?;
            let assets_obj = assets.l()?;

            let from_java =
                dlsym::<AAssetManagerFromJava>("libandroid.so", "AAssetManager_fromJava")
                    .ok_or_else(|| JniErr("AAssetManager_fromJava missing".to_string()))?;
            let open_fn = dlsym::<AAssetManagerOpen>("libandroid.so", "AAssetManager_open")
                .ok_or_else(|| JniErr("AAssetManager_open missing".to_string()))?;
            let len_fn = dlsym::<AAssetGetLength>("libandroid.so", "AAsset_getLength")
                .ok_or_else(|| JniErr("AAsset_getLength missing".to_string()))?;
            let read_fn = dlsym::<AAssetRead>("libandroid.so", "AAsset_read")
                .ok_or_else(|| JniErr("AAsset_read missing".to_string()))?;
            let close_fn = dlsym::<AAssetClose>("libandroid.so", "AAsset_close")
                .ok_or_else(|| JniErr("AAsset_close missing".to_string()))?;

            let c_name = std::ffi::CString::new(name).map_err(|e| JniErr(e.to_string()))?;
            let env_raw = env.get_native_interface();
            let manager =
                unsafe { from_java(env_raw as *mut c_void, assets_obj.as_raw() as *mut c_void) };
            if manager.is_null() {
                return Err(JniErr("AssetManager_fromJava failed".to_string()));
            }
            let asset = unsafe { open_fn(manager, c_name.as_ptr(), 0) };
            if asset.is_null() {
                return Err(JniErr(format!("asset not found: {name}")));
            }
            let len = unsafe { len_fn(asset) };
            if len <= 0 {
                unsafe { close_fn(asset) };
                return Err(JniErr(format!("asset empty: {name}")));
            }
            let mut buf = vec![0u8; len as usize];
            let read = unsafe { read_fn(asset, buf.as_mut_ptr() as *mut c_void, buf.len()) };
            unsafe { close_fn(asset) };
            if read != len as isize {
                return Err(JniErr(format!("short asset read for {name}: {read}/{len}")));
            }
            Ok(buf)
        })
        .map_err(|e| e.0)
    }
}

#[cfg(not(target_os = "android"))]
#[allow(dead_code)] // host stubs only used by android-target code
mod android {
    pub fn files_dir() -> Result<String, String> {
        Err("platform bridge unavailable off-Android".to_string())
    }

    pub fn native_library_dir() -> Result<String, String> {
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

    pub fn daemon_service_start() -> Result<bool, String> {
        Ok(true) // no-op on desktop: daemons run while the app runs
    }

    pub fn daemon_service_stop() -> Result<bool, String> {
        Ok(true)
    }

    pub fn daemon_service_running() -> Result<bool, String> {
        Ok(false)
    }

    pub fn supported_abi() -> Result<String, String> {
        Err("platform bridge unavailable off-Android".to_string())
    }

    pub fn rnsd_start(_config_dir: &str) -> Result<bool, String> {
        Err("platform bridge unavailable off-Android".to_string())
    }

    pub fn rnsd_stop() -> Result<bool, String> {
        Err("platform bridge unavailable off-Android".to_string())
    }

    pub fn rnsd_running() -> Result<bool, String> {
        Ok(false)
    }

    pub fn request_ignore_battery_optimizations() -> Result<(), String> {
        Err("platform bridge unavailable off-Android".to_string())
    }

    pub fn location_enabled() -> Result<bool, String> {
        #[cfg(target_os = "linux")]
        {
            // Desktop location switch (GNOME/standard desktops). When this
            // is off the XDG location portal rejects with
            // NotAllowed: Location services disabled BEFORE any permission
            // dialog can appear — surfacing an actionable message beats a
            // raw portal error. Unknown state (gsettings missing, schema
            // absent, non-GNOME desktop) reports Ok(true): the portal
            // attempt still shows its per-app dialog whenever the OS
            // service is on, and off it errors with the guided message.
            match std::process::Command::new("gsettings")
                .args(["get", "org.gnome.system.location", "enabled"])
                .output()
            {
                Ok(out) => {
                    if !out.status.success() {
                        return Ok(true); // schema/desktop unknown — portal decides
                    }
                    Ok(String::from_utf8_lossy(&out.stdout).trim() == "true")
                }
                Err(_) => Ok(true), // gsettings unavailable — portal decides
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err("platform bridge unavailable off-Android".to_string())
        }
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

    pub fn rnsd_status() -> Result<String, String> {
        Err("rnsd_status only available on Android".to_string())
    }
}

#[allow(unused_imports)] // host: consumed only by cfg(android) code
pub use android::{
    battery_state, cellular_connection, daemon_service_running, daemon_service_start,
    daemon_service_stop, files_dir, live_recorder_start, live_recorder_stop,
    live_recorder_write_audio, live_recorder_write_video, location_enabled, native_library_dir,
    open_app_settings, permission_granted, power_save_mode, read_asset,
    request_ignore_battery_optimizations, request_permissions, rnsd_running, rnsd_start,
    rnsd_status, rnsd_stop, sdk_int, should_show_rationale, supported_abi,
};
