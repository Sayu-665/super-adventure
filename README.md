# ShaderBridge

**Run OptiFine/Iris shader packs on Minecraft Java Edition's Vulkan backend, with Distant Horizons.**

Minecraft 26.2 added a Vulkan renderer, and 26.4 snapshots make it the default.
Iris refuses to run on it ("Iris cannot run when using Vulkan"), so on Vulkan every
shader pack stops working. Distant Horizons already draws its LODs on Vulkan, but
with no pack shading.

ShaderBridge fills that gap. A Rust compiler translates **any** shader pack,
written in OpenGL-era compatibility GLSL against lenient desktop drivers, into
strict Vulkan GLSL 4.60 and validated SPIR-V. It also produces a complete,
host-agnostic description of the pack's render pipeline. A Fabric mod loads that
description and runs it inside Minecraft's own renderer. Distant Horizons LODs and
other mods' geometry are fed through data-driven *draw profiles*.

```
 shader pack (zip/dir)                     Rust (crates/)                                  Minecraft 26.3 (java/)
 ───────────────────── ─► sb-pack ─► sb-preprocess ─► sb-transform ─► sb-compile ─► sb-pipeline ─► CompiledPack ─JNI─► Fabric mod
  .vsh/.fsh/.gsh/.csh,      options,     JCPP/Iris-       compat GLSL →     glslang →       passes, flips,    JSON + SPIR-V     renderpearl pipelines,
  shaders.properties,       properties,  identical        Vulkan GLSL 460   SPIR-V 1.5,     targets, uniforms,                 targets, uniforms,
  block.properties …        id maps      preprocessor     (AST rewrite)     reflection      DH strategy                        DH + raw-Vulkan path
                                                                                    └─► sb-runtime: headless Vulkan executor (validation/CI)
```

## What it does
- **Faithful pack semantics.** The program resolution, fallback chains, option
  discovery, `shaders.properties`, directives (`RENDERTARGETS`, const settings),
  ping-pong flip schedule, custom uniforms and id maps follow Iris's
  implementation. The preprocessor's output is identical to Iris's own (JCPP) on
  all 1,799 corpus programs and byte-identical on all 38 `.properties` files.
- **Strict translation.** These are rewritten into explicit, validated Vulkan
  interfaces:
  - legacy built-ins (`gl_FragData`, `ftransform`, `gl_MultiTexCoord*`, the
    matrices, `texture2D`/`shadow2D`, `varying`/`attribute`);
  - loose uniforms, packed into std140 blocks;
  - samplers;
  - varyings, linked across stages with size-aware locations;
  - code that only lenient NVIDIA drivers accept: const demotion, int literals,
    reserved words, missing/mismatched varyings, unused functions.
- **Depth conventions.** Minecraft 26.2+ (and DH 3.3+) uses reversed-Z. Packs
  still see GL forward-Z matrices and depth values, because the translator remaps
  `gl_Position.z` and inverts depth reads, `gl_FragCoord.z` and `gl_FragDepth`.
  Forward and reversed renders of the same pack are checked to match.
- **Distant Horizons.**
  - Packs that ship `dh_*` programs are translated against DH 3.3's exact
    `BLAZE_3D` LOD vertex format.
  - For packs without DH support, `dh_terrain`/`dh_water`/`dh_shadow` are
    *synthesized* from the pack's own terrain, water and shadow programs.
- **Other mods.** Each host draw path (vanilla terrain, entities, particles, sky,
  DH LODs, Sodium chunks, fullscreen passes) is a TOML *draw profile*. Hosts can
  register more at runtime.

## Status
| Component | State |
|---|---|
| Rust compiler (`sb-pack`, `sb-preprocess`, `sb-expr`, `sb-uniforms`, `sb-transform`, `sb-compile`) | Implemented and tested |
| Corpus validation (8 + 45 real packs, glslang + `spirv-val` + reflection + interface checks) | 100% of root/world0 program stages with default options |
| Headless Vulkan executor (`sb-runtime`, lavapipe + Khronos validation) | Renders synthetic scenes incl. DH LODs with zero validation errors |
| Pipeline orchestration, CLI, JNI | In progress |
| Fabric mod: foundation (natives, model, config, GUI, uniform providers) | Implemented and unit-tested |
| Fabric mod: render integration (pipeline injection, frame orchestration, DH, raw-Vulkan compute) | In progress |

The mod can only be compile-verified here: the build machine has no GPU and no
display. The translated shaders run on real Vulkan in the headless executor.

## Building
```sh
# Rust (1.88+; glslang is built from source by glslang-sys, so a C++ compiler is needed)
cargo build --release
cargo test

# Fabric mod (Java 25; bundles the native library built by cargo)
cd java && ./gradlew build
```

Headless tests use any Vulkan 1.2 device. Mesa lavapipe works without a GPU:
`VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json`.

## Repository layout
- `crates/sb-core`: shared types and the `CompiledPack` model, the contract between Rust and Java.
- `crates/sb-pack`: pack VFS (dir/zip), properties, options, profiles, id maps.
- `crates/sb-preprocess`: Iris/JCPP-compatible GLSL preprocessor.
- `crates/sb-expr`: custom-uniform expression language.
- `crates/sb-uniforms`: builtin uniform registry, std140 layouts, resource canonicalization.
- `crates/sb-transform`: the GLSL translator, plus `profiles/*.toml` draw profiles.
- `crates/sb-compile`: glslang → SPIR-V, reflection, validation.
- `crates/sb-pipeline`: pack → `CompiledPack`.
- `crates/sb-runtime`: headless Vulkan executor.
- `crates/sb-jni`: native library for the mod.
- `crates/sb-cli`: the `shaderbridge` command.
- `java/`: the Fabric mod.
- `docs/ARCHITECTURE.md`: the design contract.

## License
MIT OR Apache-2.0. ShaderBridge contains no code from OptiFine, Iris or other shader
mods; their documented behaviour was used as a specification.

---
*About this repository:* this repo started as **my-coding-journey**, a place to post
progress on my coding journey (updates will be inconsistent). ShaderBridge is its
current project.
