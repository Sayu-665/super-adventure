//! Spec step 11: translating and compiling one (program, draw profile) variant for every
//! requested target, with content-addressed caches for SPIR-V, validation and the
//! Renderpearl compile check.

use crate::analysis::{Key, Memo, tag};
use indexmap::IndexMap;
use sb_compile::{CompileOptions, Reflection, ValidationResult, VulkanTarget};
use sb_core::model::{AlphaTest, CompileEnvironment, DepthMode, OutputTarget, ProgramKind, VertexInput};
use sb_core::{Diagnostic, Diagnostics, ShaderStage};
use sb_transform::{AnalyzedStage, DrawProfile, PackContext, TransformOptions};
use sb_uniforms::ProgramClass;
use std::sync::Arc;

/// A compiled Vulkan module: SPIR-V, its reflection and glslang's warnings.
pub type SpirvModule = Arc<(Vec<u32>, Reflection, Vec<Diagnostic>)>;

/// Outcome of compiling one Vulkan GLSL stage (cached).
pub type SpirvResult = Result<SpirvModule, Arc<Vec<Diagnostic>>>;

/// Shared caches of a session.
#[derive(Debug, Default)]
pub struct Caches {
    pub analysis: Memo<crate::analysis::AnalysisResult>,
    /// Vulkan GLSL → SPIR-V + reflection (or the compile errors).
    pub spirv: Memo<SpirvResult>,
    pub validation: Memo<ValidationResult>,
    /// Renderpearl GLSL compile check.
    pub renderpearl: Memo<Result<(), Arc<Vec<Diagnostic>>>>,
}

fn key(parts: &[&[u8]]) -> Key {
    let mut h = blake3::Hasher::new();
    for p in parts {
        h.update(&(p.len() as u64).to_le_bytes());
        h.update(p);
    }
    *h.finalize().as_bytes()
}

/// Everything needed to translate one variant.
#[derive(Debug, Clone)]
pub struct VariantJob<'a> {
    /// Model program name (`world0/gbuffers_terrain`).
    pub name: String,
    pub stages: Vec<AnalyzedStage>,
    /// Original (or virtual) file of each stage.
    pub files: Vec<(ShaderStage, String)>,
    pub profile: &'a DrawProfile,
    pub class: ProgramClass,
    pub shadow_pass: bool,
    pub alpha_test: Option<AlphaTest>,
    pub output_locations: Vec<u32>,
    pub profile_constants: IndexMap<String, i64>,
    pub kind: ProgramKind,
}

/// One compiled stage.
#[derive(Debug, Clone)]
pub struct CompiledStage {
    pub stage: ShaderStage,
    pub source_file: String,
    pub spirv: Option<Vec<u32>>,
    pub glsl_vulkan: Option<String>,
    pub glsl_renderpearl: Option<String>,
}

/// A compiled variant.
#[derive(Debug, Clone)]
pub struct CompiledVariant {
    pub stages: Vec<CompiledStage>,
    pub vertex_inputs: Vec<VertexInput>,
    /// Physical location → base type.
    pub fragment_outputs: Vec<(u32, String)>,
    /// Binding-table names with the stages using them.
    pub resources_used: Vec<(String, Vec<ShaderStage>)>,
    pub requires_raw_vulkan: bool,
    pub push_constant_size: u32,
    pub local_size: Option<[u32; 3]>,
    /// Distinct descriptors over all stages (`None` without SPIR-V).
    pub descriptor_count: Option<u32>,
    pub reflections: Vec<Reflection>,
    /// Modules that ran through `spirv-val` successfully.
    pub validated_modules: u32,
    /// Warnings and notes.
    pub diagnostics: Diagnostics,
}

/// Settings shared by every job of a compile.
pub struct JobEnv<'a> {
    pub env: &'a CompileEnvironment,
    pub validate: bool,
    pub caches: &'a Caches,
    pub pack: PackContext<'a>,
    pub image_formats: &'a IndexMap<String, String>,
}

fn transform_options(job: &VariantJob<'_>, env: &CompileEnvironment, target: OutputTarget, formats: &IndexMap<String, String>) -> TransformOptions {
    // Fields not set here keep their defaults (e.g. `draw_parameters`: hosts draw with
    // base instance 0, so `gl_InstanceID` is plain `gl_InstanceIndex`).
    TransformOptions {
        target,
        depth_mode: env.depth_mode,
        invert_depth_reads: env.depth_mode == DepthMode::ReversedZeroToOne,
        flip_y: false,
        alpha_test: job.alpha_test,
        program_class: job.class,
        is_shadow_pass: job.shadow_pass,
        profile_constants: job.profile_constants.clone(),
        output_locations: Some(job.output_locations.clone()),
        image_formats: formats.clone(),
        storage_image_read_without_format: env.device.storage_image_read_without_format,
        emulate_shadow_samplers: !env.device.comparison_samplers,
        program_name: Some(job.name.clone()),
        ..TransformOptions::default()
    }
}

fn compile_vulkan(
    glsl: &str,
    stage: ShaderStage,
    file: &str,
    line_map: &[Option<sb_core::SourceLocation>],
    caches: &Caches,
) -> SpirvResult {
    let k = key(&[b"vk-v1", stage.name().as_bytes(), file.as_bytes(), glsl.as_bytes()]);
    if let Some(r) = caches.spirv.get(&k) {
        return r;
    }
    let r = match sb_compile::compile_glsl_detailed(glsl, stage, file, &CompileOptions::default(), Some(line_map)) {
        Ok(out) => match sb_compile::reflect(&out.spirv) {
            Ok(refl) => Ok(Arc::new((out.spirv.clone(), refl, out.warning_diagnostics(stage)))),
            Err(e) => Err(Arc::new(vec![Diagnostic::error("spv.reflect", format!("SPIR-V reflection failed: {e}")).in_stage(stage)])),
        },
        Err(f) => Err(Arc::new(f.to_diagnostics())),
    };
    caches.spirv.insert(k, r.clone());
    r
}

fn validate(spirv: &[u32], caches: &Caches) -> ValidationResult {
    let bytes: Vec<u8> = spirv.iter().flat_map(|w| w.to_le_bytes()).collect();
    let k = key(&[b"val-v1", &bytes]);
    if let Some(r) = caches.validation.get(&k) {
        return r;
    }
    let r = sb_compile::validate(spirv, VulkanTarget::Vulkan1_2);
    caches.validation.insert(k, r.clone());
    r
}

fn check_renderpearl(
    glsl: &str,
    stage: ShaderStage,
    file: &str,
    line_map: &[Option<sb_core::SourceLocation>],
    caches: &Caches,
) -> Result<(), Arc<Vec<Diagnostic>>> {
    let k = key(&[b"rp-v1", stage.name().as_bytes(), file.as_bytes(), glsl.as_bytes()]);
    if let Some(r) = caches.renderpearl.get(&k) {
        return r;
    }
    let r = sb_compile::compile_glsl(glsl, stage, file, &CompileOptions::auto_mapped(), Some(line_map))
        .map(|_| ())
        .map_err(|f| {
            Arc::new(
                f.to_diagnostics()
                    .into_iter()
                    .map(|mut d| {
                        d.code = "rp.compile".into();
                        d.message = format!("Renderpearl GLSL does not compile: {}", d.message);
                        d
                    })
                    .collect(),
            )
        });
    caches.renderpearl.insert(k, r.clone());
    r
}

/// Translate and compile one variant. Errors (translation, glslang, `spirv-val`) fail the
/// variant; a Renderpearl compile failure only drops the Renderpearl GLSL (error
/// diagnostic) because the Vulkan modules are still usable.
pub fn compile_job(job: &VariantJob<'_>, jenv: &JobEnv<'_>) -> Result<CompiledVariant, Diagnostics> {
    let env = jenv.env;
    let want_vk = env.targets.contains(&OutputTarget::Vulkan);
    let want_rp = env.targets.contains(&OutputTarget::Renderpearl);
    let mut diags = Diagnostics::new();
    let mut out = CompiledVariant {
        stages: job
            .files
            .iter()
            .map(|(s, f)| CompiledStage { stage: *s, source_file: f.clone(), spirv: None, glsl_vulkan: None, glsl_renderpearl: None })
            .collect(),
        vertex_inputs: Vec::new(),
        fragment_outputs: Vec::new(),
        resources_used: Vec::new(),
        requires_raw_vulkan: false,
        push_constant_size: 0,
        local_size: None,
        descriptor_count: None,
        reflections: Vec::new(),
        validated_modules: 0,
        diagnostics: Diagnostics::new(),
    };
    let file_of = |stage: ShaderStage| job.files.iter().find(|(s, _)| *s == stage).map(|(_, f)| f.as_str()).unwrap_or("");
    let fill_meta = |out: &mut CompiledVariant, t: &sb_transform::TransformedProgram| {
        out.vertex_inputs = t.vertex_inputs.clone();
        out.fragment_outputs = t.fragment_outputs.clone();
        out.resources_used = t.resources_used.clone();
        out.requires_raw_vulkan |= t.requires_raw_vulkan;
    };

    let mut have_meta = false;
    if want_vk {
        let opts = transform_options(job, env, OutputTarget::Vulkan, jenv.image_formats);
        let t = sb_transform::transform_program(&job.stages, job.profile, &jenv.pack, &opts)?;
        diags.extend(t.diagnostics.iter().cloned());
        fill_meta(&mut out, &t);
        have_meta = true;
        let mut failed = false;
        for ts in &t.stages {
            let file = file_of(ts.stage);
            let Some(cs) = out.stages.iter_mut().find(|s| s.stage == ts.stage) else { continue };
            cs.glsl_vulkan = Some(ts.glsl.clone());
            match compile_vulkan(&ts.glsl, ts.stage, file, &ts.line_map, jenv.caches) {
                Ok(c) => {
                    let (spirv, refl, warnings) = &*c;
                    diags.extend(warnings.iter().cloned());
                    if jenv.validate {
                        match validate(spirv, jenv.caches) {
                            ValidationResult::Valid => out.validated_modules += 1,
                            ValidationResult::Invalid(m) => {
                                failed = true;
                                diags.push(
                                    Diagnostic::error("spv.validate", format!("spirv-val rejected the module: {}", m.lines().next().unwrap_or("").trim()))
                                        .in_stage(ts.stage),
                                );
                            }
                            ValidationResult::Skipped(why) => diags.push(
                                Diagnostic::info("spv.validate-skipped", format!("SPIR-V validation skipped: {why}")).in_stage(ts.stage),
                            ),
                        }
                    }
                    out.push_constant_size = out.push_constant_size.max(refl.push_constant_size);
                    if refl.local_size.is_some() {
                        out.local_size = refl.local_size;
                    }
                    cs.spirv = Some(spirv.clone());
                    out.reflections.push(refl.clone());
                }
                Err(e) => {
                    failed = true;
                    diags.extend(e.iter().cloned());
                }
            }
        }
        if failed {
            return Err(Diagnostics(tag(diags, &job.name, None)));
        }
        let mut descriptors = std::collections::BTreeSet::new();
        for r in &out.reflections {
            for d in &r.descriptors {
                descriptors.insert((d.set, d.binding));
            }
        }
        out.descriptor_count = Some(descriptors.len() as u32);
    }
    // Renderpearl output: skipped for programs only raw Vulkan can run (unless it is the
    // only target).
    if want_rp && !(want_vk && out.requires_raw_vulkan) {
        let opts = transform_options(job, env, OutputTarget::Renderpearl, jenv.image_formats);
        match sb_transform::transform_program(&job.stages, job.profile, &jenv.pack, &opts) {
            Ok(t) => {
                if !have_meta {
                    diags.extend(t.diagnostics.iter().cloned());
                    fill_meta(&mut out, &t);
                }
                let mut rp_failed = false;
                let mut texts = Vec::new();
                for ts in &t.stages {
                    match check_renderpearl(&ts.glsl, ts.stage, file_of(ts.stage), &ts.line_map, jenv.caches) {
                        Ok(()) => texts.push((ts.stage, ts.glsl.clone())),
                        Err(e) => {
                            rp_failed = true;
                            diags.extend(e.iter().cloned());
                        }
                    }
                }
                if !rp_failed {
                    for (stage, glsl) in texts {
                        if let Some(cs) = out.stages.iter_mut().find(|s| s.stage == stage) {
                            cs.glsl_renderpearl = Some(glsl);
                        }
                    }
                } else if !want_vk {
                    return Err(Diagnostics(tag(diags, &job.name, None)));
                }
            }
            Err(d) if !want_vk => return Err(d),
            Err(d) => diags.extend(d.into_iter().map(|mut x| {
                x.code = format!("rp.{}", x.code);
                x
            })),
        }
    }
    if matches!(job.kind, ProgramKind::Compute { .. } | ProgramKind::GeometryCompute { .. })
        && out.local_size.is_none()
    {
        out.local_size = job.stages.first().and_then(|s| s.info.local_size);
    }
    out.diagnostics = Diagnostics(tag(diags, &job.name, None));
    Ok(out)
}
