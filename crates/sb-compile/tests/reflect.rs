//! Reflection of compiled modules: uniform blocks, samplers, images, SSBOs,
//! push constants, stage interfaces, built-ins and compute sizes.

mod common;

use common::compile_ok;
use pretty_assertions::assert_eq;
use sb_compile::{Access, DescriptorKind, ImageDim, reflect};
use sb_core::{GlslType, ScalarKind, ShaderStage};

#[test]
fn uniform_block_member_offsets_follow_std140() {
    let src = r#"#version 450
layout(set = 0, binding = 0, std140) uniform sb_Frame {
    float frameTimeCounter;   // 0
    vec3 cameraPosition;      // 16
    mat4 gbufferModelView;    // 32
    float weights[4];         // 96, stride 16
    vec2 viewSize;            // 160
    int frameCounter;         // 168
    mat3 normalMat;           // 176, 3 columns of 16
    ivec4 flags;              // 224
};
layout(location = 0) out vec4 c;
void main() {
    c = gbufferModelView * vec4(cameraPosition, frameTimeCounter) + weights[2] + vec4(viewSize, float(frameCounter), 0.0)
        + vec4(normalMat[0], 1.0) + vec4(flags);
}
"#;
    let refl = reflect(&compile_ok(src, ShaderStage::Fragment)).unwrap();
    assert_eq!(refl.descriptors.len(), 1);
    let d = &refl.descriptors[0];
    assert_eq!((d.set, d.binding, d.count), (0, 0, 1));
    // Anonymous block: the variable has no name, so the block type name is used.
    assert_eq!(d.name, "sb_Frame");
    assert_eq!(d.block_type_name.as_deref(), Some("sb_Frame"));
    let DescriptorKind::UniformBuffer { size, members } = &d.kind else { panic!("{:?}", d.kind) };
    let summary: Vec<(&str, u32, u32, &str)> =
        members.iter().map(|m| (m.name.as_str(), m.offset, m.size, m.type_name.as_str())).collect();
    assert_eq!(
        summary,
        vec![
            ("frameTimeCounter", 0, 4, "float"),
            ("cameraPosition", 16, 12, "vec3"),
            ("gbufferModelView", 32, 64, "mat4"),
            ("weights", 96, 64, "float[4]"),
            ("viewSize", 160, 8, "vec2"),
            ("frameCounter", 168, 4, "int"),
            ("normalMat", 176, 48, "mat3"),
            ("flags", 224, 16, "ivec4"),
        ]
    );
    assert_eq!(*size, 240);
    assert_eq!(members[3].array_stride, Some(16));
    assert_eq!(members[2].matrix_stride, Some(16));
    assert!(!members[2].row_major);
    assert_eq!(members[0].glsl_type, Some(GlslType::FLOAT));
    assert_eq!(members[1].glsl_type, Some(GlslType::VEC3));
    assert_eq!(members[2].glsl_type, Some(GlslType::MAT4));
    assert_eq!(members[3].glsl_type, Some(GlslType::FLOAT.with_array(4)));
    assert_eq!(members[6].glsl_type, Some(GlslType::MAT3));
    assert_eq!(members[7].glsl_type, Some(GlslType::IVEC4));
    assert_eq!(d.kind.vk_descriptor_type(), 6);
    assert!(refl.descriptor_by_name("sb_Frame").is_some());
}

#[test]
fn explicit_member_offsets_and_named_blocks() {
    // sb_Frame members are declared per program at their pack-global offsets.
    let src = r#"#version 450
layout(set = 0, binding = 0, std140) uniform sb_Frame {
    layout(offset = 48) vec3 sunPosition;
    layout(offset = 256) float rainStrength;
} frame;
layout(set = 0, binding = 1, std140) uniform sb_Draw { layout(offset = 0) int entityId; };
layout(location = 0) out vec4 c;
void main() { c = vec4(frame.sunPosition * frame.rainStrength, float(entityId)); }
"#;
    let refl = reflect(&compile_ok(src, ShaderStage::Fragment)).unwrap();
    let frame = refl.descriptor(0, 0).unwrap();
    assert_eq!(frame.name, "frame");
    assert_eq!(frame.block_type_name.as_deref(), Some("sb_Frame"));
    let members = frame.kind.members().unwrap();
    assert_eq!((members[0].name.as_str(), members[0].offset), ("sunPosition", 48));
    assert_eq!((members[1].name.as_str(), members[1].offset), ("rainStrength", 256));
    let DescriptorKind::UniformBuffer { size, .. } = frame.kind else { panic!() };
    assert_eq!(size, 260);
    let draw = refl.descriptor_by_name("sb_Draw").unwrap();
    assert_eq!((draw.set, draw.binding), (0, 1));
    // Lookup by block type name also works for named blocks.
    assert_eq!(refl.descriptor_by_name("sb_Frame").unwrap().binding, 0);
}

#[test]
fn sampler_kinds() {
    let src = r#"#version 450
layout(set = 1, binding = 0) uniform sampler2D colortex0;
layout(set = 1, binding = 1) uniform sampler2DShadow shadowtex0HW;
layout(set = 1, binding = 2) uniform sampler3D voxelTex;
layout(set = 1, binding = 3) uniform samplerCube skyCube;
layout(set = 1, binding = 4) uniform sampler2DArray layers;
layout(set = 1, binding = 5) uniform usampler2D materialTex;
layout(set = 1, binding = 6) uniform isampler3D ivox;
layout(set = 1, binding = 7) uniform sampler2DMS msTex;
layout(set = 1, binding = 8) uniform samplerBuffer texBuf;
layout(set = 1, binding = 9) uniform sampler1D lut;
layout(set = 1, binding = 10) uniform sampler2D noisetex[4];
layout(set = 1, binding = 11) uniform samplerCubeArrayShadow cubeShadows;
layout(location = 0) in vec2 uv;
layout(location = 0) out vec4 c;
void main() {
    c = texture(colortex0, uv) + vec4(texture(shadowtex0HW, vec3(uv, 0.5)))
      + texture(voxelTex, vec3(uv, 0.0)) + texture(skyCube, vec3(uv, 1.0))
      + texture(layers, vec3(uv, 2.0)) + vec4(texture(materialTex, uv)) + vec4(texture(ivox, vec3(uv, 0.0)))
      + texelFetch(msTex, ivec2(0), 0) + texelFetch(texBuf, 3) + texture(lut, uv.x)
      + texture(noisetex[1], uv) + vec4(texture(cubeShadows, vec4(uv, 0.0, 1.0), 0.5));
}
"#;
    let refl = reflect(&compile_ok(src, ShaderStage::Fragment)).unwrap();
    let kind = |name: &str| refl.descriptor_by_name(name).unwrap_or_else(|| panic!("{name}")).kind.clone();
    let cis = |dim, arrayed, shadow, multisampled, sample_type| DescriptorKind::CombinedImageSampler {
        dim,
        arrayed,
        shadow,
        multisampled,
        sample_type,
    };
    use ScalarKind::{Float, Int, Uint};
    assert_eq!(kind("colortex0"), cis(ImageDim::D2, false, false, false, Float));
    assert_eq!(kind("shadowtex0HW"), cis(ImageDim::D2, false, true, false, Float));
    assert_eq!(kind("voxelTex"), cis(ImageDim::D3, false, false, false, Float));
    assert_eq!(kind("skyCube"), cis(ImageDim::Cube, false, false, false, Float));
    assert_eq!(kind("layers"), cis(ImageDim::D2, true, false, false, Float));
    assert_eq!(kind("materialTex"), cis(ImageDim::D2, false, false, false, Uint));
    assert_eq!(kind("ivox"), cis(ImageDim::D3, false, false, false, Int));
    assert_eq!(kind("msTex"), cis(ImageDim::D2, false, false, true, Float));
    assert_eq!(kind("texBuf"), DescriptorKind::UniformTexelBuffer { sample_type: Float });
    assert_eq!(kind("lut"), cis(ImageDim::D1, false, false, false, Float));
    assert_eq!(kind("cubeShadows"), cis(ImageDim::Cube, true, true, false, Float));
    assert_eq!(refl.descriptor_by_name("noisetex").unwrap().count, 4);
    assert_eq!(refl.descriptor_by_name("colortex0").unwrap().count, 1);

    // sb-core dimension spelling.
    assert_eq!(kind("layers").core_dim().as_deref(), Some("2d_array"));
    assert_eq!(kind("cubeShadows").core_dim().as_deref(), Some("cube_array"));
    assert_eq!(kind("msTex").core_dim().as_deref(), Some("2d_ms"));
    assert_eq!(kind("texBuf").core_dim().as_deref(), Some("buffer"));
    assert_eq!(kind("texBuf").vk_descriptor_type(), 4);

    // Sorted by (set, binding).
    let bindings: Vec<u32> = refl.descriptors.iter().map(|d| d.binding).collect();
    assert_eq!(bindings, (0..12).collect::<Vec<_>>());
    assert!(refl.descriptors.iter().all(|d| d.set == 1));
    assert!(refl.has_capability("Sampled1D"));
    assert!(refl.has_capability("SampledBuffer"));
    assert!(refl.has_capability("SampledCubeArray"));
}

#[test]
fn separate_textures_and_samplers() {
    let src = r#"#version 450
layout(set = 0, binding = 0) uniform texture2D tex;
layout(set = 0, binding = 1) uniform sampler samp;
layout(location = 0) out vec4 c;
void main() { c = texture(sampler2D(tex, samp), vec2(0.5)); }
"#;
    let refl = reflect(&compile_ok(src, ShaderStage::Fragment)).unwrap();
    assert_eq!(
        refl.descriptor_by_name("tex").unwrap().kind,
        DescriptorKind::SampledImage { dim: ImageDim::D2, arrayed: false, multisampled: false, sample_type: ScalarKind::Float }
    );
    assert_eq!(refl.descriptor_by_name("samp").unwrap().kind, DescriptorKind::Sampler);
    assert_eq!(refl.descriptor_by_name("samp").unwrap().kind.vk_descriptor_type(), 0);
}

#[test]
fn storage_images_with_formats_and_access() {
    let src = r#"#version 450
layout(local_size_x = 8, local_size_y = 8) in;
layout(set = 2, binding = 0, rgba16f) uniform image2D colorimg0;
layout(set = 2, binding = 1, r32ui) uniform readonly uimage3D voxels;
layout(set = 2, binding = 2, rgba8) uniform writeonly image2DArray outArr;
layout(set = 2, binding = 3, r11f_g11f_b10f) uniform image3D lightVolume;
layout(set = 2, binding = 4, r32f) uniform imageBuffer histogram;
layout(set = 2, binding = 5) uniform writeonly image2D noFormat;
void main() {
    ivec2 p = ivec2(gl_GlobalInvocationID.xy);
    vec4 v = imageLoad(colorimg0, p) + vec4(imageLoad(voxels, ivec3(p, 0)));
    imageStore(colorimg0, p, v);
    imageStore(outArr, ivec3(p, 1), v);
    imageStore(lightVolume, ivec3(p, 2), imageLoad(lightVolume, ivec3(p, 2)) + v);
    imageStore(histogram, p.x, v);
    imageStore(noFormat, p, v);
}
"#;
    let refl = reflect(&compile_ok(src, ShaderStage::Compute)).unwrap();
    let kind = |name: &str| refl.descriptor_by_name(name).unwrap_or_else(|| panic!("{name}")).kind.clone();
    let img = |dim, arrayed, format: Option<&str>, access, sample_type| DescriptorKind::StorageImage {
        dim,
        arrayed,
        multisampled: false,
        format: format.map(str::to_string),
        access,
        sample_type,
    };
    use ScalarKind::{Float, Uint};
    assert_eq!(kind("colorimg0"), img(ImageDim::D2, false, Some("rgba16f"), Access::ReadWrite, Float));
    assert_eq!(kind("voxels"), img(ImageDim::D3, false, Some("r32ui"), Access::ReadOnly, Uint));
    assert_eq!(kind("outArr"), img(ImageDim::D2, true, Some("rgba8"), Access::WriteOnly, Float));
    assert_eq!(kind("lightVolume"), img(ImageDim::D3, false, Some("r11f_g11f_b10f"), Access::ReadWrite, Float));
    assert_eq!(kind("noFormat"), img(ImageDim::D2, false, None, Access::WriteOnly, Float));
    assert_eq!(
        kind("histogram"),
        DescriptorKind::StorageTexelBuffer { format: Some("r32f".into()), access: Access::ReadWrite, sample_type: Float }
    );
    assert_eq!(kind("colorimg0").vk_descriptor_type(), 3);
    assert_eq!(kind("histogram").vk_descriptor_type(), 5);
    assert!(refl.has_capability("StorageImageWriteWithoutFormat"));
    // The formats round-trip to sb-core texture formats.
    assert_eq!(
        sb_core::TextureFormat::from_glsl_image_format("rgba16f"),
        Some(sb_core::TextureFormat::RGBA16F)
    );
}

#[test]
fn storage_buffers() {
    let src = r#"#version 450
layout(local_size_x = 64) in;
struct Light { vec4 position; vec4 color; };
layout(set = 2, binding = 0, std430) buffer bufferObject0 { uint count; float values[]; } ssbo0;
layout(set = 2, binding = 1, std430) readonly buffer Lights { Light lights[16]; };
void main() {
    uint i = gl_GlobalInvocationID.x;
    ssbo0.values[i] = lights[i % 16u].color.r + float(ssbo0.count);
    atomicAdd(ssbo0.count, 1u);
}
"#;
    let refl = reflect(&compile_ok(src, ShaderStage::Compute)).unwrap();
    let b0 = refl.descriptor(2, 0).unwrap();
    assert_eq!(b0.name, "ssbo0");
    assert_eq!(b0.block_type_name.as_deref(), Some("bufferObject0"));
    let DescriptorKind::StorageBuffer { size, members, access } = &b0.kind else { panic!("{:?}", b0.kind) };
    assert_eq!(*access, Access::ReadWrite);
    assert_eq!(*size, 4, "runtime array counts as 0 bytes");
    assert_eq!(members[1].name, "values");
    assert_eq!(members[1].offset, 4);
    assert_eq!(members[1].type_name, "float[]");
    assert_eq!(members[1].array_stride, Some(4));
    assert_eq!(members[1].size, 0);
    assert_eq!(members[1].glsl_type, None);
    assert_eq!(b0.kind.vk_descriptor_type(), 7);

    let lights = refl.descriptor_by_name("Lights").unwrap();
    let DescriptorKind::StorageBuffer { size, members, access } = &lights.kind else { panic!() };
    assert_eq!(*access, Access::ReadOnly);
    assert_eq!(*size, 16 * 32);
    assert_eq!(members[0].type_name, "Light[16]");
    assert_eq!(members[0].array_stride, Some(32));
}

#[test]
fn push_constants() {
    let src = r#"#version 450
layout(push_constant) uniform PushConstants { mat4 model; vec3 tint; float alphaRef; } pc;
layout(location = 0) in vec3 pos;
void main() { gl_Position = pc.model * vec4(pos * pc.tint * pc.alphaRef, 1.0); }
"#;
    let refl = reflect(&compile_ok(src, ShaderStage::Vertex)).unwrap();
    assert_eq!(refl.push_constant_size, 80);
    let pc = refl.push_constants.as_ref().unwrap();
    assert_eq!(pc.name, "pc");
    assert_eq!(pc.block_type_name.as_deref(), Some("PushConstants"));
    let offsets: Vec<(&str, u32)> = pc.members.iter().map(|m| (m.name.as_str(), m.offset)).collect();
    assert_eq!(offsets, vec![("model", 0), ("tint", 64), ("alphaRef", 76)]);
    assert!(refl.descriptors.is_empty());

    let none = reflect(&compile_ok("#version 450\nvoid main() { gl_Position = vec4(0.0); }\n", ShaderStage::Vertex)).unwrap();
    assert_eq!(none.push_constant_size, 0);
    assert!(none.push_constants.is_none());
}

#[test]
fn vertex_inputs_and_outputs() {
    let src = r#"#version 450
layout(location = 0) in uvec3 vPosition;
layout(location = 1) in uint meta;
layout(location = 2) in vec4 vColor;
layout(location = 3) in ivec2 packedUv;
layout(location = 4) in mat4 instanceMatrix;
layout(location = 0) out vec4 color;
layout(location = 1) flat out uint blockId;
layout(location = 2) noperspective out vec2 screenUv;
layout(location = 3) centroid out vec3 normal;
layout(location = 4) out float fogs[3];
layout(location = 7) out mat3 tbn;
layout(location = 10, component = 2) out vec2 packedB;
layout(location = 10, component = 0) out vec2 packedA;
void main() {
    color = vColor;
    blockId = meta;
    screenUv = vec2(packedUv);
    normal = vec3(vPosition);
    fogs[0] = 1.0; fogs[1] = 2.0; fogs[2] = 3.0;
    tbn = mat3(instanceMatrix);
    packedA = vec2(1.0); packedB = vec2(2.0);
    gl_Position = instanceMatrix * vec4(vPosition, 1.0);
}
"#;
    let refl = reflect(&compile_ok(src, ShaderStage::Vertex)).unwrap();
    let ins: Vec<(u32, &str, &str, u32, u32, u32)> = refl
        .inputs
        .iter()
        .map(|v| (v.location, v.name.as_str(), v.base_type.as_str(), v.vec_size, v.columns, v.location_count))
        .collect();
    assert_eq!(
        ins,
        vec![
            (0, "vPosition", "uint", 3, 1, 1),
            (1, "meta", "uint", 1, 1, 1),
            (2, "vColor", "float", 4, 1, 1),
            (3, "packedUv", "int", 2, 1, 1),
            (4, "instanceMatrix", "float", 4, 4, 4),
        ]
    );
    assert_eq!(refl.input(4).unwrap().glsl_type_name(), "mat4");
    assert_eq!(refl.input_by_name("vPosition").unwrap().glsl_type_name(), "uvec3");

    let outs: Vec<(u32, u32, &str, &str)> =
        refl.outputs.iter().map(|v| (v.location, v.component, v.name.as_str(), v.base_type.as_str())).collect();
    assert_eq!(
        outs,
        vec![
            (0, 0, "color", "float"),
            (1, 0, "blockId", "uint"),
            (2, 0, "screenUv", "float"),
            (3, 0, "normal", "float"),
            (4, 0, "fogs", "float"),
            (7, 0, "tbn", "float"),
            (10, 0, "packedA", "float"),
            (10, 2, "packedB", "float"),
        ]
    );
    let out = |name: &str| refl.output_by_name(name).unwrap();
    assert!(out("blockId").flat);
    assert!(!out("color").flat);
    assert!(out("screenUv").noperspective);
    assert!(out("normal").centroid);
    assert_eq!(out("fogs").array_len, Some(3));
    assert_eq!(out("fogs").location_count, 3);
    assert_eq!(out("fogs").glsl_type_name(), "float[3]");
    assert_eq!(out("tbn").location_count, 3);
    assert_eq!(out("tbn").glsl_type_name(), "mat3");
    assert_eq!((out("packedA").component, out("packedB").component), (0, 2));
    assert!(!out("packedA").flat && !out("packedB").flat);
    assert!(refl.outputs.iter().all(|v| !v.per_vertex && !v.patch));

    assert_eq!(refl.builtin_outputs, vec!["ClipDistance", "CullDistance", "PointSize", "Position"]);
    assert!(refl.builtin_inputs.is_empty());
}

#[test]
fn fragment_interface_and_builtins() {
    let src = r#"#version 450
layout(location = 0) flat in int materialId;
layout(location = 1) in vec2 texcoord;
layout(location = 0) out vec4 sb_FragData_0;
layout(location = 1) out uvec4 sb_FragData_1;
layout(location = 2) out ivec2 sb_FragData_2;
void main() {
    sb_FragData_0 = vec4(texcoord, gl_FragCoord.z, 1.0);
    sb_FragData_1 = uvec4(materialId);
    sb_FragData_2 = ivec2(gl_FrontFacing ? 1 : 0);
    gl_FragDepth = 0.5;
}
"#;
    let refl = reflect(&compile_ok(src, ShaderStage::Fragment)).unwrap();
    assert!(refl.input_by_name("materialId").unwrap().flat);
    assert!(!refl.input_by_name("texcoord").unwrap().flat);
    let types: Vec<(u32, &str, u32)> = refl.outputs.iter().map(|v| (v.location, v.base_type.as_str(), v.vec_size)).collect();
    assert_eq!(types, vec![(0, "float", 4), (1, "uint", 4), (2, "int", 2)]);
    assert_eq!(refl.builtin_inputs, vec!["FragCoord", "FrontFacing"]);
    assert_eq!(refl.builtin_outputs, vec!["FragDepth"]);
    assert!(refl.has_execution_mode("OriginUpperLeft"));
    assert!(refl.has_execution_mode("DepthReplacing"));
    assert_eq!(refl.local_size, None);
}

#[test]
fn geometry_interface_is_per_vertex() {
    let src = r#"#version 450
layout(triangles) in;
layout(triangle_strip, max_vertices = 6) out;
layout(location = 0) in vec2 texcoord[];
layout(location = 1) flat in int ids[];
layout(location = 2) in float weights[][2];
layout(location = 0) out vec2 gTexcoord;
layout(location = 1) flat out int gId;
void main() {
    for (int i = 0; i < 3; i++) {
        gl_Position = gl_in[i].gl_Position;
        gTexcoord = texcoord[i] * weights[i][1];
        gId = ids[i];
        EmitVertex();
    }
    EndPrimitive();
}
"#;
    let refl = reflect(&compile_ok(src, ShaderStage::Geometry)).unwrap();
    let tc = refl.input_by_name("texcoord").unwrap();
    assert!(tc.per_vertex);
    assert_eq!(tc.array_len, None);
    assert_eq!(tc.vec_size, 2);
    assert!(refl.input_by_name("ids").unwrap().flat);
    let w = refl.input_by_name("weights").unwrap();
    assert_eq!(w.array_len, Some(2));
    assert_eq!(w.location_count, 2);
    assert!(refl.outputs.iter().all(|v| !v.per_vertex));
    assert!(refl.output_by_name("gId").unwrap().flat);
    assert!(refl.has_execution_mode("Triangles"));
    assert!(refl.has_execution_mode("OutputTriangleStrip"));
    let ov = refl.execution_modes.iter().find(|m| m.mode == "OutputVertices").unwrap();
    assert_eq!(ov.operands, vec![6]);
    assert!(refl.has_capability("Geometry"));
    assert!(refl.builtin_inputs.contains(&"Position".to_string()));
}

#[test]
fn tessellation_interfaces() {
    let tesc = r#"#version 450
layout(vertices = 4) out;
layout(location = 0) in vec3 pos[];
layout(location = 0) out vec3 tcPos[];
layout(location = 1) patch out vec4 patchData;
void main() {
    tcPos[gl_InvocationID] = pos[gl_InvocationID];
    patchData = vec4(1.0);
    gl_TessLevelOuter[0] = 1.0; gl_TessLevelOuter[1] = 1.0; gl_TessLevelOuter[2] = 1.0; gl_TessLevelOuter[3] = 1.0;
    gl_TessLevelInner[0] = 1.0; gl_TessLevelInner[1] = 1.0;
}
"#;
    let refl = reflect(&compile_ok(tesc, ShaderStage::TessControl)).unwrap();
    assert_eq!(refl.stage, ShaderStage::TessControl);
    assert!(refl.input_by_name("pos").unwrap().per_vertex);
    assert!(refl.output_by_name("tcPos").unwrap().per_vertex);
    let p = refl.output_by_name("patchData").unwrap();
    assert!(p.patch);
    assert!(!p.per_vertex);
    assert_eq!(p.vec_size, 4);
    assert_eq!(refl.execution_modes.iter().find(|m| m.mode == "OutputVertices").unwrap().operands, vec![4]);

    let tese = r#"#version 450
layout(quads, fractional_odd_spacing, cw) in;
layout(location = 0) in vec3 tcPos[];
layout(location = 1) patch in vec4 patchData;
void main() { gl_Position = vec4(tcPos[0] * gl_TessCoord.x, 1.0) + patchData; }
"#;
    let refl = reflect(&compile_ok(tese, ShaderStage::TessEval)).unwrap();
    assert_eq!(refl.stage, ShaderStage::TessEval);
    assert!(refl.input_by_name("tcPos").unwrap().per_vertex);
    let p = refl.input_by_name("patchData").unwrap();
    assert!(p.patch && !p.per_vertex);
    assert!(refl.has_execution_mode("Quads"));
    assert!(refl.has_execution_mode("SpacingFractionalOdd"));
    assert!(refl.has_execution_mode("VertexOrderCw"));
    assert!(refl.has_capability("Tessellation"));
}

#[test]
fn compute_local_size() {
    let src = "#version 450\nlayout(local_size_x = 16, local_size_y = 8, local_size_z = 2) in;\nvoid main() {}\n";
    let refl = reflect(&compile_ok(src, ShaderStage::Compute)).unwrap();
    assert_eq!(refl.stage, ShaderStage::Compute);
    assert_eq!(refl.local_size, Some([16, 8, 2]));

    let default = reflect(&compile_ok("#version 450\nvoid main() {}\n", ShaderStage::Compute)).unwrap();
    assert_eq!(default.local_size, Some([1, 1, 1]));

    // Specialization-constant sizes report their default values.
    let spec = "#version 450\nlayout(local_size_x_id = 0, local_size_y = 4) in;\nvoid main() {}\n";
    let refl = reflect(&compile_ok(spec, ShaderStage::Compute)).unwrap();
    assert_eq!(refl.local_size, Some([1, 4, 1]));
    // The default comes from `local_size_x`; the WorkgroupSize built-in constant wins.
    let spec = "#version 450\nlayout(local_size_x = 32, local_size_x_id = 0, local_size_y = 2) in;\nvoid main() {}\n";
    let refl = reflect(&compile_ok(spec, ShaderStage::Compute)).unwrap();
    assert_eq!(refl.local_size, Some([32, 2, 1]));

    // Uses gl_LocalInvocationID etc.
    let src = "#version 450\nlayout(local_size_x = 32) in;\nlayout(set = 0, binding = 0, std430) buffer B { uint v[]; };\n\
               void main() { v[gl_GlobalInvocationID.x] = gl_LocalInvocationIndex; }\n";
    let refl = reflect(&compile_ok(src, ShaderStage::Compute)).unwrap();
    assert_eq!(refl.local_size, Some([32, 1, 1]));
    assert!(refl.builtin_inputs.contains(&"GlobalInvocationId".to_string()));
    assert!(refl.builtin_inputs.contains(&"LocalInvocationIndex".to_string()));
}

#[test]
fn unused_declarations_are_reported() {
    // Pipeline layouts must cover every declared resource, used or not.
    let src = r#"#version 450
layout(set = 1, binding = 7) uniform sampler2D unusedTex;
layout(set = 0, binding = 0, std140) uniform U { float unusedValue; };
layout(location = 3) in vec2 unusedIn;
layout(location = 0) out vec4 c;
void main() { c = vec4(1.0); }
"#;
    let refl = reflect(&compile_ok(src, ShaderStage::Fragment)).unwrap();
    assert!(refl.descriptor(1, 7).is_some());
    assert!(refl.descriptor(0, 0).is_some());
    assert_eq!(refl.input(3).unwrap().name, "unusedIn");
}

#[test]
fn reflection_serializes() {
    let src = "#version 450\nlayout(set = 1, binding = 0) uniform sampler2D t;\nlayout(location = 0) out vec4 c;\nvoid main() { c = texture(t, vec2(0.0)); }\n";
    let refl = reflect(&compile_ok(src, ShaderStage::Fragment)).unwrap();
    let json = serde_json::to_string(&refl).unwrap();
    assert!(json.contains("\"type\":\"combined_image_sampler\""), "{json}");
    assert!(json.contains("\"dim\":\"2d\""), "{json}");
    let back: sb_compile::Reflection = serde_json::from_str(&json).unwrap();
    assert_eq!(back, refl);
}

#[test]
fn interface_blocks_and_runtime_descriptor_arrays() {
    let src = r#"#version 450
#extension GL_EXT_nonuniform_qualifier : require
layout(location = 0) out VertexData { vec2 uv; flat int id; mat2 rot; } vd;
layout(set = 1, binding = 0) uniform sampler2D textures[];
layout(location = 0) in vec2 pos;
void main() {
    vd.uv = pos + texture(textures[nonuniformEXT(gl_VertexIndex)], pos).xy;
    vd.id = gl_VertexIndex;
    vd.rot = mat2(1.0);
    gl_Position = vec4(pos, 0.0, 1.0);
}
"#;
    let refl = reflect(&compile_ok(src, ShaderStage::Vertex)).unwrap();
    // A block with a location is expanded member by member, at consecutive
    // locations, each with its own type and interpolation qualifiers.
    let outs: Vec<(u32, &str, String, bool, u32)> = refl
        .outputs
        .iter()
        .map(|v| (v.location, v.name.as_str(), v.glsl_type_name(), v.flat, v.location_count))
        .collect();
    assert_eq!(
        outs,
        vec![
            (0, "vd.uv", "vec2".to_string(), false, 1),
            (1, "vd.id", "int".to_string(), true, 1),
            (2, "vd.rot", "mat2".to_string(), false, 2),
        ]
    );
    let textures = refl.descriptor_by_name("textures").unwrap();
    assert_eq!(textures.count, 0, "runtime-sized descriptor array");
    assert!(refl.builtin_inputs.contains(&"VertexIndex".to_string()));
}

/// Regression: spirq drops interface blocks whose *members* carry the
/// locations; they must still be reported (and anonymous instances are named
/// after the block type).
#[test]
fn interface_blocks_with_member_locations() {
    let src = r#"#version 450
out Varyings { layout(location = 3) vec2 uv; layout(location = 5) flat uint id; layout(location = 7) vec4 color; };
layout(location = 8) out Extra { vec2 a; layout(location = 12) vec3 b; vec4 c; } extra;
layout(location = 0) out vec4 plain;
void main() {
    uv = vec2(0.0); id = 1u; color = vec4(1.0); plain = vec4(0.5);
    extra.a = vec2(1.0); extra.b = vec3(2.0); extra.c = vec4(3.0);
    gl_Position = vec4(0.0);
}
"#;
    let refl = reflect(&compile_ok(src, ShaderStage::Vertex)).unwrap();
    let outs: Vec<(u32, &str, &str, u32, bool)> =
        refl.outputs.iter().map(|v| (v.location, v.name.as_str(), v.base_type.as_str(), v.vec_size, v.flat)).collect();
    assert_eq!(
        outs,
        vec![
            (0, "plain", "float", 4, false),
            (3, "Varyings.uv", "float", 2, false),
            (5, "Varyings.id", "uint", 1, true),
            (7, "Varyings.color", "float", 4, false),
            (8, "extra.a", "float", 2, false),
            (12, "extra.b", "float", 3, false),
            // A member without a location follows the previous member.
            (13, "extra.c", "float", 4, false),
        ]
    );

    // Geometry inputs: a per-vertex block array is expanded too.
    let gs = r#"#version 450
layout(points) in;
layout(points, max_vertices = 1) out;
layout(location = 2) in VertexOut { vec3 normal; noperspective float depth; } vin[];
layout(location = 0) out vec3 n;
void main() { n = vin[0].normal * vin[0].depth; gl_Position = gl_in[0].gl_Position; EmitVertex(); }
"#;
    let refl = reflect(&compile_ok(gs, ShaderStage::Geometry)).unwrap();
    let normal = refl.input_by_name("vin.normal").unwrap();
    assert_eq!((normal.location, normal.vec_size, normal.per_vertex, normal.array_len), (2, 3, true, None));
    let depth = refl.input_by_name("vin.depth").unwrap();
    assert_eq!(depth.location, 3);
    assert!(depth.noperspective && depth.per_vertex);
    // gl_in[] is a built-in block, not a user input.
    assert_eq!(refl.inputs.len(), 2);
}

/// Struct-typed variables are expanded like blocks; arrays of structs stay whole.
#[test]
fn struct_interface_variables() {
    let src = r#"#version 450
struct Surface { vec3 albedo; float roughness; };
layout(location = 1) out Surface surf;
layout(location = 4) flat out Surface many[2];
void main() {
    surf.albedo = vec3(1.0); surf.roughness = 0.5;
    many[0] = surf; many[1] = surf;
    gl_Position = vec4(0.0);
}
"#;
    let refl = reflect(&compile_ok(src, ShaderStage::Vertex)).unwrap();
    let outs: Vec<(u32, &str, String, u32)> =
        refl.outputs.iter().map(|v| (v.location, v.name.as_str(), v.glsl_type_name(), v.location_count)).collect();
    assert_eq!(
        outs,
        vec![
            (1, "surf.albedo", "vec3".to_string(), 1),
            (2, "surf.roughness", "float".to_string(), 1),
            (4, "many", "struct[2]".to_string(), 4),
        ]
    );
    assert!(refl.output_by_name("many").unwrap().flat);
}

/// Dual-source blending outputs share a location and differ by `index`.
#[test]
fn fragment_output_index() {
    let src = r#"#version 450
layout(location = 0, index = 1) out vec4 blendFactor;
layout(location = 0, index = 0) out vec4 color;
void main() { color = vec4(1.0); blendFactor = vec4(0.5); }
"#;
    let refl = reflect(&compile_ok(src, ShaderStage::Fragment)).unwrap();
    let outs: Vec<(u32, u32, &str)> = refl.outputs.iter().map(|v| (v.location, v.index, v.name.as_str())).collect();
    assert_eq!(outs, vec![(0, 0, "color"), (0, 1, "blendFactor")]);
}

/// Integer storage images report their texel type (sb-core `ResourceKind::StorageImage::sample_type`).
#[test]
fn storage_image_texel_types() {
    let src = r#"#version 450
layout(local_size_x = 1) in;
layout(set = 2, binding = 0, r32i) uniform iimage2D counters;
layout(set = 2, binding = 1, r32ui) uniform uimageBuffer histogram;
layout(set = 2, binding = 2, rgba16f) uniform image3D volumes[2];
layout(set = 2, binding = 3, r32ui) uniform coherent uimage3D voxels;
void main() {
    imageAtomicAdd(counters, ivec2(0), 1);
    imageAtomicAdd(histogram, 0, 1u);
    imageStore(volumes[1], ivec3(0), vec4(1.0));
    imageAtomicMax(voxels, ivec3(1), 7u);
}
"#;
    let refl = reflect(&compile_ok(src, ShaderStage::Compute)).unwrap();
    let texel = |name: &str| match &refl.descriptor_by_name(name).unwrap().kind {
        DescriptorKind::StorageImage { sample_type, .. } | DescriptorKind::StorageTexelBuffer { sample_type, .. } => {
            *sample_type
        }
        other => panic!("{other:?}"),
    };
    assert_eq!(texel("counters"), ScalarKind::Int);
    assert_eq!(texel("histogram"), ScalarKind::Uint);
    assert_eq!(texel("volumes"), ScalarKind::Float);
    assert_eq!(refl.descriptor_by_name("volumes").unwrap().count, 2);
    assert_eq!(texel("voxels"), ScalarKind::Uint);
}

/// Regression: glslang accepts `uniform sampler2D t[2][3]` and emits an array
/// of arrays of resources, which Vulkan forbids and spirq silently skipped.
/// A missing descriptor would corrupt the pipeline layout, so reflection fails.
#[test]
fn arrays_of_arrays_of_resources_are_an_error() {
    let src = r#"#version 450
layout(set = 1, binding = 0) uniform sampler2D grid[2][3];
layout(location = 0) out vec4 c;
void main() { c = texture(grid[1][2], vec2(0.5)); }
"#;
    let spirv = sb_compile::compile_glsl(src, ShaderStage::Fragment, "t", &Default::default(), None).unwrap();
    let err = reflect(&spirv).unwrap_err();
    assert!(err.contains("`grid`") && err.contains("set 1, binding 0"), "{err}");
}

/// Several variables may alias one binding (e.g. two views of the same image);
/// each is reported, and the completeness check must not mistake them for
/// unreflectable descriptors.
#[test]
fn aliased_bindings_are_all_reported() {
    let src = r#"#version 450
layout(local_size_x = 1) in;
layout(set = 2, binding = 0, r32ui) uniform uimage2D asUint;
layout(set = 2, binding = 0, rgba8) uniform image2D asColor;
layout(set = 0, binding = 3, std430) buffer A { uint a[]; };
layout(set = 0, binding = 3, std430) buffer B { float b[]; };
void main() {
    imageAtomicAdd(asUint, ivec2(0), 1u);
    imageStore(asColor, ivec2(1), vec4(1.0));
    a[0] = 1u;
    b[1] = 2.0;
}
"#;
    let refl = reflect(&compile_ok(src, ShaderStage::Compute)).unwrap();
    let at = |set, binding| refl.descriptors.iter().filter(|d| d.set == set && d.binding == binding).count();
    assert_eq!(at(2, 0), 2, "{:#?}", refl.descriptors);
    assert_eq!(at(0, 3), 2, "{:#?}", refl.descriptors);
    let texel = |name: &str| match &refl.descriptor_by_name(name).unwrap().kind {
        DescriptorKind::StorageImage { sample_type, .. } => *sample_type,
        other => panic!("{other:?}"),
    };
    // The texel type is looked up per variable, not per binding.
    assert_eq!(texel("asUint"), ScalarKind::Uint);
    assert_eq!(texel("asColor"), ScalarKind::Float);
}
