//! Cross-check of the std140 layouts against glslang: the blocks are emitted as Vulkan
//! GLSL with explicit `layout(offset = N)` members (as the transformer does), compiled
//! with `glslangValidator`, validated with `spirv-val`, and every member offset in the
//! SPIR-V must equal the computed one. Skipped when the tools are not installed.
//!
//! glslang rejects explicit offsets that are not a multiple of the member's alignment,
//! and `spirv-val` rejects members that overlap the previous member (including an
//! array's or matrix's trailing padding). Together they check the packer.

use sb_core::GlslType;
use sb_core::model::{BlockLayout, UniformLayout, UniformSource};
use sb_uniforms::{LayoutBuilder, UniformDecl, registry};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

fn tool_available(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn declare(block: &BlockLayout) -> String {
    let mut s = format!(
        "layout(std140, set = {}, binding = {}) uniform {} {{\n",
        block.set, block.binding, block.name
    );
    for m in &block.members {
        let array = m.ty.array.map(|n| format!("[{n}]")).unwrap_or_default();
        s += &format!(
            "    layout(offset = {}) {} {}{};\n",
            m.offset,
            m.ty.glsl_name(),
            m.name,
            array
        );
    }
    s + "};\n"
}

/// Compile `layout` and return block name → (member name → offset) from the SPIR-V.
fn compile(layout: &UniformLayout, dir: &Path, tag: &str) -> HashMap<String, HashMap<String, u32>> {
    let first = |b: &BlockLayout| {
        b.members.first().map_or("0.0".to_string(), |m| {
            let idx = if m.ty.array.is_some() { "[0]" } else { "" };
            let comp = if m.ty.is_matrix() {
                "[0][0]"
            } else if m.ty.rows > 1 {
                ".x"
            } else {
                ""
            };
            format!("float({}{idx}{comp})", m.name)
        })
    };
    let src = format!(
        "#version 450\n{}{}layout(location = 0) out vec4 o;\nvoid main() {{ o = vec4({} + {}); }}\n",
        declare(&layout.frame),
        declare(&layout.draw),
        first(&layout.frame),
        first(&layout.draw)
    );
    let glsl = dir.join(format!("{tag}.frag"));
    let spv = dir.join(format!("{tag}.spv"));
    std::fs::write(&glsl, &src).unwrap();
    let out = Command::new("glslangValidator")
        .args(["-V", "--target-env", "vulkan1.2", "-o"])
        .arg(&spv)
        .arg(&glsl)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "glslang rejected the layout:\n{}\n{src}",
        String::from_utf8_lossy(&out.stdout)
    );
    let val = Command::new("spirv-val")
        .args(["--target-env", "vulkan1.2"])
        .arg(&spv)
        .output()
        .unwrap();
    assert!(
        val.status.success(),
        "spirv-val: {}",
        String::from_utf8_lossy(&val.stderr)
    );
    let dis = Command::new("spirv-dis").arg(&spv).output().unwrap();
    let text = String::from_utf8_lossy(&dis.stdout);

    // %id -> block name, (%id, index) -> member name, (%id, index) -> offset
    let mut type_names: HashMap<String, String> = HashMap::new();
    let mut member_names: HashMap<(String, u32), String> = HashMap::new();
    let mut offsets: Vec<(String, u32, u32)> = Vec::new();
    for line in text.lines() {
        let t: Vec<&str> = line.split_whitespace().collect();
        match t.as_slice() {
            ["OpName", id, name] => {
                type_names.insert(id.to_string(), name.trim_matches('"').to_string());
            }
            ["OpMemberName", id, idx, name] => {
                member_names.insert(
                    (id.to_string(), idx.parse().unwrap()),
                    name.trim_matches('"').to_string(),
                );
            }
            ["OpMemberDecorate", id, idx, "Offset", off] => {
                offsets.push((id.to_string(), idx.parse().unwrap(), off.parse().unwrap()));
            }
            _ => {}
        }
    }
    let mut result: HashMap<String, HashMap<String, u32>> = HashMap::new();
    for (id, idx, off) in offsets {
        let block = type_names.get(&id).cloned().unwrap_or_default();
        let member = member_names.get(&(id, idx)).cloned().unwrap_or_default();
        result.entry(block).or_default().insert(member, off);
    }
    result
}

fn check(layout: &UniformLayout, tag: &str) {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let spirv = compile(layout, dir, tag);
    for block in [&layout.frame, &layout.draw] {
        if block.members.is_empty() {
            continue;
        }
        let got = spirv
            .get(&block.name)
            .unwrap_or_else(|| panic!("{} missing from SPIR-V", block.name));
        assert_eq!(got.len(), block.members.len(), "{}", block.name);
        for m in &block.members {
            assert_eq!(
                got.get(&m.name),
                Some(&m.offset),
                "{}.{} ({})",
                block.name,
                m.name,
                m.ty
            );
        }
    }
}

#[test]
fn every_builtin_laid_out_by_glslang_rules() {
    if !tool_available("glslangValidator")
        || !tool_available("spirv-dis")
        || !tool_available("spirv-val")
    {
        eprintln!("glslangValidator/spirv-dis/spirv-val not found; skipping");
        return;
    }
    let mut b = LayoutBuilder::new();
    for builtin in registry::all() {
        b.add(UniformDecl::from_registry(builtin.name, builtin.ty));
    }
    let (layout, _, diags) = b.build();
    assert!(diags.is_empty(), "{diags:?}");
    check(&layout, "builtins");
}

#[test]
fn tricky_types_laid_out_by_glslang_rules() {
    if !tool_available("glslangValidator")
        || !tool_available("spirv-dis")
        || !tool_available("spirv-val")
    {
        eprintln!("glslangValidator/spirv-dis/spirv-val not found; skipping");
        return;
    }
    let t = |s: &str| GlslType::parse(s).unwrap();
    let unset = |n: &str, ty: GlslType| UniformDecl::new(n, ty, UniformSource::Unset);
    let decls = vec![
        unset("a_vec3", t("vec3")),
        unset("b_float", t("float")),
        unset("c_vec2", t("vec2")),
        unset("d_mat3", t("mat3")),
        unset("e_floats", t("float").with_array(5)),
        unset("f_vec3s", t("vec3").with_array(3)),
        unset("g_bool", t("bool")),
        unset("h_ivec3", t("ivec3")),
        unset("i_uint", t("uint")),
        unset("j_mat2", t("mat2")),
        unset("k_mat2x3", t("mat2x3")),
        unset("l_mat4x2", t("mat4x2")),
        unset("m_mat3x4", t("mat3x4")),
        unset("n_bvec2", t("bvec2")),
        unset("o_vec2s", t("vec2").with_array(2)),
        unset("p_mat3s", t("mat3").with_array(2)),
        unset("q_double", t("double")),
        unset("r_dvec3", t("dvec3")),
        unset("s_dvec2", t("dvec2")),
        unset("t_dmat3", t("dmat3")),
        unset("u_int", t("int")),
        unset("v_uvec4", t("uvec4")),
        // A type conflict: the second declaration gets `sb_as_float_worldTime`.
        UniformDecl::from_registry("worldTime", t("int")),
        UniformDecl::from_registry("worldTime", t("float")),
        UniformDecl::from_registry("entityColor", t("vec4")),
        UniformDecl::from_registry("normalMatrix", t("mat3")),
        UniformDecl::from_registry("alphaTestRef", t("float")),
    ];
    let mut b = LayoutBuilder::new();
    for d in decls {
        b.add(d);
    }
    let (layout, index, _) = b.build();
    assert_eq!(
        index.member_name("worldTime", t("float")),
        Some("sb_as_float_worldTime")
    );
    check(&layout, "tricky");
}

/// Deterministic pseudo-random mix of every valid member shape (all scalar kinds,
/// vectors, float and double matrices, arrays), split over both blocks. Each block is
/// compiled in full and as an every-other-member subset with gaps, which is what a
/// translated program declares (only the members it uses, at the pack-global
/// offsets). glslang rejects misaligned or overlapping explicit offsets, so this
/// exercises the hole-filling packer under pressure.
#[test]
fn pseudo_random_shapes_laid_out_by_glslang_rules() {
    if !tool_available("glslangValidator")
        || !tool_available("spirv-dis")
        || !tool_available("spirv-val")
    {
        eprintln!("glslangValidator/spirv-dis/spirv-val not found; skipping");
        return;
    }
    use sb_core::ScalarKind::{Bool, Double, Float, Int, Uint};
    let mut shapes = Vec::new();
    for scalar in [Float, Int, Uint, Bool, Double] {
        for rows in 1..=4u8 {
            shapes.push(GlslType::vector(scalar, rows));
        }
    }
    for scalar in [Float, Double] {
        for cols in 2..=4u8 {
            for rows in 2..=4u8 {
                shapes.push(GlslType {
                    scalar,
                    rows,
                    cols,
                    array: None,
                });
            }
        }
    }
    let mut state: u64 = 0x5eed_cafe_f00d_d00d;
    let mut next = |n: usize| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((state >> 33) as usize) % n
    };
    let mut b = LayoutBuilder::new();
    for i in 0..90 {
        let base = shapes[next(shapes.len())];
        let ty = match next(4) {
            0 => base.with_array(1 + next(4) as u32),
            _ => base,
        };
        let decl = UniformDecl::new(format!("r{i:02}"), ty, UniformSource::Unset);
        if i % 3 == 0 {
            b.add_draw(decl);
        } else {
            b.add_frame(decl);
        }
    }
    let (layout, _, diags) = b.build();
    assert!(diags.is_empty(), "{diags:?}");
    check(&layout, "random_full");
    let subset = |block: &BlockLayout| BlockLayout {
        members: block.members.iter().skip(1).step_by(2).cloned().collect(),
        ..block.clone()
    };
    let partial = UniformLayout {
        frame: subset(&layout.frame),
        draw: subset(&layout.draw),
    };
    check(&partial, "random_subset");
}
