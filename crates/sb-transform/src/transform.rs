//! Phases B and C: per-program rewriting, interface linking and emission.

use indexmap::IndexMap;
use sb_core::model::{AlphaTest, BindingTable, DepthMode, OutputTarget, UniformLayout, VertexInput};
use sb_core::{Diagnostic, Diagnostics, ShaderStage, SourceLocation};
use sb_uniforms::{MemberIndex, ProgramClass, ResourceContext};

use crate::analyze::AnalyzedStage;
use crate::profiles::DrawProfile;

/// Options of one program translation.
#[derive(Debug, Clone, PartialEq)]
pub struct TransformOptions {
    /// Vulkan (explicit set/binding) or Renderpearl (no set/binding).
    pub target: OutputTarget,
    /// Depth convention (ARCHITECTURE §4).
    pub depth_mode: DepthMode,
    /// Reversed mode only: rewrite depth-texture reads to `1 - x`, `gl_FragCoord.z` and
    /// `gl_FragDepth` writes (Iris `DepthTransformer`).
    pub invert_depth_reads: bool,
    /// Negate `gl_Position.y` in the last pre-raster stage (hosts with a negative
    /// viewport). Default false.
    pub flip_y: bool,
    /// Alpha test on fragment output 0 (`alphaTest.<prog>` or the program default);
    /// `None` for fullscreen/compute programs.
    pub alpha_test: Option<AlphaTest>,
    /// Sampler visibility class of the program.
    pub program_class: ProgramClass,
    /// The program renders the shadow pass.
    pub is_shadow_pass: bool,
    /// Constants for profile code (`SB_DH_BLOCK_ID_<n>`); missing ones default to -1.
    pub profile_constants: IndexMap<String, i64>,
    /// Explicit sampler renames: declared name -> binding-table name. Applied before
    /// canonicalization (raw `texture.<stage>.<name>` textures are normally resolved by
    /// sb-uniforms through [`ResourceContext::raw_textures`]).
    pub custom_texture_renames: IndexMap<String, String>,
    /// Physical location of logical fragment output `i` (shared gbuffer/shadow
    /// attachment list). `None` = identity. Outputs whose logical index has no entry are
    /// removed (written to a dead global).
    pub output_locations: Option<Vec<u32>>,
    /// Format qualifier for storage images declared without one (canonical image name ->
    /// `rgba16f`, ...), e.g. from `colortexNFormat`.
    pub image_formats: IndexMap<String, String>,
    /// The device can read storage images without a format qualifier
    /// (`shaderStorageImageReadWithoutFormat`).
    pub storage_image_read_without_format: bool,
    /// Emulate `sampler2DShadow` comparisons in the shader (hosts without comparison
    /// samplers).
    pub emulate_shadow_samplers: bool,
    /// Program name for diagnostics (e.g. `world0/gbuffers_terrain`).
    pub program_name: Option<String>,
}

impl Default for TransformOptions {
    fn default() -> Self {
        Self {
            target: OutputTarget::Vulkan,
            depth_mode: DepthMode::ForwardZeroToOne,
            invert_depth_reads: false,
            flip_y: false,
            alpha_test: None,
            program_class: ProgramClass::Fullscreen,
            is_shadow_pass: false,
            profile_constants: IndexMap::new(),
            custom_texture_renames: IndexMap::new(),
            output_locations: None,
            image_formats: IndexMap::new(),
            storage_image_read_without_format: true,
            emulate_shadow_samplers: false,
            program_name: None,
        }
    }
}

/// Pack-global state shared by every program of a dimension.
#[derive(Debug, Clone, Copy)]
pub struct PackContext<'a> {
    /// `sb_Frame` / `sb_Draw` layouts.
    pub layout: &'a UniformLayout,
    /// `(name, type)` -> member.
    pub members: &'a MemberIndex,
    /// Every resource's set/binding.
    pub bindings: &'a BindingTable,
    /// Canonicalization context (custom textures, images, `watershadow`).
    pub resources: &'a ResourceContext,
}

/// One translated stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransformedStage {
    /// Shader stage.
    pub stage: ShaderStage,
    /// Vulkan GLSL (`#version 460`).
    pub glsl: String,
    /// Original location of every output line (`None` for generated code).
    pub line_map: Vec<Option<SourceLocation>>,
}

/// A translated program.
#[derive(Debug, Clone, PartialEq)]
pub struct TransformedProgram {
    /// Stages in pipeline order.
    pub stages: Vec<TransformedStage>,
    /// Vertex attributes the program reads (profile inputs).
    pub vertex_inputs: Vec<VertexInput>,
    /// Fragment outputs: physical location -> base type (`float`, `int`, `uint`).
    pub fragment_outputs: Vec<(u32, String)>,
    /// Canonical binding names used, with the stages using them.
    pub resources_used: Vec<(String, Vec<ShaderStage>)>,
    /// `sb_Frame` members used.
    pub frame_members_used: Vec<String>,
    /// `sb_Draw` members used.
    pub draw_members_used: Vec<String>,
    /// The program needs features Mojang's public pipeline API cannot express.
    pub requires_raw_vulkan: bool,
    /// Warnings and notes.
    pub diagnostics: Diagnostics,
}

/// Translate every stage of one program (vertex → tessellation → geometry →
/// fragment, or a compute stage alone) for the draw `profile`.
///
/// Returns the translated stages or the error diagnostics. Never panics: internal
/// errors become `xf.internal` diagnostics.
pub fn transform_program(
    stages: &[AnalyzedStage],
    profile: &DrawProfile,
    pack: &PackContext,
    opts: &TransformOptions,
) -> Result<TransformedProgram, Diagnostics> {
    let tag = |mut d: Diagnostics| {
        if let Some(p) = &opts.program_name {
            for x in &mut d.0 {
                if x.program.is_none() {
                    x.program = Some(p.clone());
                }
            }
        }
        d
    };
    crate::stack::with_big_stack(|| crate::program::run(stages, profile, pack, opts))
        .map(|mut p| {
            p.diagnostics = tag(std::mem::take(&mut p.diagnostics));
            p
        })
        .map_err(tag)
}

/// Iris's default vertex shader for programs that ship a `.fsh` without a `.vsh`
/// (expressed with the fixed-function varyings our fragment rewrite reads).
pub const DEFAULT_VERTEX_SHADER: &str = "#version 120\n\
void main() {\n\
    gl_Position = ftransform();\n\
    gl_TexCoord[0] = gl_TextureMatrix[0] * gl_MultiTexCoord0;\n\
    gl_TexCoord[1] = gl_TextureMatrix[1] * gl_MultiTexCoord1;\n\
    gl_TexCoord[2] = gl_TextureMatrix[1] * gl_MultiTexCoord2;\n\
    gl_FrontColor = gl_Color;\n\
}\n";

pub(crate) fn error(code: &str, msg: impl Into<String>, stage: Option<ShaderStage>, loc: Option<SourceLocation>) -> Diagnostic {
    let d = Diagnostic::error(code, msg).at_opt(loc);
    match stage {
        Some(s) => d.in_stage(s),
        None => d,
    }
}
