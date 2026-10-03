//! The scene's vertex layouts against the draw profiles in
//! `crates/sb-transform/profiles/*.toml` (the contract with the translator and the Java
//! host): every input of a profile the scene feeds is provided by the layout of the same
//! name with a compatible numeric class and enough components, every layout element is
//! an input of the profile, per-instance inputs come from per-instance bindings, and
//! elements are tightly packed.

use ash::vk;
use sb_runtime::scene::formats::{ALL_LAYOUTS, VertexLayout, format_size};
use std::path::Path;

/// (numeric class, components) of a vertex attribute format.
fn format_info(f: vk::Format) -> (&'static str, u32) {
    match f {
        vk::Format::R32G32B32_SFLOAT => ("float", 3),
        vk::Format::R32G32_SFLOAT => ("float", 2),
        vk::Format::R32_SFLOAT => ("float", 1),
        vk::Format::R8G8B8A8_UNORM | vk::Format::R8G8B8A8_SNORM => ("float", 4),
        vk::Format::R16G16_SINT => ("int", 2),
        vk::Format::R8G8B8A8_SINT => ("int", 4),
        vk::Format::R32G32B32_SINT => ("int", 3),
        vk::Format::R8_UINT | vk::Format::R16_UINT | vk::Format::R32_UINT => ("uint", 1),
        vk::Format::R16G16_UINT | vk::Format::R32G32_UINT => ("uint", 2),
        vk::Format::R16G16B16_UINT => ("uint", 3),
        vk::Format::R8G8B8A8_UINT => ("uint", 4),
        other => panic!("unexpected vertex format {other:?}"),
    }
}

/// (numeric class, components) of a GLSL attribute type.
fn glsl_info(ty: &str) -> (&'static str, u32) {
    let (class, rest) = match ty.as_bytes().first() {
        Some(b'i') if ty != "int" => ("int", &ty[1..]),
        Some(b'u') if ty != "uint" => ("uint", &ty[1..]),
        _ => ("float", ty),
    };
    match (ty, rest) {
        ("int", _) => ("int", 1),
        ("uint", _) => ("uint", 1),
        (_, "float") => (class, 1),
        (_, r) if r.starts_with("vec") => (class, r[3..].parse().unwrap_or_else(|_| panic!("type {ty}"))),
        _ => panic!("unexpected attribute type {ty}"),
    }
}

struct Input {
    name: String,
    ty: String,
    instanced: bool,
}

fn profile_inputs(path: &Path) -> (String, Vec<Input>) {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let table: toml::Table = text.parse().unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let name = table["name"].as_str().expect("profile name").to_string();
    let inputs = table
        .get("inputs")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .map(|i| Input {
                    name: i["name"].as_str().expect("input name").to_string(),
                    ty: i["type"].as_str().expect("input type").to_string(),
                    instanced: i.get("instanced").and_then(|v| v.as_bool()).unwrap_or(false),
                })
                .collect()
        })
        .unwrap_or_default();
    (name, inputs)
}

fn check(layout: &VertexLayout, inputs: &[Input]) -> Vec<String> {
    let mut problems = Vec::new();
    for i in inputs {
        let Some(e) = layout.element(&i.name) else {
            problems.push(format!("{}: input `{}` has no element", layout.profile, i.name));
            continue;
        };
        let (fclass, fcomps) = format_info(e.format);
        let (gclass, gcomps) = glsl_info(&i.ty);
        if fclass != gclass || fcomps < gcomps {
            problems.push(format!("{}: `{}` is {} but the element is {:?}", layout.profile, i.name, i.ty, e.format));
        }
        let per_instance = layout.bindings.iter().find(|b| b.binding == e.binding).is_some_and(|b| b.per_instance);
        if per_instance != i.instanced {
            problems.push(format!("{}: `{}` instanced={} but its binding per_instance={per_instance}", layout.profile, i.name, i.instanced));
        }
    }
    for e in layout.elements {
        if !inputs.iter().any(|i| i.name == e.name) {
            problems.push(format!("{}: element `{}` is not an input of the profile", layout.profile, e.name));
        }
    }
    for b in layout.bindings {
        let mut elems: Vec<_> = layout.elements.iter().filter(|e| e.binding == b.binding).collect();
        elems.sort_by_key(|e| e.offset);
        let mut end = 0;
        for e in elems {
            if e.offset != end {
                problems.push(format!("{}: `{}` at {} leaves a gap or overlaps (expected {end})", layout.profile, e.name, e.offset));
            }
            end = e.offset + format_size(e.format);
        }
        if end != b.stride {
            problems.push(format!("{}: binding {} stride {} but elements end at {end}", layout.profile, b.binding, b.stride));
        }
    }
    problems
}

#[test]
fn scene_layouts_match_the_draw_profiles() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../sb-transform/profiles");
    let mut problems = Vec::new();
    let mut checked = Vec::new();
    for layout in ALL_LAYOUTS {
        let path = dir.join(format!("{}.toml", layout.profile));
        if !path.exists() {
            problems.push(format!("layout `{}` has no profile file {}", layout.profile, path.display()));
            continue;
        }
        let (name, inputs) = profile_inputs(&path);
        assert_eq!(name, layout.profile, "{}", path.display());
        problems.extend(check(layout, &inputs));
        checked.push(name);
    }
    assert!(problems.is_empty(), "{problems:#?}");
    for required in ["vanilla_terrain", "vanilla_entity", "vanilla_position", "vanilla_position_tex", "dh_terrain", "sodium_terrain"] {
        assert!(checked.iter().any(|c| c == required), "{required} not checked: {checked:?}");
    }
}

#[test]
fn glsl_type_parsing() {
    assert_eq!(glsl_info("uvec3"), ("uint", 3));
    assert_eq!(glsl_info("ivec2"), ("int", 2));
    assert_eq!(glsl_info("vec4"), ("float", 4));
    assert_eq!(glsl_info("float"), ("float", 1));
    assert_eq!(glsl_info("uint"), ("uint", 1));
    assert_eq!(glsl_info("int"), ("int", 1));
}
