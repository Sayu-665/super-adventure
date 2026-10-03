//! Test harness: translate a small program and compile every emitted stage.

use sb_compile::{CompileOptions, Reflection, ValidationResult, VulkanTarget, compile_glsl, reflect, validate};
use sb_core::model::OutputTarget;
use sb_core::{Diagnostics, MemorySources, ShaderStage};
use sb_preprocess::{PreprocessOptions, Preprocessor};
use sb_transform::{AnalyzedStage, DrawProfile, PackBuilder, PackData, TransformOptions, TransformedProgram, analyze, profile, transform_program};
use sb_uniforms::{ProgramClass, ResourceContext};

/// File name used for a stage of the test program.
pub fn file_of(stage: ShaderStage, prefix: &str) -> String {
    let ext = match stage {
        ShaderStage::Vertex => "vsh",
        ShaderStage::TessControl => "tcs",
        ShaderStage::TessEval => "tes",
        ShaderStage::Geometry => "gsh",
        ShaderStage::Fragment => "fsh",
        ShaderStage::Compute => "csh",
    };
    format!("{prefix}.{ext}")
}

/// Preprocess and analyze one stage (`file` must exist in `src`).
pub fn analyze_file(src: &MemorySources, stage: ShaderStage, file: &str) -> Result<AnalyzedStage, Diagnostics> {
    let mut pp = Preprocessor::new(src);
    let pre = pp.preprocess(file, &PreprocessOptions::default());
    analyze(stage, &pre, file)
}

/// A test program.
pub struct T {
    stages: Vec<(ShaderStage, String)>,
    extra: Vec<(ShaderStage, String, ProgramClass)>,
    profile: DrawProfile,
    /// Translation options (`program_class` defaults from the profile).
    pub opts: TransformOptions,
    resources: Option<ResourceContext>,
    files: Vec<(String, String)>,
}

/// A translated and compiled program.
pub struct Out {
    /// The transformer output.
    pub prog: TransformedProgram,
    /// Reflection of every stage, in pipeline order.
    pub refl: Vec<Reflection>,
    /// The pack layout and binding table the program was translated against.
    pub pack: PackData,
}

impl Out {
    /// Emitted GLSL of `stage`.
    pub fn glsl(&self, stage: ShaderStage) -> &str {
        &self.prog.stages.iter().find(|s| s.stage == stage).unwrap_or_else(|| panic!("no {stage} stage")).glsl
    }

    /// Emitted vertex GLSL.
    pub fn vs(&self) -> &str {
        self.glsl(ShaderStage::Vertex)
    }

    /// Emitted fragment GLSL.
    pub fn fs(&self) -> &str {
        self.glsl(ShaderStage::Fragment)
    }

    /// Reflection of `stage`.
    pub fn refl(&self, stage: ShaderStage) -> &Reflection {
        self.refl.iter().find(|r| r.stage == stage).unwrap_or_else(|| panic!("no {stage} reflection"))
    }

    /// Whether a diagnostic with `code` was reported.
    pub fn has_diag(&self, code: &str) -> bool {
        self.prog.diagnostics.iter().any(|d| d.code == code)
    }
}

/// Assert that `haystack` contains every needle (prints the text otherwise).
#[track_caller]
pub fn contains_all(haystack: &str, needles: &[&str]) {
    for n in needles {
        assert!(haystack.contains(n), "missing `{n}` in:\n{haystack}");
    }
}

/// Assert that `haystack` contains none of the needles.
#[track_caller]
pub fn contains_none(haystack: &str, needles: &[&str]) {
    for n in needles {
        assert!(!haystack.contains(n), "unexpected `{n}` in:\n{haystack}");
    }
}

fn default_class(profile: &DrawProfile) -> ProgramClass {
    if profile.fullscreen {
        ProgramClass::Fullscreen
    } else if profile.name.starts_with("dh_") {
        ProgramClass::Dh
    } else {
        ProgramClass::Gbuffers
    }
}

impl T {
    /// A program drawn with draw profile `profile`.
    pub fn new(profile_name: &str) -> Self {
        let p = profile(profile_name).unwrap_or_else(|| panic!("no profile {profile_name}"));
        Self::custom(p.clone())
    }

    /// A program drawn with a custom (runtime-registered) draw profile.
    pub fn custom(profile: DrawProfile) -> Self {
        Self {
            stages: Vec::new(),
            extra: Vec::new(),
            opts: TransformOptions { program_class: default_class(&profile), ..TransformOptions::default() },
            profile,
            resources: None,
            files: Vec::new(),
        }
    }

    /// A gbuffers program (`vanilla_terrain` profile).
    pub fn gbuffers() -> Self {
        Self::new("vanilla_terrain")
    }

    /// A composite-style program (`fullscreen` profile).
    pub fn fullscreen() -> Self {
        Self::new(sb_transform::FULLSCREEN_PROFILE)
    }

    /// Add a stage.
    pub fn stage(mut self, stage: ShaderStage, src: &str) -> Self {
        self.stages.push((stage, src.to_string()));
        self
    }

    /// Add a vertex stage.
    pub fn vs(self, src: &str) -> Self {
        self.stage(ShaderStage::Vertex, src)
    }

    /// Add a fragment stage.
    pub fn fs(self, src: &str) -> Self {
        self.stage(ShaderStage::Fragment, src)
    }

    /// Add a geometry stage.
    pub fn gs(self, src: &str) -> Self {
        self.stage(ShaderStage::Geometry, src)
    }

    /// Add a stage of another program of the pack (it only contributes to the pack
    /// layout and binding table, before the program's own stages).
    pub fn other(mut self, stage: ShaderStage, src: &str, class: ProgramClass) -> Self {
        self.extra.push((stage, src.to_string(), class));
        self
    }

    /// Add an include file.
    pub fn file(mut self, path: &str, src: &str) -> Self {
        self.files.push((path.to_string(), src.to_string()));
        self
    }

    /// Change the options.
    pub fn with(mut self, f: impl FnOnce(&mut TransformOptions)) -> Self {
        f(&mut self.opts);
        self
    }

    /// Use a custom resource context (custom textures, images).
    pub fn resources(mut self, rc: ResourceContext) -> Self {
        self.resources = Some(rc);
        self
    }

    /// Translate (no compilation).
    pub fn translate(&self) -> Result<TransformedProgram, Diagnostics> {
        self.translate_with_pack().map(|(p, _)| p)
    }

    /// Translate (no compilation); also returns the pack data.
    pub fn translate_with_pack(&self) -> Result<(TransformedProgram, PackData), Diagnostics> {
        let mut src = MemorySources::new();
        for (p, s) in &self.files {
            src = src.with(p, s.as_str());
        }
        for (st, s) in &self.stages {
            src = src.with(&file_of(*st, "p"), s.as_str());
        }
        for (i, (st, s, _)) in self.extra.iter().enumerate() {
            src = src.with(&file_of(*st, &format!("other{i}")), s.as_str());
        }
        let class = self.opts.program_class;
        let mut stages = Vec::new();
        for (st, _) in &self.stages {
            stages.push(analyze_file(&src, *st, &file_of(*st, "p"))?);
        }
        let mut others = Vec::new();
        for (i, (st, _, c)) in self.extra.iter().enumerate() {
            others.push((analyze_file(&src, *st, &file_of(*st, &format!("other{i}")))?, *c));
        }
        let prof = &self.profile;
        let declared: Vec<String> = stages
            .iter()
            .chain(others.iter().map(|(s, _)| s))
            .flat_map(|s| s.info.opaque_uniforms.iter().map(|o| o.name.clone()))
            .collect();
        let rc = self
            .resources
            .clone()
            .unwrap_or_else(|| ResourceContext::new(class))
            .detect_watershadow(declared.iter().map(String::as_str));
        // Other programs come first: they win uniform type conflicts.
        let mut b = PackBuilder::new(rc.clone());
        for (s, c) in &others {
            b.add_stage(s, *c, Some("other"));
        }
        for s in &stages {
            b.add_stage(s, class, Some("p"));
        }
        if !stages.iter().any(|s| s.stage == ShaderStage::Compute) {
            b.add_profile(prof, class);
        }
        let data = b.finish();
        let ctx = data.context(&rc);
        let prog = transform_program(&stages, prof, &ctx, &self.opts)?;
        Ok((prog, data))
    }

    /// Translate, panicking on errors.
    #[track_caller]
    pub fn translate_ok(&self) -> TransformedProgram {
        self.translate().unwrap_or_else(|d| panic!("transform failed: {d:#?}"))
    }

    /// Translate and compile every stage (with `spirv-val` when installed).
    #[track_caller]
    pub fn run(&self) -> Out {
        let (prog, pack) = self.translate_with_pack().unwrap_or_else(|d| panic!("transform failed: {d:#?}"));
        let refl = compile_program(&prog, self.opts.target);
        Out { prog, refl, pack }
    }

    /// The translation error codes (panics if the translation succeeds).
    #[track_caller]
    pub fn errors(&self) -> Vec<String> {
        match self.translate() {
            Ok(p) => panic!("expected a transform error; got:\n{}", p.stages.iter().map(|s| s.glsl.as_str()).collect::<Vec<_>>().join("\n")),
            Err(d) => d.errors().map(|e| e.code.clone()).collect(),
        }
    }
}

/// Compile every stage of `prog` for `target`; panics with the GLSL on failure.
#[track_caller]
pub fn compile_program(prog: &TransformedProgram, target: OutputTarget) -> Vec<Reflection> {
    let copts = if target == OutputTarget::Renderpearl { CompileOptions::auto_mapped() } else { CompileOptions::default() };
    let mut out = Vec::new();
    for s in &prog.stages {
        let spirv = match compile_glsl(&s.glsl, s.stage, "test", &copts, Some(&s.line_map)) {
            Ok(v) => v,
            Err(e) => panic!("glslang rejected the {} stage: {e}\n{}", s.stage, numbered(&s.glsl)),
        };
        if let ValidationResult::Invalid(m) = validate(&spirv, VulkanTarget::Vulkan1_2) {
            panic!("spirv-val rejected the {} stage: {m}\n{}", s.stage, numbered(&s.glsl));
        }
        out.push(reflect(&spirv).unwrap_or_else(|e| panic!("reflect: {e}")));
    }
    out
}

fn numbered(s: &str) -> String {
    s.lines().enumerate().map(|(i, l)| format!("{:4} {l}\n", i + 1)).collect()
}

/// A plain vertex shader writing `gl_Position` and a `vec2 uv` varying.
pub const VS_UV: &str = "#version 120\nvarying vec2 uv;\nvoid main() { gl_Position = ftransform(); uv = gl_MultiTexCoord0.st; }\n";

/// A plain fragment shader reading `uv` and writing `gl_FragData[0]`.
pub const FS_UV: &str = "#version 120\nvarying vec2 uv;\nvoid main() { gl_FragData[0] = vec4(uv, 0.0, 1.0); }\n";
