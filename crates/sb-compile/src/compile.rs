//! GLSL -> SPIR-V compilation with glslang.

use crate::ffi;
use crate::log::{LogEntry, LogSeverity, parse_generator_messages, parse_glslang_log};
use crate::module;
use crate::target::{SpirvTarget, VulkanTarget};
use crate::tools;
use glslang_sys as sys;
use sb_core::{Diagnostic, Severity, ShaderStage, SourceLocation};
use serde::{Deserialize, Serialize};
use std::ffi::CString;
use std::fmt;

/// Diagnostic code of compile errors and warnings.
pub const DIAG_CODE: &str = "spv.compile";

/// Options for [`compile_glsl`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct CompileOptions {
    /// Vulkan version to apply GLSL rules for and to validate against (default 1.2).
    pub vulkan: VulkanTarget,
    /// SPIR-V version to emit (default 1.5). Must be supported by `vulkan`.
    pub spirv: SpirvTarget,
    /// Run `spirv-opt -O --preserve-bindings --preserve-interface` on the result.
    /// glslang-sys has no built-in optimiser, so this needs the `spirv-opt` binary;
    /// when it is missing (or fails, or runs longer than 60 s) the unoptimised
    /// module is returned with a warning. Default `false`.
    pub optimize: bool,
    /// Keep `OpName`/`OpMemberName` (and other debug instructions). Default `true`:
    /// Mojang's renderpearl matches descriptors and vertex inputs by name, and
    /// reflection reports names.
    pub keep_debug_names: bool,
    /// Assign `binding` to resources that have none (glslang auto-map, then
    /// `mapIO`). Off by default: the translator assigns everything explicitly.
    pub auto_map_bindings: bool,
    /// Assign `location` to stage inputs/outputs that have none. Off by default.
    pub auto_map_locations: bool,
    /// Emit `OpSource`/`OpLine` debug information (for RenderDoc & co). Default `false`.
    pub debug_info: bool,
    /// Suppress glslang warnings. Default `false`.
    pub suppress_warnings: bool,
    /// Make `min`, `max` and `clamp` on floats return the non-NaN operand, as NVIDIA
    /// and AMD GPUs do: GLSL.std.450 `FMin`/`FMax`/`FClamp` are rewritten to
    /// `NMin`/`NMax`/`NClamp` ([`crate::module::nan_tolerant_min_max`]). Shader packs
    /// are tuned on those GPUs; GLSL and `FMin` leave the result undefined when an
    /// operand is NaN, while `NMin` defines it as the other operand. The rewrite never
    /// changes a defined result. Default `true`; `docs/ARCHITECTURE.md` §8 has the
    /// measurements behind the default (a no-op on lavapipe, which already flushes).
    pub nan_tolerant_min_max: bool,
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            vulkan: VulkanTarget::default(),
            spirv: SpirvTarget::default(),
            optimize: false,
            keep_debug_names: true,
            auto_map_bindings: false,
            auto_map_locations: false,
            debug_info: false,
            suppress_warnings: false,
            nan_tolerant_min_max: true,
        }
    }
}

impl CompileOptions {
    /// Defaults plus glslang auto-mapping of bindings and locations, which is how
    /// Mojang's shaderc-based `GlslCompiler` treats renderpearl GLSL
    /// (`auto_bind_uniforms`). Use it to compile-check `Target::Renderpearl` output.
    pub fn auto_mapped() -> Self {
        Self { auto_map_bindings: true, auto_map_locations: true, ..Self::default() }
    }
}

/// One error or warning, located in the GLSL that was compiled and (through the
/// line map) in the original pack sources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompileMessage {
    /// 1-based line in the GLSL handed to [`compile_glsl`].
    pub line: Option<u32>,
    /// 1-based column in that line, when glslang reports one.
    pub column: Option<u32>,
    /// glslang's message, e.g. `'foo' : undeclared identifier`.
    pub message: String,
    /// The original pack location of `line`, from the line map.
    pub original: Option<SourceLocation>,
}

impl CompileMessage {
    fn from_entry(e: LogEntry, line_map: Option<&[Option<SourceLocation>]>) -> Self {
        let original = e.line.and_then(|l| map_line(line_map, l));
        Self { line: e.line, column: e.column, message: e.message, original }
    }

    fn plain(message: impl Into<String>) -> Self {
        Self { line: None, column: None, message: message.into(), original: None }
    }

    /// Convert to a [`Diagnostic`] with code [`DIAG_CODE`]. The location is the
    /// original pack location; when there is none, the translated-GLSL line is
    /// appended to the message instead.
    pub fn to_diagnostic(&self, severity: Severity, stage: ShaderStage) -> Diagnostic {
        let message = match (&self.original, self.line) {
            (None, Some(line)) => format!("{} (translated GLSL line {line})", self.message),
            _ => self.message.clone(),
        };
        Diagnostic::new(severity, DIAG_CODE, message).at_opt(self.original.clone()).in_stage(stage)
    }
}

impl fmt::Display for CompileMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(o) = &self.original {
            write!(f, "{o}: ")?;
        }
        match (self.line, self.column) {
            (Some(l), Some(c)) => write!(f, "[glsl {l}:{c}] ")?,
            (Some(l), None) => write!(f, "[glsl {l}] ")?,
            _ => {}
        }
        f.write_str(&self.message)
    }
}

/// Look up the original location of a 1-based GLSL line.
fn map_line(line_map: Option<&[Option<SourceLocation>]>, line: u32) -> Option<SourceLocation> {
    let idx = usize::try_from(line).ok()?.checked_sub(1)?;
    line_map?.get(idx)?.clone()
}

/// A failed compilation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompileFailure {
    /// The `name` passed to [`compile_glsl`].
    pub name: String,
    /// The stage that was compiled.
    pub stage: ShaderStage,
    /// The raw glslang info log.
    pub log: String,
    /// Parsed errors; never empty.
    pub errors: Vec<CompileMessage>,
    /// Parsed warnings (and notes).
    pub warnings: Vec<CompileMessage>,
}

impl CompileFailure {
    fn new(name: &str, stage: ShaderStage, log: String, entries: Vec<LogEntry>, line_map: Option<&[Option<SourceLocation>]>) -> Self {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        for e in entries {
            match e.severity {
                LogSeverity::Error => errors.push(CompileMessage::from_entry(e, line_map)),
                LogSeverity::Warning | LogSeverity::Note => warnings.push(CompileMessage::from_entry(e, line_map)),
            }
        }
        if errors.is_empty() {
            let text = log.trim();
            errors.push(CompileMessage::plain(if text.is_empty() { "glslang failed without a message" } else { text }));
        }
        Self { name: name.to_string(), stage, log, errors, warnings }
    }

    fn simple(name: &str, stage: ShaderStage, message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            name: name.to_string(),
            stage,
            log: message.clone(),
            errors: vec![CompileMessage::plain(message)],
            warnings: Vec::new(),
        }
    }

    /// All errors (severity error) and warnings (severity warning) as diagnostics
    /// with code [`DIAG_CODE`], located in the original sources when the line map
    /// knows the line. The stage is set; the caller adds the program.
    pub fn to_diagnostics(&self) -> Vec<Diagnostic> {
        self.errors
            .iter()
            .map(|m| m.to_diagnostic(Severity::Error, self.stage))
            .chain(self.warnings.iter().map(|m| m.to_diagnostic(Severity::Warning, self.stage)))
            .collect()
    }
}

impl fmt::Display for CompileFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to compile {} ({} stage)", self.name, self.stage)?;
        for e in &self.errors {
            write!(f, "\n  error: {e}")?;
        }
        Ok(())
    }
}

impl std::error::Error for CompileFailure {}

/// A successful compilation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileOutput {
    /// The SPIR-V module.
    pub spirv: Vec<u32>,
    /// glslang / SPIR-V generator / post-processing warnings.
    pub warnings: Vec<CompileMessage>,
    /// The raw glslang info log (usually empty).
    pub log: String,
}

impl CompileOutput {
    /// Warnings as diagnostics (code [`DIAG_CODE`], severity warning).
    pub fn warning_diagnostics(&self, stage: ShaderStage) -> Vec<Diagnostic> {
        self.warnings.iter().map(|m| m.to_diagnostic(Severity::Warning, stage)).collect()
    }
}

/// Native stacks tried for the thread glslang runs on, largest first, each with
/// the largest estimated expression depth (see `crate::guard`, after macro
/// expansion) it accepts.
///
/// glslang's tree passes use about 0.75 KiB of stack per expression-tree level
/// (comma operators, which the estimate counts double, about 1.2 KiB), so both
/// limits keep a safety margin of more than 3x (~19 MiB of 64 MiB, ~2.3 MiB of
/// 8 MiB). Stack memory is reserved, not committed, until used; the smaller
/// stack is a fallback for systems where the large reservation fails (strict
/// overcommit accounting, address-space limits). The deepest statement in the
/// reference corpus is estimated at 335.
const COMPILER_STACKS: [(usize, usize); 2] = [(64 << 20, 25_000), (8 << 20, 3_000)];

/// Limit used when no compiler thread can be started at all and glslang runs on
/// the caller's thread, whose stack may be as small as 512 KiB (JNI threads).
const INLINE_MAX_STATEMENT_OPERATORS: usize = 300;

/// Run glslang on a dedicated large-stack thread (see `crate::guard`), trying
/// the stacks of [`COMPILER_STACKS`] in turn, or inline with a strict complexity
/// limit if no thread can be created.
fn run_on_compiler_thread(job: ffi::Job<'_>) -> Result<ffi::JobOutput, ffi::JobError> {
    run_with_stacks(job, &COMPILER_STACKS)
}

fn run_with_stacks(job: ffi::Job<'_>, stacks: &[(usize, usize)]) -> Result<ffi::JobOutput, ffi::JobError> {
    for &(stack_bytes, max_statement_operators) in stacks {
        let job = ffi::Job { max_statement_operators, ..job };
        let outcome = std::thread::scope(|scope| {
            std::thread::Builder::new()
                .name("sb-compile".into())
                .stack_size(stack_bytes)
                .spawn_scoped(scope, || ffi::compile(&job))
                .map(|handle| {
                    handle.join().unwrap_or_else(|_| {
                        Err(ffi::JobError {
                            phase: ffi::Phase::Init,
                            log: "ERROR: glslang compiler thread panicked\n".into(),
                        })
                    })
                })
        });
        if let Ok(result) = outcome {
            return result;
        }
    }
    ffi::compile(&ffi::Job { max_statement_operators: INLINE_MAX_STATEMENT_OPERATORS, ..job })
}

/// Drop repeated identical entries (glslang reports some `#version` problems once
/// per preprocess and once per parse).
fn dedup_entries(entries: &mut Vec<LogEntry>) {
    let mut seen = std::collections::HashSet::new();
    entries.retain(|e| seen.insert((e.severity, e.line, e.column, e.message.clone())));
}

fn sys_stage(stage: ShaderStage) -> sys::glslang_stage_t {
    use sys::glslang_stage_t as S;
    match stage {
        ShaderStage::Vertex => S::Vertex,
        ShaderStage::TessControl => S::TesselationControl,
        ShaderStage::TessEval => S::TesselationEvaluation,
        ShaderStage::Geometry => S::Geometry,
        ShaderStage::Fragment => S::Fragment,
        ShaderStage::Compute => S::Compute,
    }
}

/// Compile Vulkan GLSL to SPIR-V.
///
/// * `source`: complete, preprocessed GLSL starting with `#version` (e.g. `#version 450`,
///   no `compatibility` profile). `#include` is refused.
/// * `stage`: the shader stage; the entry point is `main`.
/// * `name`: a label for messages (e.g. `world0/gbuffers_terrain.fsh`).
/// * `line_map`: optional original location of every GLSL line (`line_map[i]` is
///   line `i + 1`), used to fill [`CompileMessage::original`].
///
/// glslang applies Vulkan GLSL rules (`VULKAN_RULES | SPV_RULES`): no loose
/// uniforms, explicit locations for user inputs/outputs (unless
/// `auto_map_locations`), no compatibility profile, `#version` >= 140.
///
/// Inputs that would make glslang exhaust memory, time or its native stack are
/// rejected with an error that contains "to compile safely": macros expanding to
/// more than 4 million tokens or nesting invocations more than 64 deep, struct
/// types expanding to more than 100,000 members, and statements whose
/// expression trees could be deeper than the compiler thread's stack allows
/// (estimated depth 25,000).
///
/// Thread safety: may be called concurrently from any number of threads.
pub fn compile_glsl(
    source: &str,
    stage: ShaderStage,
    name: &str,
    opts: &CompileOptions,
    line_map: Option<&[Option<SourceLocation>]>,
) -> Result<Vec<u32>, CompileFailure> {
    compile_glsl_detailed(source, stage, name, opts, line_map).map(|o| o.spirv)
}

/// [`compile_glsl`], also returning the warnings of a successful compilation.
pub fn compile_glsl_detailed(
    source: &str,
    stage: ShaderStage,
    name: &str,
    opts: &CompileOptions,
    line_map: Option<&[Option<SourceLocation>]>,
) -> Result<CompileOutput, CompileFailure> {
    if !opts.vulkan.supports(opts.spirv) {
        return Err(CompileFailure::simple(name, stage, format!("{} cannot be consumed by {}", opts.spirv, opts.vulkan)));
    }
    let csource = match CString::new(source) {
        Ok(s) => s,
        Err(e) => {
            let line = source[..e.nul_position()].matches('\n').count() as u32 + 1;
            let msg = CompileMessage {
                line: Some(line),
                column: None,
                message: "source contains a NUL character".into(),
                original: map_line(line_map, line),
            };
            return Err(CompileFailure {
                name: name.to_string(),
                stage,
                log: format!("ERROR: 0:{line}: source contains a NUL character"),
                errors: vec![msg],
                warnings: Vec::new(),
            });
        }
    };
    let cname = CString::new(name.replace('\0', "")).ok();

    let mut messages = sys::glslang_messages_t::DEFAULT
        | sys::glslang_messages_t::SPV_RULES
        | sys::glslang_messages_t::VULKAN_RULES
        | sys::glslang_messages_t::DISPLAY_ERROR_COLUMN;
    if opts.debug_info {
        messages |= sys::glslang_messages_t::DEBUG_INFO;
    }
    if opts.suppress_warnings {
        messages |= sys::glslang_messages_t::SUPPRESS_WARNINGS;
    }
    let mut shader_options = sys::glslang_shader_options_t::DEFAULT;
    if opts.auto_map_bindings {
        shader_options |= sys::glslang_shader_options_t::AUTO_MAP_BINDINGS;
    }
    if opts.auto_map_locations {
        shader_options |= sys::glslang_shader_options_t::AUTO_MAP_LOCATIONS;
    }

    let job = ffi::Job {
        source: &csource,
        stage: sys_stage(stage),
        client_version: opts.vulkan.to_sys(),
        spirv_version: opts.spirv.to_sys(),
        messages,
        shader_options,
        debug_info: opts.debug_info,
        source_name: cname.as_deref(),
        map_io: opts.auto_map_bindings || opts.auto_map_locations,
        // Set per attempt by `run_with_stacks`.
        max_statement_operators: INLINE_MAX_STATEMENT_OPERATORS,
    };

    let out = match run_on_compiler_thread(job) {
        Ok(out) => out,
        Err(e) => {
            let mut entries = parse_glslang_log(&e.log);
            dedup_entries(&mut entries);
            let mut failure = CompileFailure::new(name, stage, e.log, entries, line_map);
            if failure.log.trim().is_empty() {
                failure.errors = vec![CompileMessage::plain(format!("glslang {} failed without a message", e.phase.describe()))];
            }
            return Err(failure);
        }
    };
    finish(name, stage, opts, line_map, out)
}

/// Turn glslang's output into the result: logged errors and generator errors
/// fail the compile; warnings are collected; optional post-processing runs.
fn finish(
    name: &str,
    stage: ShaderStage,
    opts: &CompileOptions,
    line_map: Option<&[Option<SourceLocation>]>,
    out: ffi::JobOutput,
) -> Result<CompileOutput, CompileFailure> {
    let mut entries = parse_glslang_log(&out.log);
    dedup_entries(&mut entries);
    let generator = parse_generator_messages(&out.generator_messages);
    // glslang logs some errors without failing (e.g. "#version: compilation for
    // SPIR-V does not support the compatibility profile"), and its SPIR-V
    // generator reports operations it could not translate ("Missing
    // functionality") while still emitting an (incomplete) module: treat both as
    // failures.
    if entries.iter().chain(&generator).any(|e| e.severity == LogSeverity::Error) {
        let log = match (out.log.trim().is_empty(), out.generator_messages.trim().is_empty()) {
            (_, true) => out.log,
            (true, false) => out.generator_messages,
            (false, false) => format!("{}\n{}", out.log.trim_end(), out.generator_messages),
        };
        entries.extend(generator);
        return Err(CompileFailure::new(name, stage, log, entries, line_map));
    }
    let generator_warnings = generator.into_iter().filter(|_| !opts.suppress_warnings);
    let mut warnings: Vec<CompileMessage> =
        entries.into_iter().chain(generator_warnings).map(|e| CompileMessage::from_entry(e, line_map)).collect();

    let mut spirv = out.spirv;
    if opts.nan_tolerant_min_max
        && let Err(reason) = module::nan_tolerant_min_max(&mut spirv)
    {
        warnings.push(CompileMessage::plain(format!("min/max/clamp left NaN-sensitive: {reason}")));
    }
    if opts.optimize {
        match tools::optimize(&spirv, opts.vulkan) {
            Ok(optimized) => spirv = optimized,
            Err(reason) => warnings.push(CompileMessage::plain(format!("module left unoptimized: {reason}"))),
        }
    }
    if !opts.keep_debug_names {
        match module::strip_debug_info(&spirv) {
            Ok(stripped) => spirv = stripped,
            Err(reason) => warnings.push(CompileMessage::plain(format!("debug names kept: {reason}"))),
        }
    }
    Ok(CompileOutput { spirv, warnings, log: out.log })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAG: &str = "#version 450\nlayout(location = 0) out vec4 color;\nvoid main() { color = vec4(1.0); }\n";

    #[test]
    fn compiles_minimal_fragment() {
        let spv = compile_glsl(FRAG, ShaderStage::Fragment, "t.fsh", &CompileOptions::default(), None).unwrap();
        assert_eq!(spv[0], module::MAGIC);
        assert_eq!(spv[1], SpirvTarget::Spirv1_5.header_word());
    }

    #[test]
    fn spirv_version_follows_options() {
        let opts = CompileOptions { vulkan: VulkanTarget::Vulkan1_0, spirv: SpirvTarget::Spirv1_0, ..Default::default() };
        let spv = compile_glsl(FRAG, ShaderStage::Fragment, "t.fsh", &opts, None).unwrap();
        assert_eq!(spv[1], SpirvTarget::Spirv1_0.header_word());
    }

    #[test]
    fn rejects_unsupported_target_combination() {
        let opts = CompileOptions { vulkan: VulkanTarget::Vulkan1_0, spirv: SpirvTarget::Spirv1_5, ..Default::default() };
        let err = compile_glsl(FRAG, ShaderStage::Fragment, "t.fsh", &opts, None).unwrap_err();
        assert!(err.errors[0].message.contains("cannot be consumed"), "{err}");
    }

    #[test]
    fn nul_byte_is_an_error_not_a_panic() {
        let src = "#version 450\nvoid main() {}\n\0junk";
        let err = compile_glsl(src, ShaderStage::Fragment, "t.fsh", &CompileOptions::default(), None).unwrap_err();
        assert_eq!(err.errors[0].line, Some(3));
    }

    #[test]
    fn line_map_lookup() {
        let map = vec![None, Some(SourceLocation::new("a.glsl", 7))];
        assert_eq!(map_line(Some(&map), 2), Some(SourceLocation::new("a.glsl", 7)));
        assert_eq!(map_line(Some(&map), 1), None);
        assert_eq!(map_line(Some(&map), 0), None);
        assert_eq!(map_line(Some(&map), 3), None);
        assert_eq!(map_line(None, 2), None);
    }

    #[test]
    fn diagnostics_mention_glsl_line_without_original() {
        let m = CompileMessage { line: Some(4), column: None, message: "boom".into(), original: None };
        let d = m.to_diagnostic(Severity::Error, ShaderStage::Vertex);
        assert_eq!(d.code, DIAG_CODE);
        assert_eq!(d.message, "boom (translated GLSL line 4)");
        assert_eq!(d.stage, Some(ShaderStage::Vertex));
        let m = CompileMessage { original: Some(SourceLocation::new("x.glsl", 2)), ..m };
        let d = m.to_diagnostic(Severity::Warning, ShaderStage::Vertex);
        assert_eq!(d.message, "boom");
        assert_eq!(d.location, Some(SourceLocation::new("x.glsl", 2)));
        assert_eq!(d.severity, Severity::Warning);
    }

    fn job_output(spirv: Vec<u32>, log: &str, generator_messages: &str) -> ffi::JobOutput {
        ffi::JobOutput { spirv, log: log.into(), generator_messages: generator_messages.into() }
    }

    #[test]
    fn generator_errors_fail_the_compile() {
        let spv = compile_glsl(FRAG, ShaderStage::Fragment, "t.fsh", &CompileOptions::default(), None).unwrap();
        let opts = CompileOptions::default();
        for messages in ["Missing functionality: matrix swizzle\n", "error: SPIRV-Tools Validation Errors\n"] {
            let out = job_output(spv.clone(), "", messages);
            let err = finish("t.fsh", ShaderStage::Fragment, &opts, None, out).unwrap_err();
            assert!(err.errors[0].message.starts_with("SPIR-V generator: "), "{err}");
            assert_eq!(err.log, messages);
        }
        // Generator warnings only warn (and are dropped with suppress_warnings).
        let out = job_output(spv.clone(), "", "TBD functionality: x\nwarning: y\n");
        let ok = finish("t.fsh", ShaderStage::Fragment, &opts, None, out).unwrap();
        assert_eq!(ok.warnings.len(), 2);
        let quiet = CompileOptions { suppress_warnings: true, ..Default::default() };
        let out = job_output(spv.clone(), "", "TBD functionality: x\n");
        assert!(finish("t.fsh", ShaderStage::Fragment, &quiet, None, out).unwrap().warnings.is_empty());
        // A logged error fails even when glslang produced a module; the logs are joined.
        let out = job_output(spv, "ERROR: 0:1: 'x' : y\n", "warning: z\n");
        let err = finish("t.fsh", ShaderStage::Fragment, &opts, None, out).unwrap_err();
        assert_eq!(err.errors.len(), 1);
        assert_eq!(err.warnings.len(), 1);
        assert!(err.log.contains("'x' : y") && err.log.contains("warning: z"), "{}", err.log);
    }

    #[test]
    fn falls_back_to_smaller_stacks_when_a_thread_cannot_be_created() {
        let source = std::ffi::CString::new(FRAG).unwrap();
        let job = ffi::Job {
            source: &source,
            stage: sys_stage(ShaderStage::Fragment),
            client_version: VulkanTarget::Vulkan1_2.to_sys(),
            spirv_version: SpirvTarget::Spirv1_5.to_sys(),
            messages: sys::glslang_messages_t::DEFAULT
                | sys::glslang_messages_t::SPV_RULES
                | sys::glslang_messages_t::VULKAN_RULES,
            shader_options: sys::glslang_shader_options_t::DEFAULT,
            debug_info: false,
            source_name: None,
            map_io: false,
            max_statement_operators: 0,
        };
        // A 1 PiB stack cannot be reserved: the next entry (then the inline
        // fallback) compiles the shader.
        assert!(run_with_stacks(job, &[(1 << 50, 25_000), (8 << 20, 3_000)]).is_ok());
        assert!(run_with_stacks(job, &[(1 << 50, 25_000)]).is_ok());
        // Each attempt applies its own complexity limit.
        let deep = std::ffi::CString::new(format!(
            "#version 450\nlayout(location = 0) out vec4 c;\nvoid main() {{ c = vec4(1.0){}; }}\n",
            " + c".repeat(400)
        ))
        .unwrap();
        let deep_job = ffi::Job { source: &deep, ..job };
        assert!(run_with_stacks(deep_job, &[(8 << 20, 3_000)]).is_ok());
        let err = run_with_stacks(deep_job, &[(1 << 50, 25_000)]).unwrap_err();
        assert!(err.log.contains("too complex"), "{}", err.log);
    }

    #[test]
    fn failure_always_has_an_error() {
        let f = CompileFailure::new("n", ShaderStage::Fragment, "".into(), vec![], None);
        assert_eq!(f.errors.len(), 1);
        let f = CompileFailure::new("n", ShaderStage::Fragment, "WARNING: 0:1: w\n".into(), parse_glslang_log("WARNING: 0:1: w\n"), None);
        assert_eq!(f.errors.len(), 1);
        assert_eq!(f.warnings.len(), 1);
    }
}
