//! Running the recursive parser/transform passes on a thread with a large stack.
//!
//! glsl-lang and our passes recurse over expression trees. Real shaders nest a few
//! hundred levels at most, but callers may run on small stacks (JNI threads, rayon
//! workers), and glsl-lang drops its tree recursively. Each entry point therefore runs
//! on a short-lived scoped thread with a 64 MiB stack (reserved, not committed), and
//! panics are turned into errors instead of unwinding into the caller.

/// Stack size of the worker thread.
const STACK_BYTES: usize = 64 << 20;

/// Run `f` on a large-stack thread. Returns the panic message if `f` panicked.
pub(crate) fn run<T: Send>(f: impl FnOnce() -> T + Send) -> Result<T, String> {
    let mut slot = Some(f);
    let outcome = std::thread::scope(|scope| {
        let slot_ref = &mut slot;
        let spawned = std::thread::Builder::new()
            .name("sb-transform".into())
            .stack_size(STACK_BYTES)
            .spawn_scoped(scope, move || slot_ref.take().map(|f| std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))));
        match spawned {
            Ok(handle) => Some(handle.join()),
            Err(_) => None,
        }
    });
    let result = match outcome {
        Some(Ok(Some(r))) => r,
        Some(Ok(None)) => return Err("internal error: worker closure missing".into()),
        Some(Err(p)) => Err(p),
        // The thread could not be created: run inline.
        None => match slot.take() {
            Some(f) => std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)),
            None => return Err("internal error: worker closure missing".into()),
        },
    };
    result.map_err(|p| {
        p.downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| p.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".into())
    })
}

/// [`run`] for functions returning `Result<_, Diagnostics>`: a panic becomes an
/// `xf.internal` error.
pub(crate) fn with_big_stack<T: Send>(
    f: impl FnOnce() -> Result<T, sb_core::Diagnostics> + Send,
) -> Result<T, sb_core::Diagnostics> {
    match run(f) {
        Ok(r) => r,
        Err(msg) => {
            let mut d = sb_core::Diagnostics::new();
            d.push(sb_core::Diagnostic::error("xf.internal", format!("internal transformer error: {msg}")));
            Err(d)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_and_catches_panics() {
        assert_eq!(run(|| 41 + 1), Ok(42));
        let e = run(|| -> i32 { panic!("boom") }).unwrap_err();
        assert!(e.contains("boom"));
        let r: Result<(), _> = with_big_stack(|| panic!("bad"));
        assert_eq!(r.unwrap_err().iter().next().unwrap().code, "xf.internal");
    }
}
