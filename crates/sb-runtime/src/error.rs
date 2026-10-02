//! Errors of the runtime.

use ash::vk;

/// Errors returned by [`Runtime`](crate::Runtime) operations.
///
/// Model inconsistencies (a broken program, a missing blob, an unknown resource) are
/// never errors: the offending program is skipped and the reason is recorded in
/// [`FrameStats`](crate::FrameStats). Errors are reserved for conditions that prevent
/// rendering altogether.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    /// The Vulkan loader (`libvulkan`) could not be loaded.
    #[error("the Vulkan loader is not available: {0}")]
    Loader(String),
    /// No physical device satisfies the requirements (or the name filter).
    #[error("no suitable Vulkan device: {0}")]
    NoDevice(String),
    /// A Vulkan call failed.
    #[error("Vulkan call `{call}` failed: {result:?}")]
    Vulkan {
        /// The failing entry point.
        call: &'static str,
        /// The returned result code.
        result: vk::Result,
    },
    /// The device was lost (driver crash, GPU hang, or a fault caused by a shader).
    #[error("the Vulkan device was lost during `{0}`")]
    DeviceLost(&'static str),
    /// GPU memory allocation failed.
    #[error("GPU memory allocation failed: {0}")]
    Allocation(String),
    /// The request cannot be served (zero size, unknown dimension folder, ...).
    #[error("invalid render request: {0}")]
    InvalidRequest(String),
    /// The device lacks a capability the request needs.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// Writing an output file failed.
    #[error("cannot write `{path}`: {message}")]
    Io {
        /// The file that could not be written.
        path: String,
        /// The underlying error.
        message: String,
    },
}

impl RuntimeError {
    /// Wrap a failed Vulkan call, mapping `VK_ERROR_DEVICE_LOST` to
    /// [`RuntimeError::DeviceLost`].
    pub(crate) fn vk(call: &'static str, result: vk::Result) -> Self {
        if result == vk::Result::ERROR_DEVICE_LOST {
            RuntimeError::DeviceLost(call)
        } else {
            RuntimeError::Vulkan { call, result }
        }
    }
}

/// Shorthand for `map_err(|e| RuntimeError::vk(call, e))`.
pub(crate) trait VkResultExt<T> {
    fn vk(self, call: &'static str) -> Result<T, RuntimeError>;
}

impl<T> VkResultExt<T> for Result<T, vk::Result> {
    fn vk(self, call: &'static str) -> Result<T, RuntimeError> {
        self.map_err(|e| RuntimeError::vk(call, e))
    }
}

impl From<gpu_allocator::AllocationError> for RuntimeError {
    fn from(e: gpu_allocator::AllocationError) -> Self {
        RuntimeError::Allocation(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_lost_is_reported_separately() {
        assert!(matches!(
            RuntimeError::vk("vkQueueSubmit", vk::Result::ERROR_DEVICE_LOST),
            RuntimeError::DeviceLost("vkQueueSubmit")
        ));
        assert!(matches!(
            RuntimeError::vk("vkCreateImage", vk::Result::ERROR_OUT_OF_DEVICE_MEMORY),
            RuntimeError::Vulkan { call: "vkCreateImage", .. }
        ));
    }
}
