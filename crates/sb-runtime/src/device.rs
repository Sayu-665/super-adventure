//! The Vulkan instance, validation messenger, device, queue, allocator and command
//! submission helpers.

use crate::error::{RuntimeError, VkResultExt};
use ash::vk;
use gpu_allocator::vulkan::{Allocation, AllocationCreateDesc, Allocator, AllocatorCreateDesc};
use std::ffi::{CStr, CString, c_void};
use std::sync::{Arc, Mutex};

/// Options for creating a [`Runtime`](crate::Runtime).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeOptions {
    /// Enable `VK_LAYER_KHRONOS_validation` and collect its errors and warnings into
    /// every [`RenderOutput`](crate::RenderOutput). If the layer is not installed the
    /// runtime continues without it ([`DeviceInfo::validation`] is then `false`).
    pub validation: bool,
    /// Prefer a CPU device (Mesa lavapipe) over GPUs.
    pub prefer_cpu_device: bool,
    /// Only consider devices whose name contains this string (case-insensitive).
    pub device_name_filter: Option<String>,
}

/// Information about the selected device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Device name (e.g. `llvmpipe (LLVM 19.1.7, 256 bits)`).
    pub name: String,
    /// Device API version `(major, minor, patch)`.
    pub api_version: (u32, u32, u32),
    /// Driver version (vendor encoding).
    pub driver_version: u32,
    /// `discrete`, `integrated`, `virtual`, `cpu` or `other`.
    pub device_type: String,
    /// Capabilities in the model's terms (for `CompileEnvironment::device`).
    pub caps: sb_core::model::DeviceCaps,
    /// The validation layer is active.
    pub validation: bool,
    /// Synchronization validation (hazard detection between commands) is active as well.
    pub sync_validation: bool,
    /// `VK_EXT_depth_clip_control` is enabled (needed for `DepthMode::GlNegOneToOne`).
    pub depth_clip_control: bool,
}

/// Severity of a collected validation message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum MessageSeverity {
    Warning,
    Error,
}

/// A message from the validation layer (or the runtime's own setup notes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidationMessage {
    pub severity: MessageSeverity,
    pub text: String,
}

impl ValidationMessage {
    pub fn render(&self) -> String {
        let s = match self.severity {
            MessageSeverity::Error => "error",
            MessageSeverity::Warning => "warning",
        };
        format!("[{s}] {}", self.text)
    }
}

type Sink = Mutex<Vec<ValidationMessage>>;

/// Most messages kept per render (a broken pack can produce one per draw).
const MAX_MESSAGES: usize = 2000;

unsafe extern "system" fn debug_callback(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _types: vk::DebugUtilsMessageTypeFlagsEXT,
    data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    user: *mut c_void,
) -> vk::Bool32 {
    if user.is_null() || data.is_null() {
        return vk::FALSE;
    }
    // SAFETY: `user` is the `Arc<Sink>` pointer registered in `Gpu::new`, kept alive
    // until the messenger is destroyed; `data` is valid for the duration of the call.
    let (sink, data) = unsafe { (&*(user as *const Sink), &*data) };
    let sev = if severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR) {
        MessageSeverity::Error
    } else if severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::WARNING) {
        MessageSeverity::Warning
    } else {
        return vk::FALSE;
    };
    // SAFETY: the strings are NUL-terminated and valid during the callback.
    let id = unsafe { data.message_id_name_as_c_str() }.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let msg = unsafe { data.message_as_c_str() }.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    if let Ok(mut v) = sink.lock()
        && v.len() < MAX_MESSAGES
    {
        v.push(ValidationMessage { severity: sev, text: if id.is_empty() { msg } else { format!("{id}: {msg}") } });
    }
    vk::FALSE
}

enum DynamicRendering {
    Core,
    Khr(ash::khr::dynamic_rendering::Device),
}

/// The device and everything that lives as long as it.
pub(crate) struct Gpu {
    _entry: ash::Entry,
    pub instance: ash::Instance,
    debug: Option<(ash::ext::debug_utils::Instance, vk::DebugUtilsMessengerEXT)>,
    debug_device: Option<ash::ext::debug_utils::Device>,
    sink: Arc<Sink>,
    /// Notes about the validation setup (layer missing, sync validation off), repeated in
    /// every render's messages.
    setup_notes: Vec<ValidationMessage>,
    pub physical: vk::PhysicalDevice,
    pub device: ash::Device,
    pub queue: vk::Queue,
    allocator: Option<Allocator>,
    pub props: vk::PhysicalDeviceProperties,
    pub features: vk::PhysicalDeviceFeatures,
    dynamic_rendering: DynamicRendering,
    pub depth_clip_control: bool,
    pub command_pool: vk::CommandPool,
    fence: vk::Fence,
    pub info: DeviceInfo,
}

fn device_type_name(t: vk::PhysicalDeviceType) -> &'static str {
    match t {
        vk::PhysicalDeviceType::DISCRETE_GPU => "discrete",
        vk::PhysicalDeviceType::INTEGRATED_GPU => "integrated",
        vk::PhysicalDeviceType::VIRTUAL_GPU => "virtual",
        vk::PhysicalDeviceType::CPU => "cpu",
        _ => "other",
    }
}

fn has_extension(exts: &[vk::ExtensionProperties], name: &CStr) -> bool {
    exts.iter().any(|e| e.extension_name_as_c_str().is_ok_and(|n| n == name))
}

impl Gpu {
    pub fn new(opts: &RuntimeOptions) -> Result<Gpu, RuntimeError> {
        // SAFETY: loading the system Vulkan loader.
        let entry = unsafe { ash::Entry::load() }.map_err(|e| RuntimeError::Loader(e.to_string()))?;
        let sink: Arc<Sink> = Arc::new(Mutex::new(Vec::new()));
        // SAFETY: plain queries on a loaded entry.
        let loader_version = unsafe { entry.try_enumerate_instance_version() }.vk("vkEnumerateInstanceVersion")?.unwrap_or(vk::API_VERSION_1_0);
        if loader_version < vk::API_VERSION_1_2 {
            return Err(RuntimeError::Unsupported(format!(
                "the Vulkan loader only supports API {}.{}",
                vk::api_version_major(loader_version),
                vk::api_version_minor(loader_version)
            )));
        }
        let api = loader_version.min(vk::API_VERSION_1_3);
        let layers = unsafe { entry.enumerate_instance_layer_properties() }.unwrap_or_default();
        let validation_layer = c"VK_LAYER_KHRONOS_validation";
        let has_validation = layers.iter().any(|l| l.layer_name_as_c_str().is_ok_and(|n| n == validation_layer));
        let inst_exts = unsafe { entry.enumerate_instance_extension_properties(None) }.unwrap_or_default();
        let has_debug_utils = has_extension(&inst_exts, ash::ext::debug_utils::NAME);
        let validation = opts.validation && has_validation && has_debug_utils;
        // Synchronization validation (hazard tracking between commands) is enabled with the
        // core checks through VK_EXT_validation_features, which the layer itself exposes.
        let layer_exts = if validation { unsafe { entry.enumerate_instance_extension_properties(Some(validation_layer)) }.unwrap_or_default() } else { Vec::new() };
        let sync_validation = validation && has_extension(&layer_exts, ash::ext::validation_features::NAME);
        if validation && !sync_validation {
            sink.lock()
                .map(|mut v| {
                    v.push(ValidationMessage {
                        severity: MessageSeverity::Warning,
                        text: "sb-runtime: the validation layer does not expose VK_EXT_validation_features; synchronization validation is off".into(),
                    })
                })
                .ok();
        }
        if opts.validation && !validation {
            sink.lock().map(|mut v| {
                v.push(ValidationMessage {
                    severity: MessageSeverity::Warning,
                    text: "sb-runtime: validation requested but VK_LAYER_KHRONOS_validation / VK_EXT_debug_utils is not available".into(),
                })
            })
            .ok();
        }

        let app_name = c"sb-runtime";
        let app = vk::ApplicationInfo::default().application_name(app_name).engine_name(app_name).api_version(api);
        let layer_ptrs = if validation { vec![validation_layer.as_ptr()] } else { Vec::new() };
        let mut ext_ptrs = if has_debug_utils { vec![ash::ext::debug_utils::NAME.as_ptr()] } else { Vec::new() };
        if sync_validation {
            ext_ptrs.push(ash::ext::validation_features::NAME.as_ptr());
        }
        let enabled_features = [vk::ValidationFeatureEnableEXT::SYNCHRONIZATION_VALIDATION];
        let mut validation_features = vk::ValidationFeaturesEXT::default().enabled_validation_features(&enabled_features);
        let user = Arc::as_ptr(&sink) as *mut c_void;
        let mut messenger_info = vk::DebugUtilsMessengerCreateInfoEXT::default()
            .message_severity(vk::DebugUtilsMessageSeverityFlagsEXT::WARNING | vk::DebugUtilsMessageSeverityFlagsEXT::ERROR)
            .message_type(
                vk::DebugUtilsMessageTypeFlagsEXT::GENERAL | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
            )
            .pfn_user_callback(Some(debug_callback))
            .user_data(user);
        let mut create = vk::InstanceCreateInfo::default().application_info(&app).enabled_layer_names(&layer_ptrs).enabled_extension_names(&ext_ptrs);
        if validation {
            create = create.push_next(&mut messenger_info);
        }
        if sync_validation {
            create = create.push_next(&mut validation_features);
        }
        // SAFETY: valid create info; the sink outlives the instance (dropped after it).
        let instance = unsafe { entry.create_instance(&create, None) }.vk("vkCreateInstance")?;
        let debug = if validation {
            let du = ash::ext::debug_utils::Instance::new(&entry, &instance);
            let info = vk::DebugUtilsMessengerCreateInfoEXT::default()
                .message_severity(vk::DebugUtilsMessageSeverityFlagsEXT::WARNING | vk::DebugUtilsMessageSeverityFlagsEXT::ERROR)
                .message_type(
                    vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                        | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                        | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
                )
                .pfn_user_callback(Some(debug_callback))
                .user_data(user);
            match unsafe { du.create_debug_utils_messenger(&info, None) } {
                Ok(m) => Some((du, m)),
                Err(e) => {
                    unsafe { instance.destroy_instance(None) };
                    return Err(RuntimeError::vk("vkCreateDebugUtilsMessengerEXT", e));
                }
            }
        } else {
            None
        };
        // From here on, failures must destroy the instance: wrap in a guard.
        let mut guard = InstanceGuard { instance: Some(instance), debug };
        let instance = guard.instance.as_ref().expect("instance present");

        let physicals = unsafe { instance.enumerate_physical_devices() }.vk("vkEnumeratePhysicalDevices")?;
        let mut candidates = Vec::new();
        let mut seen = Vec::new();
        for pd in physicals {
            let props = unsafe { instance.get_physical_device_properties(pd) };
            let name = props.device_name_as_c_str().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            seen.push(format!("{name} ({}, API {}.{})", device_type_name(props.device_type), vk::api_version_major(props.api_version), vk::api_version_minor(props.api_version)));
            if let Some(filter) = &opts.device_name_filter
                && !name.to_lowercase().contains(&filter.to_lowercase())
            {
                continue;
            }
            let exts = unsafe { instance.enumerate_device_extension_properties(pd) }.unwrap_or_default();
            let core13 = props.api_version >= vk::API_VERSION_1_3 && api >= vk::API_VERSION_1_3;
            let khr_dr = has_extension(&exts, ash::khr::dynamic_rendering::NAME);
            if props.api_version < vk::API_VERSION_1_2 || !(core13 || khr_dr) {
                continue;
            }
            let families = unsafe { instance.get_physical_device_queue_family_properties(pd) };
            let Some(family) = families.iter().position(|f| f.queue_flags.contains(vk::QueueFlags::GRAPHICS | vk::QueueFlags::COMPUTE)) else {
                continue;
            };
            let rank = match props.device_type {
                vk::PhysicalDeviceType::CPU if opts.prefer_cpu_device => 0,
                vk::PhysicalDeviceType::DISCRETE_GPU => 1,
                vk::PhysicalDeviceType::INTEGRATED_GPU => 2,
                vk::PhysicalDeviceType::VIRTUAL_GPU => 3,
                vk::PhysicalDeviceType::CPU => 4,
                _ => 5,
            };
            candidates.push((rank, pd, props, family as u32, core13, exts));
        }
        candidates.sort_by_key(|c| c.0);
        let Some((_, physical, props, family, core13, exts)) = candidates.into_iter().next() else {
            return Err(RuntimeError::NoDevice(format!(
                "need Vulkan 1.3 (or 1.2 + VK_KHR_dynamic_rendering) with a graphics+compute queue{}; devices: [{}]",
                opts.device_name_filter.as_ref().map(|f| format!(" matching `{f}`")).unwrap_or_default(),
                seen.join(", ")
            )));
        };

        // Enable every supported core feature (packs use a wide range of SPIR-V
        // capabilities), plus dynamic rendering and depth clip control.
        let mut f11 = vk::PhysicalDeviceVulkan11Features::default();
        let mut f12 = vk::PhysicalDeviceVulkan12Features::default();
        let mut f13 = vk::PhysicalDeviceVulkan13Features::default();
        let mut dr = vk::PhysicalDeviceDynamicRenderingFeatures::default();
        let mut dcc = vk::PhysicalDeviceDepthClipControlFeaturesEXT::default();
        let has_dcc = has_extension(&exts, ash::ext::depth_clip_control::NAME);
        {
            let mut q = vk::PhysicalDeviceFeatures2::default().push_next(&mut f11).push_next(&mut f12);
            if core13 {
                q = q.push_next(&mut f13);
            } else {
                q = q.push_next(&mut dr);
            }
            if has_dcc {
                q = q.push_next(&mut dcc);
            }
            unsafe { instance.get_physical_device_features2(physical, &mut q) };
        }
        let base_features = unsafe { instance.get_physical_device_features(physical) };
        let mut f11 = vk::PhysicalDeviceVulkan11Features { p_next: std::ptr::null_mut(), ..f11 };
        let mut f12 = vk::PhysicalDeviceVulkan12Features { p_next: std::ptr::null_mut(), ..f12 };
        let mut f13 = vk::PhysicalDeviceVulkan13Features { p_next: std::ptr::null_mut(), ..f13 };
        let dr_supported = if core13 { f13.dynamic_rendering == vk::TRUE } else { dr.dynamic_rendering == vk::TRUE };
        if !dr_supported {
            return Err(RuntimeError::NoDevice("the selected device does not support dynamic rendering".into()));
        }
        let mut dr = vk::PhysicalDeviceDynamicRenderingFeatures::default().dynamic_rendering(true);
        let depth_clip_control = has_dcc && dcc.depth_clip_control == vk::TRUE;
        let mut dcc = vk::PhysicalDeviceDepthClipControlFeaturesEXT::default().depth_clip_control(true);
        let mut dev_exts: Vec<*const std::ffi::c_char> = Vec::new();
        if !core13 {
            dev_exts.push(ash::khr::dynamic_rendering::NAME.as_ptr());
        }
        if depth_clip_control {
            dev_exts.push(ash::ext::depth_clip_control::NAME.as_ptr());
        }
        let priorities = [1.0f32];
        let queue_info = [vk::DeviceQueueCreateInfo::default().queue_family_index(family).queue_priorities(&priorities)];
        let mut features2 = vk::PhysicalDeviceFeatures2::default().features(base_features).push_next(&mut f11).push_next(&mut f12);
        if core13 {
            features2 = features2.push_next(&mut f13);
        } else {
            features2 = features2.push_next(&mut dr);
        }
        if depth_clip_control {
            features2 = features2.push_next(&mut dcc);
        }
        let dinfo = vk::DeviceCreateInfo::default().queue_create_infos(&queue_info).enabled_extension_names(&dev_exts).push_next(&mut features2);
        let device = unsafe { instance.create_device(physical, &dinfo, None) }.vk("vkCreateDevice")?;
        let queue = unsafe { device.get_device_queue(family, 0) };
        let dynamic_rendering = if core13 { DynamicRendering::Core } else { DynamicRendering::Khr(ash::khr::dynamic_rendering::Device::new(instance, &device)) };
        let debug_device = guard.debug.as_ref().map(|_| ash::ext::debug_utils::Device::new(instance, &device));

        let cleanup_device = |device: &ash::Device| unsafe { device.destroy_device(None) };
        let allocator = match Allocator::new(&AllocatorCreateDesc {
            instance: instance.clone(),
            device: device.clone(),
            physical_device: physical,
            debug_settings: Default::default(),
            buffer_device_address: false,
            allocation_sizes: Default::default(),
        }) {
            Ok(a) => a,
            Err(e) => {
                cleanup_device(&device);
                return Err(e.into());
            }
        };
        let pool_info = vk::CommandPoolCreateInfo::default().queue_family_index(family).flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let command_pool = match unsafe { device.create_command_pool(&pool_info, None) } {
            Ok(p) => p,
            Err(e) => {
                drop(allocator);
                cleanup_device(&device);
                return Err(RuntimeError::vk("vkCreateCommandPool", e));
            }
        };
        let fence = match unsafe { device.create_fence(&vk::FenceCreateInfo::default(), None) } {
            Ok(f) => f,
            Err(e) => {
                unsafe { device.destroy_command_pool(command_pool, None) };
                drop(allocator);
                cleanup_device(&device);
                return Err(RuntimeError::vk("vkCreateFence", e));
            }
        };

        let caps = sb_core::model::DeviceCaps {
            geometry_shader: base_features.geometry_shader == vk::TRUE,
            tessellation_shader: base_features.tessellation_shader == vk::TRUE,
            storage_image_read_without_format: base_features.shader_storage_image_read_without_format == vk::TRUE,
            storage_image_write_without_format: base_features.shader_storage_image_write_without_format == vk::TRUE,
            depth_clip_control,
            max_push_constants_size: props.limits.max_push_constants_size,
            max_color_attachments: props.limits.max_color_attachments,
            ..Default::default()
        };
        let info = DeviceInfo {
            name: props.device_name_as_c_str().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            api_version: (vk::api_version_major(props.api_version), vk::api_version_minor(props.api_version), vk::api_version_patch(props.api_version)),
            driver_version: props.driver_version,
            device_type: device_type_name(props.device_type).to_string(),
            caps,
            validation,
            sync_validation,
            depth_clip_control,
        };
        let (instance, debug) = guard.take();
        let setup_notes = sink.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default();
        Ok(Gpu {
            _entry: entry,
            instance,
            debug,
            debug_device,
            sink,
            setup_notes,
            physical,
            device,
            queue,
            allocator: Some(allocator),
            props,
            features: base_features,
            dynamic_rendering,
            depth_clip_control,
            command_pool,
            fence,
            info,
        })
    }

    /// Take the collected validation messages.
    pub fn take_messages(&self) -> Vec<ValidationMessage> {
        self.sink.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default()
    }

    /// Notes about the validation setup itself (empty when everything requested is active).
    pub fn setup_notes(&self) -> &[ValidationMessage] {
        &self.setup_notes
    }

    pub fn limits(&self) -> &vk::PhysicalDeviceLimits {
        &self.props.limits
    }

    /// Optimal-tiling features of `format`.
    pub fn format_features(&self, format: vk::Format) -> vk::FormatFeatureFlags {
        unsafe { self.instance.get_physical_device_format_properties(self.physical, format) }.optimal_tiling_features
    }

    /// Buffer features of `format` (vertex buffer support).
    pub fn buffer_format_features(&self, format: vk::Format) -> vk::FormatFeatureFlags {
        unsafe { self.instance.get_physical_device_format_properties(self.physical, format) }.buffer_features
    }

    pub fn allocate(&mut self, desc: &AllocationCreateDesc<'_>) -> Result<Allocation, RuntimeError> {
        match self.allocator.as_mut() {
            Some(a) => Ok(a.allocate(desc)?),
            None => Err(RuntimeError::Allocation("allocator already destroyed".into())),
        }
    }

    pub fn free(&mut self, allocation: Allocation) {
        if let Some(a) = self.allocator.as_mut() {
            let _ = a.free(allocation);
        }
    }

    /// Give a Vulkan object a debug name (shown in validation messages).
    pub fn set_name<H: vk::Handle>(&self, handle: H, name: &str) {
        if let Some(du) = &self.debug_device
            && let Ok(name) = CString::new(name.replace('\0', " "))
        {
            let info = vk::DebugUtilsObjectNameInfoEXT::default().object_handle(handle).object_name(&name);
            let _ = unsafe { du.set_debug_utils_object_name(&info) };
        }
    }

    pub fn cmd_begin_rendering(&self, cmd: vk::CommandBuffer, info: &vk::RenderingInfo<'_>) {
        // SAFETY: the caller records into a command buffer in the recording state.
        unsafe {
            match &self.dynamic_rendering {
                DynamicRendering::Core => self.device.cmd_begin_rendering(cmd, info),
                DynamicRendering::Khr(k) => k.cmd_begin_rendering(cmd, info),
            }
        }
    }

    pub fn cmd_end_rendering(&self, cmd: vk::CommandBuffer) {
        // SAFETY: as above.
        unsafe {
            match &self.dynamic_rendering {
                DynamicRendering::Core => self.device.cmd_end_rendering(cmd),
                DynamicRendering::Khr(k) => k.cmd_end_rendering(cmd),
            }
        }
    }

    /// Allocate a primary command buffer.
    pub fn allocate_command_buffer(&self) -> Result<vk::CommandBuffer, RuntimeError> {
        let info = vk::CommandBufferAllocateInfo::default().command_pool(self.command_pool).level(vk::CommandBufferLevel::PRIMARY).command_buffer_count(1);
        let v = unsafe { self.device.allocate_command_buffers(&info) }.vk("vkAllocateCommandBuffers")?;
        v.into_iter().next().ok_or(RuntimeError::Vulkan { call: "vkAllocateCommandBuffers", result: vk::Result::ERROR_UNKNOWN })
    }

    pub fn free_command_buffer(&self, cmd: vk::CommandBuffer) {
        unsafe { self.device.free_command_buffers(self.command_pool, &[cmd]) };
    }

    /// Submit a recorded command buffer and wait for it.
    pub fn submit_and_wait(&self, cmd: vk::CommandBuffer) -> Result<(), RuntimeError> {
        let cmds = [cmd];
        let submit = [vk::SubmitInfo::default().command_buffers(&cmds)];
        unsafe {
            self.device.reset_fences(&[self.fence]).vk("vkResetFences")?;
            self.device.queue_submit(self.queue, &submit, self.fence).vk("vkQueueSubmit")?;
            self.device.wait_for_fences(&[self.fence], true, u64::MAX).vk("vkWaitForFences")?;
        }
        Ok(())
    }

    /// Record and run a one-time command buffer.
    pub fn one_shot(&self, record: impl FnOnce(&ash::Device, vk::CommandBuffer)) -> Result<(), RuntimeError> {
        let cmd = self.allocate_command_buffer()?;
        let result = (|| {
            let begin = vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
            unsafe { self.device.begin_command_buffer(cmd, &begin) }.vk("vkBeginCommandBuffer")?;
            record(&self.device, cmd);
            unsafe { self.device.end_command_buffer(cmd) }.vk("vkEndCommandBuffer")?;
            self.submit_and_wait(cmd)
        })();
        self.free_command_buffer(cmd);
        result
    }

    pub fn wait_idle(&self) -> Result<(), RuntimeError> {
        unsafe { self.device.device_wait_idle() }.vk("vkDeviceWaitIdle")
    }
}

impl Drop for Gpu {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_fence(self.fence, None);
            self.device.destroy_command_pool(self.command_pool, None);
        }
        // The allocator must go before the device.
        self.allocator.take();
        unsafe {
            self.device.destroy_device(None);
            if let Some((du, m)) = self.debug.take() {
                du.destroy_debug_utils_messenger(m, None);
            }
            self.instance.destroy_instance(None);
        }
    }
}

/// Destroys the instance (and messenger) if device creation fails half-way.
struct InstanceGuard {
    instance: Option<ash::Instance>,
    debug: Option<(ash::ext::debug_utils::Instance, vk::DebugUtilsMessengerEXT)>,
}

impl InstanceGuard {
    fn take(&mut self) -> (ash::Instance, Option<(ash::ext::debug_utils::Instance, vk::DebugUtilsMessengerEXT)>) {
        (self.instance.take().expect("instance taken once"), self.debug.take())
    }
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        if let Some(instance) = self.instance.take() {
            unsafe {
                if let Some((du, m)) = self.debug.take() {
                    du.destroy_debug_utils_messenger(m, None);
                }
                instance.destroy_instance(None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The messenger really collects validation errors: a deliberately invalid call
    /// (a zero-sized buffer, VUID-VkBufferCreateInfo-size-00912) must be reported.
    #[test]
    fn validation_messages_are_collected() {
        let gpu = match Gpu::new(&RuntimeOptions { validation: true, prefer_cpu_device: true, device_name_filter: None }) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("skipping: {e}");
                return;
            }
        };
        if !gpu.info.validation {
            eprintln!("skipping: validation layer not installed");
            return;
        }
        let _ = gpu.take_messages();
        let info = vk::BufferCreateInfo::default().size(0).usage(vk::BufferUsageFlags::UNIFORM_BUFFER);
        if let Ok(b) = unsafe { gpu.device.create_buffer(&info, None) } {
            unsafe { gpu.device.destroy_buffer(b, None) };
        }
        let msgs = gpu.take_messages();
        assert!(msgs.iter().any(|m| m.severity == MessageSeverity::Error && m.text.contains("00912")), "{msgs:?}");
        assert!(msgs.iter().all(|m| m.render().starts_with('[')));
    }

    /// Synchronization validation is active with the core checks: two transfer writes to
    /// the same buffer without a barrier between them must be reported as a hazard.
    #[test]
    fn sync_validation_reports_hazards() {
        let mut gpu = match Gpu::new(&RuntimeOptions { validation: true, prefer_cpu_device: true, device_name_filter: None }) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("skipping: {e}");
                return;
            }
        };
        if !gpu.info.validation {
            eprintln!("skipping: validation layer not installed");
            return;
        }
        assert!(gpu.info.sync_validation, "validation is on but synchronization validation is not");
        assert!(gpu.setup_notes().is_empty(), "{:?}", gpu.setup_notes());
        let _ = gpu.take_messages();
        let mut arena = crate::resources::Arena::default();
        let buf = arena.create_buffer(&mut gpu, "hazard", 256, vk::BufferUsageFlags::TRANSFER_DST, gpu_allocator::MemoryLocation::GpuOnly).expect("buffer");
        let b = arena.buffer(buf).buffer;
        gpu.one_shot(|d, cmd| unsafe {
            d.cmd_fill_buffer(cmd, b, 0, 256, 1);
            d.cmd_fill_buffer(cmd, b, 0, 256, 2);
        })
        .expect("submit");
        arena.destroy_all(&mut gpu);
        let msgs = gpu.take_messages();
        assert!(msgs.iter().any(|m| m.text.contains("SYNC-HAZARD-WRITE-AFTER-WRITE")), "{msgs:?}");
    }
}
