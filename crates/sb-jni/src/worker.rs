//! Where heavy calls run: a dedicated thread with a 64 MiB stack (the calling JNI thread
//! blocks on it), optionally inside a rayon pool of a requested size.
//!
//! JNI threads, and Minecraft's worker threads in particular, may have small stacks. The
//! pipeline's own recursive stages (the GLSL parser and transformer, glslang) already move
//! to large-stack threads, but everything else of a compile (preprocessing, properties,
//! expressions, serialization) runs on the thread that calls it, so heavy calls never run
//! on the caller's stack. The stack is reserved address space, not committed memory.

use crate::error::{Error, Result, panic_message};
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, LazyLock, Mutex};

/// Stack size of the worker thread and of the threads of sized compile pools.
pub const WORKER_STACK_BYTES: usize = 64 << 20;

/// Largest thread count accepted for a compile pool.
pub const MAX_THREADS: usize = 256;

/// Run `f` with panics turned into [`Error::Internal`].
pub(crate) fn catch<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    std::panic::catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|p| Err(Error::Internal(panic_message(&*p))))
}

/// Run `f` on a fresh thread with a [`WORKER_STACK_BYTES`] stack and wait for it. Panics
/// become [`Error::Internal`]. If the thread cannot be created, `f` runs on the calling
/// thread instead.
pub(crate) fn run<T: Send>(f: impl FnOnce() -> Result<T> + Send) -> Result<T> {
    let mut slot = Some(f);
    let joined = std::thread::scope(|scope| {
        let slot_ref = &mut slot;
        std::thread::Builder::new()
            .name("sb-jni-worker".into())
            .stack_size(WORKER_STACK_BYTES)
            .spawn_scoped(scope, move || slot_ref.take().map(catch))
            .map(|handle| handle.join())
    });
    match joined {
        Ok(Ok(Some(result))) => result,
        Ok(Ok(None)) => Err(Error::Internal("the worker closure was already taken".into())),
        Ok(Err(payload)) => Err(Error::Internal(panic_message(&*payload))),
        // The thread could not be spawned: the closure was not consumed.
        Err(_) => match slot.take() {
            Some(f) => catch(f),
            None => Err(Error::Internal("the worker closure was already taken".into())),
        },
    }
}

static POOLS: LazyLock<Mutex<HashMap<usize, Arc<rayon::ThreadPool>>>> = LazyLock::new(Default::default);

/// The cached rayon pool with `threads` threads (built on first use).
fn pool(threads: usize) -> Option<Arc<rayon::ThreadPool>> {
    let mut pools = POOLS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(p) = pools.get(&threads) {
        return Some(p.clone());
    }
    let built = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .stack_size(WORKER_STACK_BYTES)
        .thread_name(move |i| format!("sb-jni-compile-{threads}-{i}"))
        .build()
        .ok()?;
    let p = Arc::new(built);
    pools.insert(threads, p.clone());
    Some(p)
}

/// Run `f` inside a rayon pool of `threads` threads (`0` = rayon's global pool, which uses
/// one thread per CPU), so the pipeline's parallel steps use at most that many threads.
/// `f` itself runs on a pool thread (with a [`WORKER_STACK_BYTES`] stack). If the pool
/// cannot be built, `f` runs in the global pool.
pub(crate) fn in_pool<T: Send>(threads: usize, f: impl FnOnce() -> T + Send) -> T {
    match (threads > 0).then(|| pool(threads.min(MAX_THREADS))).flatten() {
        Some(p) => p.install(f),
        None => f(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_on_a_large_stack_and_catches_panics() {
        // 24 MiB of stack frames would overflow any default thread stack.
        fn deep(n: u32) -> u64 {
            let pad = std::hint::black_box([n as u8; 4096]);
            if n == 0 { u64::from(pad[0]) } else { deep(n - 1) + u64::from(pad[1]) }
        }
        let depth = (24 << 20) / 4096;
        assert_eq!(run(|| Ok(deep(depth))).unwrap(), (1..=u64::from(depth)).map(|i| i % 256).sum::<u64>());
        let name = run(|| Ok(std::thread::current().name().map(str::to_string))).unwrap();
        assert_eq!(name.as_deref(), Some("sb-jni-worker"));

        let e = run(|| -> Result<()> { panic!("boom {}", 42) }).unwrap_err();
        assert!(matches!(&e, Error::Internal(m) if m.contains("boom 42")), "{e}");
        let e = run(|| -> Result<()> { std::panic::panic_any(7_u8) }).unwrap_err();
        assert!(matches!(&e, Error::Internal(m) if m == "unknown panic"), "{e}");
    }

    #[test]
    fn sized_pools_limit_parallelism() {
        let n = in_pool(3, rayon::current_num_threads);
        assert_eq!(n, 3);
        let name = in_pool(2, || std::thread::current().name().map(str::to_string)).unwrap_or_default();
        assert!(name.starts_with("sb-jni-compile-2-"), "{name}");
        // The pool is reused.
        assert!(Arc::ptr_eq(&pool(3).unwrap(), &pool(3).unwrap()));
        // 0 = the global pool.
        assert_eq!(in_pool(0, rayon::current_num_threads), rayon::current_num_threads());
        // Panics propagate out of the pool (and are caught by `run`).
        let e = run(|| in_pool(2, || -> Result<()> { panic!("in pool") })).unwrap_err();
        assert!(e.to_string().contains("in pool"), "{e}");
    }
}
