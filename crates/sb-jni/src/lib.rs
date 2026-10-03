//! # sb-jni
//!
//! The native library of the ShaderBridge Fabric mod: the Rust side of
//! `dev.shaderbridge.natives.ShaderBridgeNative` (the JNI contract shared with
//! `java/src/main/java/dev/shaderbridge/natives/ShaderBridgeNative.java`).
//!
//! * Library name `sb_jni` (`libsb_jni.so`, `sb_jni.dll`, `libsb_jni.dylib`); the mod
//!   bundles it as `/natives/<os>-<arch>/<file>` (Gradle task `buildNative`).
//! * [`api`]: the pure-Rust API, one function per native method. The JNI exports (in the
//!   private `exports` module, symbols `Java_dev_shaderbridge_natives_ShaderBridgeNative_*`)
//!   only convert arguments and results, so everything can be tested without a JVM through
//!   the `rlib`.
//! * Native objects (pack sessions, custom-uniform evaluators) live in a registry; Java
//!   holds opaque positive `long` ids, never pointers. A closed, unknown or wrong-kind
//!   handle is an error, never undefined behaviour.
//! * Every export catches panics and never unwinds into the JVM. Failures return `0`,
//!   `null` or `false` and set a thread-local last error (`lastError()`).
//! * Heavy calls (listing, opening, options, compiles, variants, normalization) run on a
//!   dedicated thread with a 64 MiB stack; the calling JNI thread blocks on it. Compiles
//!   run inside a rayon pool of the requested size (`"threads"` in `settingsJson`, an
//!   extension of the contract; 0 or absent = one thread per CPU).
//!
//! ## Payloads
//!
//! * `compile` returns the `CompiledPack` JSON with its `blobs` index filled; `blobSize` /
//!   `blobData` then hand out the concatenated, 8-byte aligned blob buffer the index points
//!   into (kept until the session's next compile or close).
//! * `compileVariant` returns `{"program": Program, "blobs": [BlobInfo]}`, whose offsets
//!   index a separate buffer (`variantBlobSize` / `variantBlobData`).
//! * `settingsJson` is `{"dimensions": [...]|null, "validate": bool, "cacheDir": String|null,
//!   "threads": int}` (every key optional; unknown keys ignored). With `cacheDir`, compiles
//!   are cached on disk in the layout of `sb_pipeline::compile_pack`.
//! * `blobData` / `variantBlobData` write at the destination buffer's position (its
//!   position is not changed) and need `remaining() >= size`; `evaluateUniforms` treats the
//!   whole buffer (capacity) as the `sb_Frame` block. Destination buffers must be direct
//!   and writable.

#![warn(missing_docs)]

pub mod api;
pub mod error;
mod evaluator;
mod exports;
mod profiles;
mod registry;
mod session;
mod worker;

pub use error::{Error, HandleKind, Result};
pub use worker::{MAX_THREADS, WORKER_STACK_BYTES};

#[cfg(test)]
mod tests;
