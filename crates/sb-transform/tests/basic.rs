//! Smoke tests: small programs through the whole transformer and glslang.

use sb_core::model::{AlphaTest, DepthMode};
use sb_core::{MemorySources, ShaderStage};
use sb_preprocess::{PreprocessOptions, Preprocessor};
use sb_transform::{PackBuilder, TransformOptions, analyze, profile, transform_program};
use sb_uniforms::{ProgramClass, ResourceContext};

fn run(vs: &str, fs: &str, profile_name: &str, class: ProgramClass, opts: TransformOptions) -> sb_transform::TransformedProgram {
    let src = MemorySources::new().with("p.vsh", vs).with("p.fsh", fs);
    let mut pp = Preprocessor::new(&src);
    let o = PreprocessOptions::default();
    let v = analyze(ShaderStage::Vertex, &pp.preprocess("p.vsh", &o), "p.vsh").unwrap();
    let f = analyze(ShaderStage::Fragment, &pp.preprocess("p.fsh", &o), "p.fsh").unwrap();
    let prof = profile(profile_name).unwrap();
    let rc = ResourceContext::new(class);
    let mut b = PackBuilder::new(rc.clone());
    b.add_stage(&v, class, None);
    b.add_stage(&f, class, None);
    b.add_profile(prof, class);
    let data = b.finish();
    let ctx = data.context(&rc);
    let out = transform_program(&[v, f], prof, &ctx, &TransformOptions { program_class: class, ..opts }).unwrap_or_else(|d| panic!("{d:#?}"));
    for s in &out.stages {
        eprintln!("==== {:?}\n{}", s.stage, s.glsl);
        if let Err(e) = sb_compile::compile_glsl(&s.glsl, s.stage, "test", &sb_compile::CompileOptions::default(), Some(&s.line_map)) {
            panic!("{e}\n{}", s.glsl);
        }
    }
    out
}

#[test]
fn legacy_pair_with_every_profile() {
    let vs = "#version 120\nvarying vec4 texcoord;\nvarying vec4 color;\nvoid main() {\n  gl_Position = ftransform();\n  texcoord = gl_TextureMatrix[0] * gl_MultiTexCoord0;\n  color = gl_Color;\n}\n";
    let fs = "#version 120\nuniform sampler2D texture;\nvarying vec4 texcoord;\nvarying vec4 color;\nvoid main() {\n  gl_FragData[0] = texture2D(texture, texcoord.st) * color;\n}\n";
    for p in sb_transform::builtin_profiles() {
        let class = if p.fullscreen { ProgramClass::Fullscreen } else if p.name.starts_with("dh_") { ProgramClass::Dh } else { ProgramClass::Gbuffers };
        let opts = TransformOptions {
            alpha_test: Some(AlphaTest { func: sb_core::program::AlphaFunc::Greater, reference: 0.1 }),
            depth_mode: DepthMode::ForwardZeroToOne,
            ..TransformOptions::default()
        };
        eprintln!("######## profile {}", p.name);
        run(vs, fs, &p.name, class, opts);
    }
}
