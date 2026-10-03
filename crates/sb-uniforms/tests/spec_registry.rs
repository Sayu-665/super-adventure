//! Table-driven checks of the builtin registry against the research spec and the
//! OptiFine documentation.

mod common;

use common::{OPTIFINE_BOOL_PARAMETERS, OPTIFINE_DOC, SPEC_SECTION_3};
use sb_core::GlslType;
use sb_uniforms::registry::{self, Frequency, sources};
use std::collections::HashSet;

fn ty(s: &str) -> GlslType {
    GlslType::parse(s).unwrap_or_else(|| panic!("bad type {s}"))
}

#[test]
fn every_spec_uniform_is_registered_with_the_spec_type() {
    let mut seen = HashSet::new();
    for &(name, t) in SPEC_SECTION_3 {
        assert!(seen.insert(name), "{name} listed twice in the spec table");
        let b = registry::get(name)
            .unwrap_or_else(|| panic!("spec uniform `{name}` is missing from the registry"));
        assert_eq!(b.ty, ty(t), "type of `{name}`");
    }
}

#[test]
fn optifine_documented_types_are_compatible() {
    for &(name, t) in OPTIFINE_DOC {
        let b =
            registry::get(name).unwrap_or_else(|| panic!("OptiFine uniform `{name}` is missing"));
        assert!(
            registry::is_type_compatible(b.ty, ty(t)),
            "`{name}`: registry {} vs OptiFine {t}",
            b.ty
        );
        if b.ty != ty(t) {
            // The only documented divergence: hideGUI is int in OptiFine and bool in Iris.
            assert_eq!(name, "hideGUI");
        }
    }
}

#[test]
fn optifine_custom_uniform_parameters() {
    for &name in OPTIFINE_BOOL_PARAMETERS {
        assert_eq!(
            registry::custom_uniform_input_type(name),
            Some(GlslType::BOOL),
            "{name}"
        );
        assert_eq!(registry::frequency_of(name), Frequency::Frame, "{name}");
    }
    // OptiFine float parameters exist as Iris int/float uniforms.
    for (name, t) in [
        ("biome", "int"),
        ("biome_category", "int"),
        ("biome_precipitation", "int"),
        ("temperature", "float"),
        ("rainfall", "float"),
    ] {
        assert_eq!(
            registry::custom_uniform_input_type(name),
            Some(ty(t)),
            "{name}"
        );
    }
}

#[test]
fn every_registry_entry_is_accounted_for() {
    // Everything in the registry is either in the spec tables or a documented
    // ShaderBridge addition.
    let spec: HashSet<&str> = SPEC_SECTION_3
        .iter()
        .map(|(n, _)| *n)
        .chain(OPTIFINE_BOOL_PARAMETERS.iter().copied())
        .collect();
    for b in registry::all() {
        if spec.contains(b.name) {
            continue;
        }
        assert!(
            b.source == sources::SHADERBRIDGE || b.source == sources::VOXY,
            "`{}` is neither in the spec nor a ShaderBridge or Voxy addition",
            b.name
        );
    }
    let extras: Vec<&str> = registry::all()
        .iter()
        .filter(|b| b.source == sources::SHADERBRIDGE)
        .map(|b| b.name)
        .collect();
    assert_eq!(extras, vec!["sb_FogColor", "fogScale"]);
    assert_eq!(registry::get("sb_FogColor").unwrap().ty, GlslType::VEC4);
    assert_eq!(registry::get("fogScale").unwrap().ty, GlslType::FLOAT);
}

#[test]
fn sources_follow_the_spec_grouping() {
    for name in [
        "modelViewMatrix",
        "normalMatrix",
        "textureMatrix",
        "colorModulator",
        "chunkOffset",
        "modelOffset",
    ] {
        assert_eq!(registry::get(name).unwrap().source, sources::CORE, "{name}");
    }
    for name in [
        "dhProjection",
        "dhProjectionInverse",
        "dhPreviousProjection",
        "dhNearPlane",
        "dhFarPlane",
        "dhRenderDistance",
    ] {
        assert_eq!(registry::get(name).unwrap().source, sources::DH, "{name}");
    }
    for name in [
        "cameraPositionInt",
        "currentDate",
        "lightningBoltPosition",
        "gtextureSize",
        "pi",
    ] {
        assert_eq!(registry::get(name).unwrap().source, sources::IRIS, "{name}");
    }
    for &(name, _) in OPTIFINE_DOC {
        let source = registry::get(name).unwrap().source;
        assert!(
            source == sources::OPTIFINE || source == sources::CORE,
            "{name}: {source}"
        );
    }
}
