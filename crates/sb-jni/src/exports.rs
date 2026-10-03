//! The JNI exports of `dev.shaderbridge.natives.ShaderBridgeNative`. Each one converts its
//! arguments, calls the matching [`api`](crate::api) function and converts the result.
//!
//! Every export:
//! * clears this thread's last error on entry (except `lastError` itself);
//! * catches panics, which never unwind into the JVM;
//! * reports failure as `0`, `null` or `false` and records the reason as the thread's last
//!   error; a Java exception raised by a JNI call it made (e.g. an `OutOfMemoryError` while
//!   creating the result string) is cleared and reported the same way, so these methods
//!   never throw.
//!
//! Null string arguments are errors, except `optionValues` and `settingsJson` (null means
//! empty) and `language` (null means `en_us`). Direct `ByteBuffer` arguments must be
//! writable; `blobData`/`variantBlobData` write at the buffer's position (its position is
//! not changed), `evaluateUniforms` uses the whole buffer as the `sb_Frame` block.

use crate::api;
use crate::error::{Error, Result, panic_message};
use jni::JNIEnv;
use jni::objects::{JByteBuffer, JClass, JString};
use jni::sys::{JNI_FALSE, JNI_TRUE, jboolean, jfloat, jlong, jstring};
use std::cell::RefCell;
use std::panic::AssertUnwindSafe;
use std::path::Path;

thread_local! {
    static LAST_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn set_last_error(message: Option<String>) {
    // `try_with`: the slot may already be gone while the thread is exiting.
    let _ = LAST_ERROR.try_with(|slot| *slot.borrow_mut() = message);
}

/// The last error recorded on this thread.
fn last_error() -> Option<String> {
    LAST_ERROR.try_with(|slot| slot.borrow().clone()).ok().flatten()
}

/// Clear a pending Java exception (left by a failed JNI call).
fn clear_exception(env: &mut JNIEnv) {
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
}

/// A failed JNI call as an [`Error`], with any pending exception cleared.
fn jni_error(env: &mut JNIEnv, e: jni::errors::Error) -> Error {
    clear_exception(env);
    Error::Jni(e.to_string())
}

/// Run the body of an export: clear the last error, catch panics, record errors and
/// return `fail` on error.
fn call<'local, T>(env: &mut JNIEnv<'local>, fail: T, body: impl FnOnce(&mut JNIEnv<'local>) -> Result<T>) -> T {
    set_last_error(None);
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| body(env)))
        .unwrap_or_else(|payload| Err(Error::Internal(panic_message(&*payload))));
    match result {
        Ok(value) => value,
        Err(e) => {
            clear_exception(env);
            set_last_error(Some(e.to_string()));
            fail
        }
    }
}

/// A required string argument.
fn string_arg(env: &mut JNIEnv, s: &JString, what: &str) -> Result<String> {
    if s.is_null() {
        return Err(Error::invalid(format!("{what} is null")));
    }
    match env.get_string(s) {
        Ok(js) => Ok(String::from(js)),
        Err(e) => Err(jni_error(env, e)),
    }
}

/// A string argument where null means `default`.
fn string_or(env: &mut JNIEnv, s: &JString, what: &str, default: &str) -> Result<String> {
    if s.is_null() { Ok(default.to_string()) } else { string_arg(env, s, what) }
}

/// A new Java string (null-free modified UTF-8 is produced by the jni crate).
fn new_string(env: &mut JNIEnv, s: &str) -> Result<jstring> {
    match env.new_string(s) {
        Ok(j) => Ok(j.into_raw()),
        Err(e) => Err(jni_error(env, e)),
    }
}

/// A handle argument: ids are positive, so negative values become the never-issued 0.
fn handle(h: jlong) -> u64 {
    u64::try_from(h).unwrap_or(0)
}

/// A handle result.
fn jhandle(h: u64) -> Result<jlong> {
    jlong::try_from(h).map_err(|_| Error::Internal(format!("handle {h} does not fit a Java long")))
}

/// Writable memory of a direct `ByteBuffer`: `(address, length)`.
struct Region {
    ptr: *mut u8,
    len: usize,
}

/// The writable region of a direct buffer: from its position to its limit
/// (`from_position`), or its whole capacity.
fn direct_region(env: &mut JNIEnv, buf: &JByteBuffer, what: &str, from_position: bool) -> Result<Region> {
    if buf.is_null() {
        return Err(Error::invalid(format!("{what} is null")));
    }
    let base = match env.get_direct_buffer_address(buf) {
        Ok(p) => p,
        Err(_) => {
            clear_exception(env);
            return Err(Error::invalid(format!("{what} is not a direct ByteBuffer")));
        }
    };
    let capacity = env.get_direct_buffer_capacity(buf).map_err(|e| jni_error(env, e))?;
    let read_only = env.call_method(buf, "isReadOnly", "()Z", &[]).and_then(|v| v.z()).map_err(|e| jni_error(env, e))?;
    if read_only {
        return Err(Error::invalid(format!("{what} is read-only")));
    }
    if !from_position {
        return Ok(Region { ptr: base, len: capacity });
    }
    let position = env.call_method(buf, "position", "()I", &[]).and_then(|v| v.i()).map_err(|e| jni_error(env, e))?;
    let limit = env.call_method(buf, "limit", "()I", &[]).and_then(|v| v.i()).map_err(|e| jni_error(env, e))?;
    let (Ok(position), Ok(limit)) = (usize::try_from(position), usize::try_from(limit)) else {
        return Err(Error::invalid(format!("{what} has a negative position or limit")));
    };
    if position > limit || limit > capacity {
        return Err(Error::invalid(format!("{what} has position {position} and limit {limit} beyond its capacity {capacity}")));
    }
    // SAFETY: `position <= capacity`, so the offset stays inside (or one past) the buffer.
    let ptr = unsafe { base.add(position) };
    Ok(Region { ptr, len: limit - position })
}

/// Copy `bytes` to the start of `region`.
fn copy_into(region: &Region, bytes: &[u8], what: &str) -> Result<()> {
    if bytes.len() > region.len {
        return Err(Error::invalid(format!(
            "{what} has {} bytes remaining but the data needs {}",
            region.len,
            bytes.len()
        )));
    }
    // SAFETY: the region is `region.len >= bytes.len()` writable bytes of a direct buffer the
    // caller keeps alive (it is a live local reference for the duration of the native call),
    // and Rust-owned `bytes` cannot overlap Java memory.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), region.ptr, bytes.len()) };
    Ok(())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_shaderbridge_natives_ShaderBridgeNative_version<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    call(&mut env, std::ptr::null_mut(), |env| new_string(env, api::version()))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_shaderbridge_natives_ShaderBridgeNative_lastError<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    // Not `call`: that would clear the error being read.
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| match last_error() {
        Some(message) => new_string(&mut env, &message).unwrap_or(std::ptr::null_mut()),
        None => std::ptr::null_mut(),
    }));
    result.unwrap_or(std::ptr::null_mut())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_shaderbridge_natives_ShaderBridgeNative_listPacks<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    shaderpacks_dir: JString<'local>,
) -> jstring {
    call(&mut env, std::ptr::null_mut(), |env| {
        let dir = string_arg(env, &shaderpacks_dir, "shaderpacksDir")?;
        let json = api::list_packs_json(Path::new(&dir))?;
        new_string(env, &json)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_shaderbridge_natives_ShaderBridgeNative_openPack<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    pack_path: JString<'local>,
) -> jlong {
    call(&mut env, 0, |env| {
        let path = string_arg(env, &pack_path, "packPath")?;
        jhandle(api::open_pack(Path::new(&path))?)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_shaderbridge_natives_ShaderBridgeNative_closePack<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session: jlong,
) {
    call(&mut env, (), |_| api::close_pack(handle(session)))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_shaderbridge_natives_ShaderBridgeNative_getOptions<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session: jlong,
    language: JString<'local>,
) -> jstring {
    call(&mut env, std::ptr::null_mut(), |env| {
        let language = string_or(env, &language, "language", "en_us")?;
        let json = api::get_options(handle(session), &language)?;
        new_string(env, &json)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_shaderbridge_natives_ShaderBridgeNative_compile<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session: jlong,
    env_json: JString<'local>,
    option_values: JString<'local>,
    settings_json: JString<'local>,
) -> jstring {
    call(&mut env, std::ptr::null_mut(), |env| {
        let env_json = string_arg(env, &env_json, "envJson")?;
        let option_values = string_or(env, &option_values, "optionValues", "")?;
        let settings_json = string_or(env, &settings_json, "settingsJson", "")?;
        let json = api::compile(handle(session), &env_json, &option_values, &settings_json)?;
        new_string(env, &json)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_shaderbridge_natives_ShaderBridgeNative_blobSize<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session: jlong,
) -> jlong {
    call(&mut env, 0, |_| {
        let size = api::blob_size(handle(session))?;
        jlong::try_from(size).map_err(|_| Error::Internal(format!("blob buffer of {size} bytes")))
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_shaderbridge_natives_ShaderBridgeNative_blobData<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session: jlong,
    dst: JByteBuffer<'local>,
) -> jboolean {
    call(&mut env, JNI_FALSE, |env| {
        let region = direct_region(env, &dst, "dst", true)?;
        api::with_blob_data(handle(session), |bytes| copy_into(&region, bytes, "dst"))??;
        Ok(JNI_TRUE)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_shaderbridge_natives_ShaderBridgeNative_compileVariant<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session: jlong,
    folder: JString<'local>,
    geometry_program: JString<'local>,
    profile: JString<'local>,
) -> jstring {
    call(&mut env, std::ptr::null_mut(), |env| {
        let folder = string_arg(env, &folder, "folder")?;
        let geometry_program = string_arg(env, &geometry_program, "geometryProgram")?;
        let profile = string_arg(env, &profile, "profile")?;
        let json = api::compile_variant(handle(session), &folder, &geometry_program, &profile)?;
        new_string(env, &json)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_shaderbridge_natives_ShaderBridgeNative_variantBlobData<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session: jlong,
    dst: JByteBuffer<'local>,
) -> jboolean {
    call(&mut env, JNI_FALSE, |env| {
        let region = direct_region(env, &dst, "dst", true)?;
        api::with_variant_blob_data(handle(session), |bytes| copy_into(&region, bytes, "dst"))??;
        Ok(JNI_TRUE)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_shaderbridge_natives_ShaderBridgeNative_variantBlobSize<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session: jlong,
) -> jlong {
    call(&mut env, 0, |_| {
        let size = api::variant_blob_size(handle(session))?;
        jlong::try_from(size).map_err(|_| Error::Internal(format!("variant blob buffer of {size} bytes")))
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_shaderbridge_natives_ShaderBridgeNative_registerProfile<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    toml: JString<'local>,
) -> jstring {
    // Contract: null on success, the error message otherwise (also the last error).
    set_last_error(None);
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let toml = string_arg(&mut env, &toml, "toml")?;
        api::register_profile(&toml)
    }))
    .unwrap_or_else(|payload| Err(Error::Internal(panic_message(&*payload))));
    match result {
        Ok(_) => std::ptr::null_mut(),
        Err(e) => {
            clear_exception(&mut env);
            let message = e.to_string();
            set_last_error(Some(message.clone()));
            std::panic::catch_unwind(AssertUnwindSafe(|| new_string(&mut env, &message)))
                .ok()
                .and_then(Result::ok)
                .unwrap_or_else(|| {
                    // Even the message could not be returned: never report success.
                    clear_exception(&mut env);
                    let fallback = "registerProfile failed";
                    env.new_string(fallback).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
                })
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_shaderbridge_natives_ShaderBridgeNative_createUniformEvaluator<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session: jlong,
    folder: JString<'local>,
) -> jlong {
    call(&mut env, 0, |env| {
        let folder = string_arg(env, &folder, "folder")?;
        jhandle(api::create_uniform_evaluator(handle(session), &folder)?)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_shaderbridge_natives_ShaderBridgeNative_evaluateUniforms<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    evaluator: jlong,
    frame_block: JByteBuffer<'local>,
    frame_delta_seconds: jfloat,
) -> jboolean {
    call(&mut env, JNI_FALSE, |env| {
        let region = direct_region(env, &frame_block, "frameBlock", false)?;
        if region.len == 0 {
            return Err(Error::invalid("frameBlock has no capacity"));
        }
        // SAFETY: `region` is the whole writable memory of a direct buffer kept alive by the
        // local reference for the duration of this call; Java does not touch it while the
        // (synchronous) call runs.
        let block = unsafe { std::slice::from_raw_parts_mut(region.ptr, region.len) };
        api::evaluate_uniforms(handle(evaluator), block, frame_delta_seconds)?;
        Ok(JNI_TRUE)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_shaderbridge_natives_ShaderBridgeNative_destroyUniformEvaluator<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    evaluator: jlong,
) {
    call(&mut env, (), |_| api::destroy_uniform_evaluator(handle(evaluator)))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_shaderbridge_natives_ShaderBridgeNative_normalizeOptionValues<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    session: jlong,
    option_values: JString<'local>,
) -> jstring {
    call(&mut env, std::ptr::null_mut(), |env| {
        let option_values = string_or(env, &option_values, "optionValues", "")?;
        let text = api::normalize_option_values(handle(session), &option_values)?;
        new_string(env, &text)
    })
}
