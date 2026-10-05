# ShaderBridge

**Run OptiFine/Iris shader packs on Minecraft Java Edition's Vulkan backend, with Distant Horizons.**

Minecraft 26.2 added a Vulkan renderer. Minecraft 26.3 still starts on OpenGL by default (Vulkan
is an opt-in setting), and the 26.4 snapshots make Vulkan the default. Iris refuses to run on
Vulkan ("Iris cannot run when using Vulkan"), so on Vulkan every shader pack stops working.
Distant Horizons already draws its LODs on Vulkan, but with no pack shading.

ShaderBridge fills that gap. A Rust compiler translates **any** shader pack, written in
OpenGL-era compatibility GLSL against lenient desktop drivers, into strict Vulkan GLSL 4.60 and
validated SPIR-V. It also produces a complete, host-agnostic description of the pack's render
pipeline. A Fabric mod loads that description and runs it inside Minecraft's own renderer.
Distant Horizons LODs, Sodium's terrain and other mods' geometry are fed through data-driven
*draw profiles*.

```
 shader pack (zip/dir)                     Rust (crates/)                                  Minecraft 26.3 (java/)
 ───────────────────── ─► sb-pack ─► sb-preprocess ─► sb-transform ─► sb-compile ─► sb-pipeline ─► CompiledPack ─JNI─► Fabric mod
  .vsh/.fsh/.gsh/.csh,      options,     JCPP/Iris-       compat GLSL →     glslang →       passes, flips,    JSON + SPIR-V     renderpearl pipelines,
  shaders.properties,       properties,  compatible       Vulkan GLSL 460   SPIR-V 1.5,     targets, uniforms,                 targets, uniforms,
  block.properties …        id maps      preprocessor     (AST rewrite)     reflection      DH strategy                        DH, Sodium, raw Vulkan
                                                                                    └─► sb-runtime: headless Vulkan executor (validation/CI)
```

> **Status in one sentence.** The compiler translates and validates every program of the 55
> real packs it was tested on, and the headless Vulkan executor renders them with zero
> validation errors; the Fabric mod is complete enough to build, pass its unit and bytecode
> tests and load packs through JNI, but **it has never run inside Minecraft**: the development
> machine has no GPU and no display. Treat it as untested in game.

## What it does
- **Faithful pack semantics.** Program resolution, fallback chains, option discovery,
  `shaders.properties`, directives (`RENDERTARGETS`, const settings), the ping-pong flip
  schedule, custom uniforms, id maps, and per-geometry blend and alpha-test defaults follow
  Iris's implementation. The preprocessor's output equals Iris's own preprocessor (JCPP) on all
  1,799 corpus programs, apart from the deliberate `sb_kw_` escaping of reserved words used as
  identifiers (245 programs); its output for the 38 corpus `.properties` files is byte-identical
  to JCPP's.
- **Strict translation.** These are rewritten into explicit, validated Vulkan interfaces:
  - legacy built-ins (`gl_FragData`, `ftransform`, `gl_MultiTexCoord*`, the matrices,
    `texture2D`/`shadow2D`, `varying`/`attribute`);
  - loose uniforms, packed into std140 blocks;
  - samplers;
  - varyings, linked across stages with size-aware locations;
  - code that only lenient NVIDIA drivers accept: const demotion, int literals, reserved words,
    missing/mismatched varyings, unused functions.
- **Depth conventions.** Minecraft 26.2+ (and DH 3.3+) uses reversed-Z. Packs still see GL
  forward-Z matrices and depth values, because the translator remaps `gl_Position.z` and inverts
  depth reads, `gl_FragCoord.z` and `gl_FragDepth`. Forward and reversed renders of the same pack
  are checked to match.
- **Distant Horizons.** Packs that ship `dh_*` programs are translated against DH 3.3's exact
  `BLAZE_3D` LOD vertex format. For packs without DH support, `dh_terrain`/`dh_water`/`dh_shadow`
  are *synthesized* from the pack's own terrain, water and shadow programs.
- **Other mods.** Each host draw path (vanilla terrain, entities, particles, sky, DH LODs, Sodium
  chunks, fullscreen passes) is a TOML *draw profile*. Hosts can register more at runtime.

## Status

| Component | State | Evidence |
|---|---|---|
| Rust compiler (`sb-pack`, `sb-preprocess`, `sb-expr`, `sb-uniforms`, `sb-transform`, `sb-compile`) | Implemented and tested | Unit, differential (JCPP, glslang, `java.util.Properties`) and corpus tests ([VALIDATION](docs/VALIDATION.md#rust-tests)) |
| Pipeline orchestration, CLI, JNI (`sb-pipeline`, `sb-cli`, `sb-jni`) | Implemented and tested | Compile caching, on-demand variants, `shaderbridge validate`/`render`, JNI exports with unit tests and a Java native smoke test |
| Corpus validation: 55 real packs (12 small, 43 extended) | 55/55 packs pass, 5,641 programs, 11,141 SPIR-V modules, all pass `spirv-val` | [Corpus validate](docs/VALIDATION.md#corpus-validate) |
| Translator matrix (default and maximum options; forward, reversed, Renderpearl, DH-synth variants) | 100% of program stages compile (known pack-side bugs excluded and listed) | [Transform matrix](docs/VALIDATION.md#transform-matrix) |
| Headless Vulkan executor (`sb-runtime`, lavapipe + Khronos validation incl. synchronization) | Renders real packs, including DH LODs, with zero validation messages | [Renders](docs/VALIDATION.md#renders) |
| Fabric mod: foundation (natives, model, config, GUI, uniform providers) | Implemented and unit-tested | JUnit ([Java tests](docs/VALIDATION.md#java-build-and-tests)) |
| Fabric mod: render integration (main-pass takeover, shadow pass with entity shadows, DH LODs and generic objects, extended chunk vertex, Sodium 0.9 terrain, raw Vulkan path) | Implemented; **compile- and bytecode-verified only, never run in a game** | Every mixin target checked against the 26.3 and Sodium jars; see [JAVA_MOD.md](docs/JAVA_MOD.md#verification) |

Full numbers, per-pack tables and what is only compile-verified: **[docs/VALIDATION.md](docs/VALIDATION.md)**.
How the mod renders a pack: **[docs/JAVA_MOD.md](docs/JAVA_MOD.md)**. The design contract shared by
the crates and the mod: **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)**.

## Results

Packs rendered by the headless executor (`shaderbridge render`, 480×270, 30 frames, lavapipe,
Khronos validation on: 0 errors, 0 warnings for each). The scene is synthetic (procedural
terrain, trees, water, entities, sky and DH LODs), not Minecraft.

| Complementary Reimagined | Photon | Bliss |
|---|---|---|
| ![Complementary Reimagined](docs/images/render-complementary.png) | ![Photon](docs/images/render-photon.png) | ![Bliss](docs/images/render-bliss.png) |
| **BSL** | **Arc** (black specks on far LODs: open issue) | **Leaves before/after** the per-geometry blend fix |
| ![BSL](docs/images/render-bsl-shaders.png) | ![Arc](docs/images/render-arc-shader.png) | ![Leaves before and after](docs/images/leaves-before-after.png) |

## Known limitations

- **Never run in Minecraft.** No GPU and no display were available. Mixin application at
  runtime, every GPU call of the mod, the DH takeover and the Sodium integration are verified
  by compilation, bytecode checks and unit tests only. Expect bugs on first contact with a real
  game.
- **Minecraft 26.3 starts on OpenGL.** The mod renders packs on both backends, but the raw Vulkan
  path (compute programs, custom images, SSBOs, composite viewport scale) only exists on Vulkan:
  select *Options → Video Settings → Graphics API → Prefer Vulkan (Experimental)*. On OpenGL those
  programs are declined with a diagnostic.
- **Reversed-Z only in game.** Packs share Minecraft's depth buffer, so only the
  `ReversedZeroToOne` translation renders (the default); other depth modes are for `sb-runtime`.
- **Platforms.** The jar bundles the native library of the machine that built it (here Linux
  x86_64). Build on each target platform to support it.
- **Rendering gaps** (details in [JAVA_MOD.md §11](docs/JAVA_MOD.md#11-known-limitations)):
  entity shadows only from entities prepared for the camera; `entityId`/`blockEntityId` are -1;
  the hand stays vanilla; `layer.*` render-layer overrides are not applied; alpha-test defaults
  are per geometry slot, not per vanilla pipeline; `backFace.*` is ignored (as in Iris 26.3);
  `gl_InstanceID` ignores a non-zero base instance; Sodium terrain has no tangent attribute and
  `mc_chunkFade` is 1.
- **Pack-side issues.** Arc shader shows black specks on far LOD silhouettes (NaNs from its own
  view-position reconstruction, still open). Some packs' maximum-option configurations hit
  bugs in the packs themselves; they are listed in the [transform matrix](docs/VALIDATION.md#transform-matrix).
- **The test corpus is not shipped.** The packs are third-party; [VALIDATION.md](docs/VALIDATION.md#corpora)
  lists the exact versions and how to point the tests at them (`SB_CORPUS_DIR`, `SB_CORPUS_DIRS`,
  or a git-ignored `test-data/` directory at the repository root).

## Building

```sh
# Rust (1.88+; glslang is built from source by glslang-sys, so a C++ compiler is needed).
# The workspace's .cargo/config.toml builds glslang with -DNDEBUG; sb-compile refuses to build
# glslang with its asserts enabled.
cargo build --release
cargo test --workspace

# The command-line tool
cargo run --release -p sb-cli -- validate path/to/pack.zip
cargo run --release -p sb-cli -- render path/to/pack.zip -o out.png --depth reversed

# Fabric mod (Java 25). Builds crates/sb-jni with cargo and bundles it in the jar.
cd java && ./gradlew build              # add -PskipNative for a Java-only build
```

Headless tests use any Vulkan 1.2 device. Mesa lavapipe works without a GPU:
`VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json`.

## Installing the mod

1. Install Minecraft **26.3** with **Fabric Loader ≥ 0.19.5** and **Fabric API**, on **Java 25**.
2. Copy `java/build/libs/shaderbridge-0.1.0.jar` (built on the same OS/architecture as the
   player's) into the `mods` folder. Do not install Iris: the mod declares it incompatible.
3. Optional: **Distant Horizons ≥ 3.3.0** (verified against 3.3.4) and **Sodium 0.9.2 /
   0.9.3-alpha.1** for 26.3. With another Sodium version whose hooks moved, ShaderBridge refuses
   packs with a message instead of breaking Sodium.
4. Put shader packs (zip or folder) into `shaderpacks/`, select Vulkan in the video settings
   (see above), and pick a pack in ShaderBridge's screen (the *Shader Packs...* button in Video
   Settings, or the *Shader Packs* key binding). Packs
   are compiled in the background on world join and cached in `shaderbridge/cache`.

## Repository layout
- `crates/sb-core`: shared types and the `CompiledPack` model, the contract between Rust and Java.
- `crates/sb-pack`: pack VFS (dir/zip), properties, options, profiles, id maps.
- `crates/sb-preprocess`: Iris/JCPP-compatible GLSL preprocessor.
- `crates/sb-expr`: custom-uniform expression language.
- `crates/sb-uniforms`: builtin uniform registry, std140 layouts, resource canonicalization.
- `crates/sb-transform`: the GLSL translator, plus `profiles/*.toml` draw profiles.
- `crates/sb-compile`: glslang → SPIR-V, reflection, validation.
- `crates/sb-pipeline`: pack → `CompiledPack`, compile caches, on-demand variants.
- `crates/sb-runtime`: headless Vulkan executor.
- `crates/sb-jni`: native library for the mod.
- `crates/sb-cli`: the `shaderbridge` command.
- `java/`: the Fabric mod.
- `docs/ARCHITECTURE.md`: the design contract.
- `docs/JAVA_MOD.md`: how the Fabric mod renders a compiled pack, and what is verified.
- `docs/VALIDATION.md`: test, corpus and render results.

## License
MIT OR Apache-2.0. ShaderBridge contains no code from OptiFine, Iris, Sodium or Distant Horizons;
their documented behaviour and sources were used as a specification.

---
*About this repository:* this repo started as **my-coding-journey**, a place to post
progress on my coding journey (updates will be inconsistent). ShaderBridge is its
current project.
