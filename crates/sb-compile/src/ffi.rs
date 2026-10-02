//! Minimal safe wrapper over the glslang C interface (`glslang-sys`).
//!
//! This is the only module of the crate that contains `unsafe` code. It exists
//! because the safe `glslang` crate (0.8) cannot express what ShaderBridge needs:
//!
//! * shader options (`AUTO_MAP_BINDINGS`, `AUTO_MAP_LOCATIONS`) must be set
//!   *before* parsing, but `glslang::Shader::new` parses immediately;
//! * auto-mapping requires `TProgram::mapIO()` between linking and SPIR-V
//!   generation, which `glslang::Program::compile` never calls;
//! * the safe wrapper panics on interior NUL bytes and on non-UTF-8 info logs,
//!   and gives no access to warnings of successful compiles or to SPIR-V
//!   generator messages.
//!
//! Process-wide initialisation is still delegated to [`glslang::Compiler::acquire`],
//! so every crate in the process shares one `glslang::InitializeProcess` call.
//!
//! Ownership model: every glslang object is created and destroyed inside one
//! call of [`compile`] on the calling thread. After process initialisation,
//! glslang compiles independent `TShader`/`TProgram` objects concurrently
//! (each thread uses its own pool allocator), which is what makes
//! [`crate::compile_glsl`] safe to call from many threads at once.

use glslang_sys as sys;
use std::ffi::{CStr, c_char, c_int, c_void};
use std::ptr::NonNull;

/// Resource limits the shaders are compiled against: glslang's
/// `DefaultTBuiltInResource` (identical to `glslang::limits::DEFAULT_LIMITS`).
///
/// Only a handful of these are enforced for Vulkan GLSL (fragment output
/// locations against `max_draw_buffers`, compute local size, texel offsets,
/// geometry output vertices, clip/cull distances); the rest only define the
/// values of `gl_Max*` built-in constants.
static RESOURCES: sys::glslang_resource_t = sys::glslang_resource_t {
    max_lights: 32,
    max_clip_planes: 6,
    max_texture_units: 32,
    max_texture_coords: 32,
    max_vertex_attribs: 64,
    max_vertex_uniform_components: 4096,
    max_varying_floats: 64,
    max_vertex_texture_image_units: 32,
    max_combined_texture_image_units: 80,
    max_texture_image_units: 32,
    max_fragment_uniform_components: 4096,
    max_draw_buffers: 32,
    max_vertex_uniform_vectors: 128,
    max_varying_vectors: 8,
    max_fragment_uniform_vectors: 16,
    max_vertex_output_vectors: 16,
    max_fragment_input_vectors: 15,
    min_program_texel_offset: -8,
    max_program_texel_offset: 7,
    max_clip_distances: 8,
    max_compute_work_group_count_x: 65535,
    max_compute_work_group_count_y: 65535,
    max_compute_work_group_count_z: 65535,
    max_compute_work_group_size_x: 1024,
    max_compute_work_group_size_y: 1024,
    max_compute_work_group_size_z: 64,
    max_compute_uniform_components: 1024,
    max_compute_texture_image_units: 16,
    max_compute_image_uniforms: 8,
    max_compute_atomic_counters: 8,
    max_compute_atomic_counter_buffers: 1,
    max_varying_components: 60,
    max_vertex_output_components: 64,
    max_geometry_input_components: 64,
    max_geometry_output_components: 128,
    max_fragment_input_components: 128,
    max_image_units: 8,
    max_combined_image_units_and_fragment_outputs: 8,
    max_combined_shader_output_resources: 8,
    max_image_samples: 0,
    max_vertex_image_uniforms: 0,
    max_tess_control_image_uniforms: 0,
    max_tess_evaluation_image_uniforms: 0,
    max_geometry_image_uniforms: 0,
    max_fragment_image_uniforms: 8,
    max_combined_image_uniforms: 8,
    max_geometry_texture_image_units: 16,
    max_geometry_output_vertices: 256,
    max_geometry_total_output_components: 1024,
    max_geometry_uniform_components: 64,
    max_geometry_varying_components: 128,
    max_tess_control_input_components: 128,
    max_tess_control_output_components: 16,
    max_tess_control_texture_image_units: 1,
    max_tess_control_uniform_components: 1024,
    max_tess_control_total_output_components: 4096,
    max_tess_evaluation_input_components: 128,
    max_tess_evaluation_output_components: 128,
    max_tess_evaluation_texture_image_units: 16,
    max_tess_evaluation_uniform_components: 1024,
    max_tess_patch_components: 0,
    max_patch_vertices: 32,
    max_tess_gen_level: 64,
    max_viewports: 16,
    max_vertex_atomic_counters: 0,
    max_tess_control_atomic_counters: 0,
    max_tess_evaluation_atomic_counters: 0,
    max_geometry_atomic_counters: 0,
    max_fragment_atomic_counters: 8,
    max_combined_atomic_counters: 8,
    max_atomic_counter_bindings: 1,
    max_vertex_atomic_counter_buffers: 0,
    max_tess_control_atomic_counter_buffers: 0,
    max_tess_evaluation_atomic_counter_buffers: 0,
    max_geometry_atomic_counter_buffers: 0,
    max_fragment_atomic_counter_buffers: 1,
    max_combined_atomic_counter_buffers: 1,
    max_atomic_counter_buffer_size: 16384,
    max_transform_feedback_buffers: 4,
    max_transform_feedback_interleaved_components: 64,
    max_cull_distances: 8,
    max_combined_clip_and_cull_distances: 8,
    max_samples: 4,
    max_mesh_output_vertices_nv: 256,
    max_mesh_output_primitives_nv: 512,
    max_mesh_work_group_size_x_nv: 32,
    max_mesh_work_group_size_y_nv: 1,
    max_mesh_work_group_size_z_nv: 1,
    max_task_work_group_size_x_nv: 32,
    max_task_work_group_size_y_nv: 1,
    max_task_work_group_size_z_nv: 1,
    max_mesh_view_count_nv: 4,
    max_mesh_output_vertices_ext: 256,
    max_mesh_output_primitives_ext: 256,
    max_mesh_work_group_size_x_ext: 128,
    max_mesh_work_group_size_y_ext: 128,
    max_mesh_work_group_size_z_ext: 128,
    max_task_work_group_size_x_ext: 128,
    max_task_work_group_size_y_ext: 128,
    max_task_work_group_size_z_ext: 128,
    max_mesh_view_count_ext: 4,
    __bindgen_anon_1: sys::glslang_resource_s__bindgen_ty_1 { max_dual_source_draw_buffers_ext: 1 },
    limits: sys::glslang_limits_t {
        non_inductive_for_loops: true,
        while_loops: true,
        do_while_loops: true,
        general_uniform_indexing: true,
        general_attribute_matrix_vector_indexing: true,
        general_varying_indexing: true,
        general_sampler_indexing: true,
        general_variable_indexing: true,
        general_constant_matrix_vector_indexing: true,
    },
};

/// Everything glslang needs to compile one stage.
#[derive(Clone, Copy)]
pub(crate) struct Job<'a> {
    /// NUL-terminated GLSL source.
    pub source: &'a CStr,
    pub stage: sys::glslang_stage_t,
    pub client_version: sys::glslang_target_client_version_t,
    pub spirv_version: sys::glslang_target_language_version_t,
    pub messages: sys::glslang_messages_t,
    pub shader_options: sys::glslang_shader_options_t,
    /// Emit `OpLine`/`OpSource` debug information.
    pub debug_info: bool,
    /// File name recorded in `OpSource`/`OpString` when `debug_info` is set.
    pub source_name: Option<&'a CStr>,
    /// Run `TProgram::mapIO()` after linking (required for auto-mapping).
    pub map_io: bool,
    /// Reject the shader after preprocessing when a statement's estimated
    /// expression depth (see `crate::guard`) exceeds this.
    pub max_statement_operators: usize,
}

/// Which glslang phase failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Init,
    Preprocess,
    Parse,
    Link,
    MapIo,
    Generate,
}

impl Phase {
    /// Human-readable phase name.
    pub fn describe(self) -> &'static str {
        match self {
            Self::Init => "initialisation",
            Self::Preprocess => "preprocessing",
            Self::Parse => "parsing",
            Self::Link => "linking",
            Self::MapIo => "I/O mapping",
            Self::Generate => "SPIR-V generation",
        }
    }
}

/// A failed compilation: the phase and the combined info logs.
#[derive(Debug)]
pub(crate) struct JobError {
    pub phase: Phase,
    pub log: String,
}

/// A successful compilation.
#[derive(Debug)]
pub(crate) struct JobOutput {
    pub spirv: Vec<u32>,
    /// Shader + program info logs (warnings), possibly empty.
    pub log: String,
    /// Messages of the SPIR-V generator (e.g. "SPIR-V generation warnings").
    pub generator_messages: String,
}

/// Owned `glslang_shader_t`, deleted on drop.
struct ShaderHandle(NonNull<sys::glslang_shader_t>);

impl Drop for ShaderHandle {
    fn drop(&mut self) {
        // SAFETY: the pointer came from `glslang_shader_create`, is non-null, and is
        // deleted exactly once. Any program that referenced it was dropped first
        // (see the declaration order in `compile`).
        unsafe { sys::glslang_shader_delete(self.0.as_ptr()) }
    }
}

/// Owned `glslang_program_t`, deleted on drop.
struct ProgramHandle(NonNull<sys::glslang_program_t>);

impl Drop for ProgramHandle {
    fn drop(&mut self) {
        // SAFETY: the pointer came from `glslang_program_create`, is non-null, and is
        // deleted exactly once. Deleting a program does not delete its shaders.
        unsafe { sys::glslang_program_delete(self.0.as_ptr()) }
    }
}

/// Copy a C string owned by glslang into a Rust `String` (lossy UTF-8).
///
/// # Safety
/// `ptr` must be null or point to a NUL-terminated string that stays valid for
/// the duration of the call.
unsafe fn c_string(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: guaranteed by the caller.
    unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned()
}

/// Include callback that refuses every `#include`.
///
/// ShaderBridge sources are fully preprocessed. Without callbacks the glslang C
/// interface would fall back to a `DirStackFileIncluder` that reads arbitrary
/// files from disk when a pack enables `GL_GOOGLE_include_directive`.
unsafe extern "C" fn deny_include(
    _ctx: *mut c_void,
    _header_name: *const c_char,
    _includer_name: *const c_char,
    _include_depth: usize,
) -> *mut sys::glsl_include_result_t {
    std::ptr::null_mut()
}

/// Release callback matching [`deny_include`] (which never allocates).
unsafe extern "C" fn free_include(_ctx: *mut c_void, _result: *mut sys::glsl_include_result_t) -> c_int {
    0
}

/// Compile one stage to SPIR-V.
pub(crate) fn compile(job: &Job<'_>) -> Result<JobOutput, JobError> {
    if glslang::Compiler::acquire().is_none() {
        return Err(JobError { phase: Phase::Init, log: "glslang process initialisation failed".into() });
    }

    // glslang's preprocessor has no expansion limits: screen macro bombs first.
    if let Some((line, message)) = crate::macros::macro_hazard(&job.source.to_string_lossy()) {
        return Err(JobError { phase: Phase::Preprocess, log: format!("ERROR: 0:{line}: '#define' : {message}\n") });
    }

    // glslang keeps a pointer to `input.code` (and to `input` itself during
    // preprocess/parse), so `input` must outlive the shader's parse calls; it lives
    // until the end of this function.
    let input = sys::glslang_input_t {
        language: sys::glslang_source_t::GLSL,
        stage: job.stage,
        client: sys::glslang_client_t::Vulkan,
        client_version: job.client_version,
        target_language: sys::glslang_target_language_t::SPIRV,
        target_language_version: job.spirv_version,
        code: job.source.as_ptr(),
        // Also used by the C interface as the Vulkan GLSL dialect version
        // (`#define VULKAN 100`); sources must carry their own `#version`.
        default_version: 100,
        default_profile: sys::glslang_profile_t::None,
        force_default_version_and_profile: 0,
        forward_compatible: 0,
        messages: job.messages,
        resource: &RESOURCES,
        callbacks: sys::glsl_include_callbacks_t {
            include_system: Some(deny_include),
            include_local: Some(deny_include),
            free_include_result: Some(free_include),
        },
        callbacks_ctx: std::ptr::null_mut(),
    };

    // SAFETY: `input` is fully initialised and `input.code` points to a
    // NUL-terminated string that outlives the shader.
    let raw = unsafe { sys::glslang_shader_create(&input) };
    let shader = ShaderHandle(NonNull::new(raw).ok_or_else(|| JobError {
        phase: Phase::Init,
        log: "glslang_shader_create returned null".into(),
    })?);
    let shader_log = |shader: &ShaderHandle| {
        // SAFETY: the shader is alive; the returned pointer is owned by it and
        // copied before any further glslang call.
        unsafe { c_string(sys::glslang_shader_get_info_log(shader.0.as_ptr())) }
    };

    // SAFETY: valid shader handle; options are plain flags. Must precede parsing.
    unsafe { sys::glslang_shader_set_options(shader.0.as_ptr(), job.shader_options.0) };

    // SAFETY: valid shader handle and `input` (see above).
    if unsafe { sys::glslang_shader_preprocess(shader.0.as_ptr(), &input) } == 0 {
        return Err(JobError { phase: Phase::Preprocess, log: shader_log(&shader) });
    }
    // Macro-expanded text that `parse` will consume: bound its type sizes and
    // expression depth (see `crate::guard`).
    // SAFETY: valid shader handle; preprocessing succeeded so the string is set,
    // and it is copied before any further glslang call.
    let preprocessed = unsafe { c_string(sys::glslang_shader_get_preprocessed_code(shader.0.as_ptr())) };
    if let Some((line, message)) = crate::guard::struct_hazard(&preprocessed) {
        return Err(JobError { phase: Phase::Preprocess, log: format!("ERROR: 0:{line}: {message}\n") });
    }
    let (depth, line) = crate::guard::max_statement_operators(&preprocessed);
    if depth > job.max_statement_operators {
        return Err(JobError {
            phase: Phase::Preprocess,
            log: format!(
                "ERROR: 0:{line}: 'expression' : too complex to compile safely (estimated nesting depth {depth} in one statement, limit {})\n",
                job.max_statement_operators
            ),
        });
    }

    // SAFETY: as above; parse reads the preprocessed text stored in the shader.
    if unsafe { sys::glslang_shader_parse(shader.0.as_ptr(), &input) } == 0 {
        return Err(JobError { phase: Phase::Parse, log: shader_log(&shader) });
    }
    let parse_log = shader_log(&shader);

    // Declared after `shader` so that it is dropped (deleted) first.
    // SAFETY: no preconditions.
    let raw = unsafe { sys::glslang_program_create() };
    let program = ProgramHandle(NonNull::new(raw).ok_or_else(|| JobError {
        phase: Phase::Init,
        log: "glslang_program_create returned null".into(),
    })?);
    let program_log = |program: &ProgramHandle| {
        // SAFETY: the program is alive; the string is copied immediately.
        unsafe { c_string(sys::glslang_program_get_info_log(program.0.as_ptr())) }
    };
    let combined = |a: &str, b: &str| -> String {
        match (a.trim().is_empty(), b.trim().is_empty()) {
            (true, _) => b.to_string(),
            (_, true) => a.to_string(),
            _ => format!("{a}\n{b}"),
        }
    };

    // SAFETY: both handles are valid; the shader outlives the program.
    unsafe { sys::glslang_program_add_shader(program.0.as_ptr(), shader.0.as_ptr()) };
    // SAFETY: valid program handle; `messages` uses the same bit layout as EShMessages.
    if unsafe { sys::glslang_program_link(program.0.as_ptr(), job.messages.0) } == 0 {
        return Err(JobError { phase: Phase::Link, log: combined(&parse_log, &program_log(&program)) });
    }
    if job.map_io {
        // SAFETY: valid, linked program handle.
        if unsafe { sys::glslang_program_map_io(program.0.as_ptr()) } == 0 {
            return Err(JobError { phase: Phase::MapIo, log: combined(&parse_log, &program_log(&program)) });
        }
    }
    if job.debug_info
        && let Some(name) = job.source_name
    {
        // SAFETY: valid program handle and NUL-terminated strings; glslang copies both.
        unsafe {
            sys::glslang_program_set_source_file(program.0.as_ptr(), job.stage, name.as_ptr());
            sys::glslang_program_add_source_text(
                program.0.as_ptr(),
                job.stage,
                job.source.as_ptr(),
                job.source.to_bytes().len(),
            );
        }
    }

    let mut spv_options = sys::glslang_spv_options_t {
        generate_debug_info: job.debug_info,
        strip_debug_info: false,
        disable_optimizer: true,
        optimize_size: false,
        // Would print to stdout (and needs SPIRV-Tools, which glslang-sys omits).
        disassemble: false,
        validate: false,
        emit_nonsemantic_shader_debug_info: false,
        emit_nonsemantic_shader_debug_source: false,
        compile_only: false,
        optimize_allow_expanded_id_bound: false,
    };
    // SAFETY: valid, linked program handle containing `job.stage`; options initialised.
    unsafe { sys::glslang_program_SPIRV_generate_with_options(program.0.as_ptr(), job.stage, &mut spv_options) };

    // SAFETY: valid program handle; the message pointer may be null.
    let generator_messages = unsafe { c_string(sys::glslang_program_SPIRV_get_messages(program.0.as_ptr())) };
    // SAFETY: valid program handle.
    let len = unsafe { sys::glslang_program_SPIRV_get_size(program.0.as_ptr()) };
    if len == 0 {
        return Err(JobError {
            phase: Phase::Generate,
            log: combined(&combined(&parse_log, &program_log(&program)), &generator_messages),
        });
    }
    let mut spirv = vec![0u32; len];
    // SAFETY: `spirv` has room for exactly `len` words, the size glslang reported.
    unsafe { sys::glslang_program_SPIRV_get(program.0.as_ptr(), spirv.as_mut_ptr()) };

    let log = combined(&parse_log, &program_log(&program));
    drop(program);
    drop(shader);
    Ok(JobOutput { spirv, log, generator_messages })
}
