//! The error type of the [`api`](crate::api) layer. Its `Display` text is what Java reads
//! with `ShaderBridgeNative.lastError()`.

use std::fmt;

/// What kind of native handle a call expected (for error messages).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleKind {
    /// A pack session (`openPack`).
    Session,
    /// A custom-uniform evaluator (`createUniformEvaluator`).
    Evaluator,
}

impl fmt::Display for HandleKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            HandleKind::Session => "pack session",
            HandleKind::Evaluator => "uniform evaluator",
        })
    }
}

/// A failed native call.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The handle was never issued, was already closed, or names another kind of object.
    #[error("unknown or closed {kind} handle {handle}")]
    UnknownHandle {
        /// The expected kind of handle.
        kind: HandleKind,
        /// The handle as passed by the caller (negative JNI values are reported as 0).
        handle: u64,
    },
    /// An argument is null, malformed or out of range.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    /// The pack cannot be opened at all.
    #[error(transparent)]
    Pack(#[from] sb_pack::PackError),
    /// File system access failed.
    #[error("{context}: {source}")]
    Io {
        /// What was being done.
        context: String,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// The call needs the output of a successful `compile` on this session.
    #[error("the session has no successful compile yet")]
    NotCompiled,
    /// The requested variant or evaluator cannot be produced (with the reason).
    #[error("{0}")]
    Unavailable(String),
    /// A JSON payload could not be produced.
    #[error("cannot serialize {what}: {source}")]
    Serialize {
        /// The payload.
        what: &'static str,
        /// The serde error.
        #[source]
        source: serde_json::Error,
    },
    /// A JNI call failed (pending Java exceptions are cleared and reported here).
    #[error("JNI error: {0}")]
    Jni(String),
    /// A bug: a panic in ShaderBridge, caught before it could reach the JVM.
    #[error("internal error: {0}")]
    Internal(String),
}

/// Result of the [`api`](crate::api) layer.
pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    /// An [`Error::InvalidArgument`].
    pub(crate) fn invalid(msg: impl Into<String>) -> Self {
        Error::InvalidArgument(msg.into())
    }

    /// An [`Error::Io`].
    pub(crate) fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Error::Io { context: context.into(), source }
    }
}

/// The message of a caught panic payload.
pub(crate) fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_string())
}
