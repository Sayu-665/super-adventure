//! Temporary probes (printed output) for the semantic review.

use sb_core::ShaderStage;
use sb_core::model::{AlphaTest, DepthMode};
use sb_core::program::AlphaFunc;

use crate::harness::*;

fn show(title: &str, out: &Out) {
    println!("===================== {title}");
    for s in &out.prog.stages {
        println!("----- {}\n{}", s.stage, s.glsl);
    }
    for d in out.prog.diagnostics.iter() {
        println!("diag: {} {}", d.code, d.message);
    }
}

#[test]
fn probe_alpha_core() {
    let vs = "#version 330 core\nin vec3 vaPosition;\nvoid main() { gl_Position = vec4(vaPosition, 1.0); }\n";
    let fs = "#version 330 core\nlayout(location = 0) out vec4 outColor0;\nvoid main() { outColor0 = vec4(0.0); }\n";
    let out = T::gbuffers().with(|o| o.alpha_test = Some(AlphaTest { func: AlphaFunc::Greater, reference: 0.1 })).vs(vs).fs(fs).run();
    show("alpha core", &out);
}

#[test]
fn probe_varying_pad() {
    let vs = "#version 120\nvarying vec3 v;\nvarying vec2 w;\nvoid main() { gl_Position = ftransform(); v = vec3(1.0); w = vec2(2.0); }\n";
    let fs = "#version 120\nvarying vec4 v;\nvarying vec4 w;\nvoid main() { gl_FragData[0] = v + w; }\n";
    let out = T::gbuffers().vs(vs).fs(fs).run();
    show("varying pad", &out);
}

#[test]
fn probe_gather_compare_reversed() {
    let fs = "#version 400\nuniform sampler2DShadow shadowtex0;\nin vec2 uv;\nout vec4 c;\nvoid main() { c = textureGather(shadowtex0, uv, 0.5) + textureGatherOffset(shadowtex0, uv, 0.5, ivec2(1)); }\n";
    let vs = "#version 400\nout vec2 uv;\nvoid main() { gl_Position = vec4(0.0); uv = vec2(0.0); }\n";
    let out = T::gbuffers()
        .with(|o| {
            o.depth_mode = DepthMode::ReversedZeroToOne;
            o.invert_depth_reads = true;
        })
        .vs(vs)
        .fs(fs)
        .run();
    show("gather compare reversed", &out);
}

#[test]
fn probe_gs_double_emit() {
    let gs = "#version 150\nlayout(triangles) in;\nlayout(triangle_strip, max_vertices = 6) out;\nvoid main() {\n  gl_Position = gl_in[0].gl_Position; EmitVertex();\n  gl_Position.x += 0.1; EmitVertex();\n  EndPrimitive();\n}\n";
    let vs = "#version 150\nin vec3 vaPosition;\nvoid main() { gl_Position = vec4(vaPosition, 1.0); }\n";
    let fs = "#version 150\nout vec4 outColor0;\nvoid main() { outColor0 = vec4(1.0); }\n";
    let out = T::gbuffers().vs(vs).gs(gs).fs(fs).run();
    show("gs double emit", &out);
}

#[test]
fn probe_reserved_members() {
    let fs = "#version 120\nstruct S { float filter; vec3 input; };\nuniform sampler2D colortex0;\nvarying vec2 uv;\nfloat sample(S s) { return s.filter + s.input.x; }\nvoid main() { S s; s.filter = 1.0; s.input = vec3(2.0); float output = sample(s); gl_FragData[0] = vec4(output); }\n";
    let out = T::fullscreen().vs(VS_UV).fs(fs).run();
    show("reserved members", &out);
}

#[test]
fn probe_builtins_vs() {
    let vs = "#version 120\nvarying vec4 lm;\nvarying vec4 tc;\nvarying vec3 n;\nvoid main() { gl_Position = ftransform(); float z = ftransform().z; lm = gl_TextureMatrix[1] * gl_MultiTexCoord1; tc = gl_TextureMatrix[0] * gl_MultiTexCoord0; n = gl_NormalMatrix * gl_Normal; gl_Position.z += z * 0.0; }\n";
    let fs = "#version 120\nvarying vec4 lm;\nvarying vec4 tc;\nvarying vec3 n;\nvoid main() { gl_FragData[0] = lm + tc + vec4(n, 1.0); }\n";
    for p in ["vanilla_terrain", "vanilla_entity", "vanilla_particle", "dh_terrain", "fullscreen"] {
        let out = T::new(p).vs(vs).fs(fs).run();
        show(p, &out);
    }
}

#[test]
fn probe_depth_gather() {
    let fs = "#version 400\nuniform sampler2D depthtex0;\nin vec2 uv;\nout vec4 c;\nvoid main() { c = vec4(textureGather(depthtex0, uv).r, texelFetch(depthtex0, ivec2(0), 0).r, textureLod(depthtex0, uv, 0.0).rg); }\n";
    let vs = "#version 400\nout vec2 uv;\nvoid main() { gl_Position = vec4(0.0); uv = vec2(0.0); }\n";
    let out = T::fullscreen()
        .with(|o| {
            o.depth_mode = DepthMode::ReversedZeroToOne;
            o.invert_depth_reads = true;
        })
        .vs(vs)
        .fs(fs)
        .run();
    show("depth gather", &out);
    let _ = ShaderStage::Vertex;
}

#[test]
fn probe_uniform_offsets() {
    let other = "#version 130\nuniform float a;\nuniform vec3 b;\nuniform int c;\nuniform mat3 m3;\nuniform float arr[3];\nuniform bool flag;\nuniform vec2 v2;\nuniform mat2 m2;\nvoid main() { gl_FragData[0] = vec4(a + b.x + float(c) + m3[0][0] + arr[1] + float(flag) + v2.x + m2[1][1]); }\n";
    let fs = "#version 130\nuniform vec2 v2;\nuniform float arr[3];\nuniform mat3 m3;\nuniform vec3 b;\nuniform ivec3 iv;\nuniform mat4 m4;\nuniform uint u;\nuniform mat2 m2;\nvarying vec2 uv;\nvoid main() { gl_FragData[0] = vec4(v2.x + arr[2] + m3[2][2] + b.z + float(iv.y) + m4[3][3] + float(u) + m2[1][0]); }\n";
    let out = T::fullscreen().other(ShaderStage::Fragment, other, sb_uniforms::ProgramClass::Fullscreen).vs(VS_UV).fs(fs).run();
    show("uniform offsets", &out);
    for d in &out.refl(ShaderStage::Fragment).descriptors {
        if let Some(ms) = d.kind.members() {
            for m in ms {
                let l = out.pack.layout.frame.member(&m.name);
                println!("refl {} off {} size {} astride {:?} mstride {:?}  layout {:?}", m.name, m.offset, m.size, m.array_stride, m.matrix_stride, l.map(|l| (l.offset, l.ty)));
            }
        }
    }
}

#[test]
fn probe_tess_epilogue() {
    let vs = "#version 400\nin vec3 vaPosition;\nout vec3 p;\nvoid main() { p = vaPosition; }\n";
    let tcs = "#version 400\nlayout(vertices = 3) out;\nin vec3 p[];\nout vec3 q[];\nvoid main() { q[gl_InvocationID] = p[gl_InvocationID]; gl_TessLevelOuter[0] = 1.0; gl_TessLevelOuter[1] = 1.0; gl_TessLevelOuter[2] = 1.0; gl_TessLevelInner[0] = 1.0; }\n";
    let tes = "#version 400\nlayout(triangles) in;\nin vec3 q[];\nout vec3 r;\nvoid main() { r = q[0] * gl_TessCoord.x + q[1] * gl_TessCoord.y + q[2] * gl_TessCoord.z; gl_Position = vec4(r, 1.0); }\n";
    let fs = "#version 400\nin vec3 r;\nout vec4 c;\nvoid main() { c = vec4(r, 1.0); }\n";
    let out = T::gbuffers().vs(vs).stage(ShaderStage::TessControl, tcs).stage(ShaderStage::TessEval, tes).fs(fs).run();
    show("tess", &out);
}

#[test]
fn probe_int_literals() {
    let fs = "#version 130\nvarying vec2 uv;\nvoid main() { uint a = 0xFFFFFFFF; int b = 0x80000000; uint c = 4294967295; int d = -2147483648; uint e = 0x80000000u; gl_FragData[0] = vec4(float(a), float(b), float(c), float(d) + float(e)); }\n";
    let out = T::fullscreen().vs(VS_UV).fs(fs).run();
    show("int literals", &out);
}

#[test]
fn probe_host_samplers() {
    let vs = "#version 120\nvarying vec2 uv;\nvarying vec2 lm;\nvoid main() { gl_Position = ftransform(); uv = gl_MultiTexCoord0.st; lm = (gl_TextureMatrix[1] * gl_MultiTexCoord1).st; }\n";
    let fs = "#version 120\nuniform sampler2D texture;\nuniform sampler2D lightmap;\nuniform sampler2D gtexture;\nvarying vec2 uv;\nvarying vec2 lm;\nvoid main() { gl_FragData[0] = texture2D(texture, uv) * texture2D(lightmap, lm) * texture2D(gtexture, uv); }\n";
    let out = T::gbuffers().with(|o| o.target = sb_core::model::OutputTarget::Renderpearl).vs(vs).fs(fs).run();
    show("host samplers rp", &out);
    let out = T::gbuffers().vs(vs).fs(fs).run();
    show("host samplers vk", &out);
}

#[test]
fn probe_gs_ff() {
    let vs = "#version 120\nvoid main() { gl_Position = ftransform(); gl_FrontColor = gl_Color; gl_TexCoord[0] = gl_MultiTexCoord0; }\n";
    let gs = "#version 150 compatibility\nlayout(triangles) in;\nlayout(triangle_strip, max_vertices = 3) out;\nflat out int id;\nout mat3 tbn;\nvoid main() { for (int i = 0; i < 3; i++) { gl_Position = gl_in[i].gl_Position; gl_FrontColor = gl_FrontColorIn[i]; gl_TexCoord[0] = gl_TexCoordIn[i][0]; id = i; tbn = mat3(1.0); EmitVertex(); } EndPrimitive(); }\n";
    let fs = "#version 120\nflat in int id;\nin mat3 tbn;\nvoid main() { gl_FragData[0] = gl_Color * gl_TexCoord[0] * float(id) * tbn[1].x; }\n";
    let out = T::gbuffers().vs(vs).gs(gs).fs(fs).run();
    show("gs ff", &out);
}

#[test]
fn probe_builtin_overload() {
    let fs = "#version 130\nvarying vec2 uv;\nvec3 pow(vec3 x, float y) { return pow(x, vec3(y)); }\nvoid main() { float a = pow(2.0, 3.0); vec3 b = pow(vec3(uv, 1.0), 2.0); gl_FragData[0] = vec4(b, a); }\n";
    match T::fullscreen().vs(VS_UV).fs(fs).translate() {
        Ok(p) => {
            for s in &p.stages {
                println!("{}", s.glsl);
                let r = sb_compile::compile_glsl(&s.glsl, s.stage, "x", &sb_compile::CompileOptions::default(), None);
                println!("compile: {:?}", r.err().map(|e| e.to_string()));
            }
        }
        Err(e) => println!("ERR {e:?}"),
    }
}

#[test]
fn probe_gs_profile_global_referenced() {
    let vs = "#version 150\nin vec3 vaPosition;\nvoid main() { gl_Position = vec4(vaPosition, 1.0); }\n";
    let gs = "#version 150\nlayout(triangles) in;\nlayout(triangle_strip, max_vertices = 3) out;\nuniform vec4 entityColor;\nout vec4 tint;\n\
              void main() { for (int i = 0; i < 3; i++) { gl_Position = gl_in[i].gl_Position; tint = entityColor; EmitVertex(); } }\n";
    let fs = "#version 150\nuniform vec4 entityColor;\nin vec4 tint;\nout vec4 c;\nvoid main() { c = entityColor * tint; }\n";
    match T::new("vanilla_entity").vs(vs).gs(gs).fs(fs).translate() {
        Ok(p) => {
            for s in &p.stages {
                println!("----- {}\n{}", s.stage, s.glsl);
                let r = sb_compile::compile_glsl(&s.glsl, s.stage, "x", &sb_compile::CompileOptions::default(), None);
                println!("compile: {:?}", r.err().map(|e| e.to_string()));
            }
        }
        Err(e) => println!("ERR {e:?}"),
    }
}

#[test]
fn probe_gs_pass() {
    let vs = "#version 150\nin vec3 vaPosition;\nvoid main() { gl_Position = vec4(vaPosition, 1.0); }\n";
    let gs = "#version 150\nlayout(triangles) in;\nlayout(triangle_strip, max_vertices = 3) out;\n\
              void main() { for (int i = 0; i < 3; i++) { gl_Position = gl_in[i].gl_Position; EmitVertex(); } }\n";
    let fs = "#version 150\nuniform vec4 entityColor;\nout vec4 c;\nvoid main() { c = entityColor; }\n";
    let out = T::new("vanilla_entity").vs(vs).gs(gs).fs(fs).run();
    show("gs pass", &out);
}
