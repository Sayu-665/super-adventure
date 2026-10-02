# ShaderBridge — Architecture

ShaderBridge runs **OptiFine/Iris-format shader packs on Minecraft Java Edition's
Vulkan backend**, including **Distant Horizons (DH)** LODs and draw paths added by
other mods. Its core is a shader compiler written in Rust. The Fabric mod that
loads it is a thin Java layer.

This document is the design contract shared by all crates and the Java mod.
Everything marked **(contract)** is relied on across component boundaries.
Changing it means updating every consumer.

---

## 1. Why this exists (ground truth, October 2026)

Facts checked first-hand against Mojang's version JSON, the decompiled 26.3
`client.jar`, and the released Iris and DH jars:

* Minecraft **26.3** (Java 25, unobfuscated) ships a new renderer,
  `com.mojang.renderpearl`, with **OpenGL and Vulkan backends**. Core shaders
  are GLSL, compiled to SPIR-V by shaderc for a Vulkan 1.2 target in
  `frontend/shaders/GlslCompiler`. Backends consume SPIR-V. On GL it is
  cross-compiled back to GLSL with spvc.
* **Iris 1.11.x refuses to run on the Vulkan backend.** Its own message reads:
  *"Iris cannot run when using Vulkan. Would you like to switch to OpenGL?"*
* **Distant Horizons 3.3.x** has a `BLAZE_3D` rendering engine, the default for
  MC ≥ 26.1.2, which supports Vulkan. With Iris installed it falls back to
  OpenGL, and notes that this "will be unavailable once Minecraft moves to Vulkan".
* Shader packs are written in **legacy and compatibility-profile GLSL**:
  `#version 120` through `440 compatibility`. They use `gl_FragData`,
  `ftransform()`, `gl_MultiTexCoord*`, `texture2D` and loose uniforms, and
  their code is only tested against lenient OpenGL drivers. Vulkan/SPIR-V
  compilers reject all of this.

ShaderBridge fills this gap. It translates any pack to strict Vulkan GLSL 4.50
and SPIR-V, assigns explicit interfaces and resources, and describes the pack's
complete render pipeline as data that a Vulkan host can execute.

## 2. Components

```
crates/
  sb-core        shared types + the CompiledPack model (contract)
  sb-pack        pack VFS (dir/zip), properties files, options, id maps, lang
  sb-preprocess  GLSL preprocessor (includes, macros, conditionals, line maps)
  sb-expr        expression language (custom uniforms, program.X.enabled)
  sb-uniforms    builtin-uniform registry + std140 layout
  sb-transform   compat GLSL -> Vulkan GLSL 450 (AST rewriting, linking)
  sb-compile     glslang -> SPIR-V, spirq reflection, validation
  sb-pipeline    orchestration: pack -> CompiledPack (programs, passes, targets, DH)
  sb-runtime     headless Vulkan executor (ash) for validation and rendering
  sb-jni         cdylib exposing the API to the Java mod via JNI
  sb-cli         `shaderbridge` command-line tool
java/            Fabric mod for MC 26.3 (Java 25)
docs/            this file, plus format and usage docs
```

Dependency order: `core → {pack, preprocess, expr, uniforms} → transform → compile → pipeline → {runtime, jni, cli}`.

## 3. End-to-end flow

```
pack dir/zip ──sb-pack──► files, shaders.properties (raw + preprocessed), options, id maps
                │
                ▼ per dimension (world0 / world-1 / world1 / dimension.properties)
         program resolution (fallback chains, program.X.enabled, profiles)
                │
                ▼ per program stage (.vsh/.tcs/.tes/.gsh/.fsh/.csh)
   sb-preprocess: include expansion → option edits → macro/conditional expansion
                  (comments kept in active code, #version/#extension hoisted)
                │
                ├──► directive scan (RENDERTARGETS/DRAWBUFFERS, const directives)
                ▼
   sb-transform phase A (analyze): parse → StageInfo (uniforms, samplers, varyings, builtins…)
                │
   [pack barrier] sb-uniforms: pack-global uniform layout; binding table for every resource name
                │
   sb-transform phase B (link): cross-stage interface (locations, missing varyings, types)
   sb-transform phase C (rewrite+emit): Vulkan GLSL 450 per stage
                │
   sb-compile: glslang → SPIR-V (Vulkan 1.2 / SPIR-V 1.5) → spirq reflection → validation
                │
                ▼
   sb-pipeline: CompiledPack {render targets, flip schedule, passes, programs, uniform layout,
                              custom uniforms, DH programs (native or synthesized), diagnostics}
                │
        ┌───────┴─────────┬─────────────────┐
     sb-cli           sb-runtime         sb-jni ──► Java mod (renderpearl + DH API)
```

## 4. Clip-space, depth and winding conventions (contract)

Mojang's Vulkan backend (from `VulkanRenderPass` and `VulkanRenderPipeline`):
* The viewport has positive height, origin `(0,0)` and depth range `0..1`. The
  present blit flips Y.
* `frontFace = CLOCKWISE`. This reproduces GL's CCW winding in framebuffer space.
* On Vulkan, Mojang's projection matrices map depth to `[0,1]`
  (`RENDERPEARL_DEPTH_IS_ZERO_TO_ONE`).
* Since 26.2, depth is **reversed-Z**: near maps to 1, far maps to 0, the
  compare op is `GEQUAL`, and depth clears to `0.0`. DH ≥ 3.3 also renders
  reversed-Z.

Shader packs assume GL conventions throughout, for example
`gbufferProjectionInverse * vec4(uv, depth, 1) * 2 - 1`. ShaderBridge therefore:

1. **Feeds packs GL-style forward-Z matrices** (NDC z in `[-1,1]`, near→-1).
   The host converts its own projection into this form, as Iris does.
2. **Translates for a selectable `DepthMode`** (`TransformOptions::depth_mode`):
   * `ForwardZeroToOne`, the `sb-runtime` default. The last pre-raster stage
     runs `gl_Position.z = (gl_Position.z + gl_Position.w) * 0.5;` after the
     pack's `main()`. Depth then equals GL window depth. The host uses
     `LESS`/`LEQUAL` and clears to 1.0.
   * `ReversedZeroToOne`, the Java-mod default on 26.2+. It shares depth with
     vanilla and DH, following Iris's DepthTransformer:
     * `gl_Position.z = (gl_Position.w - gl_Position.z) * 0.5`.
     * Every depth-texture read becomes `1.0 - value` (`depthtex*`,
       `dhDepthTex*`, `shadowtex*` sampled as non-shadow samplers).
     * `gl_FragCoord.z` → `(1.0 - gl_FragCoord.z)`.
     * Writes `gl_FragDepth = x` → `gl_FragDepth = 1.0 - (x)`.
     * Shadow-comparison lookups flip their reference (`1.0 - ref`).

     The host uses `GEQUAL` and clears to 0.0.
   * `GlNegOneToOne` applies no remap. It is for hosts with GL's default clip control.

   Packs observe identical depth values in both `ZeroToOne` modes. The
   headless test renders the same pack in both modes and compares the
   images, to verify the reversed-Z rewrite.
3. **Does not flip Y.** With a positive viewport, NDC y=-1 maps to memory row 0,
   as in GL, where texture v=0 is row 0. Rendering and sampling therefore stay
   self-consistent, and `gl_FragCoord`/`dFdy` keep GL's memory-relative
   semantics. A `TransformOptions::flip_y` escape hatch exists for hosts that
   use a negative viewport.
4. Hosts must use `frontFace = CLOCKWISE` for pack pipelines, as Mojang does.
5. `gl_InstanceID` → `(gl_InstanceIndex - gl_BaseInstance)`. This needs
   `GL_ARB_shader_draw_parameters`, so the plain `gl_InstanceIndex` is used when
   the host guarantees base instance 0. `gl_VertexID` → `gl_VertexIndex`.

## 5. Descriptor and uniform conventions (contract)

Two output **targets** share one transformer:

* `Target::Vulkan`: explicit `set`/`binding` decorations. Used by `sb-runtime`
  and by raw-Vulkan host paths.
* `Target::Renderpearl`: no set/binding decorations, because Mojang's shaderc
  auto-binds and its PipelineBuilder **matches descriptors and vertex
  attributes by name**. Every resource therefore gets a unique, stable name.
  Profile-provided host resources keep the host's names, e.g. `Sampler0`,
  `DynamicTransforms`, DH's `vertUniqueUniformBlock`. Programs that need 1D/3D
  textures, storage images, SSBOs, compute, geometry or tessellation are marked
  `requires_raw_vulkan = true`, because Mojang's public pipeline API rejects
  all of these.

### 5.1 Uniform blocks

* **`sb_Frame`** (std140, `set 0, binding 0`): a **pack-global** block. It is
  the union of every *loose* (non-opaque) uniform declared by any program in
  the pack, plus every builtin referenced by a custom-uniform expression, plus
  the custom uniforms themselves. Each program declares only the members it
  uses, each with `layout(offset = N)`, so every program reads one shared,
  once-per-frame buffer. Members whose name is not a known builtin and not a
  custom uniform are `UniformSource::Unset`. They are zero-filled, matching
  GL's behaviour for unset uniforms.
* **`sb_Draw`** (std140, `set 0, binding 1`): per-draw values not supplied by
  a draw profile's host blocks. Examples: `entityId`, `blockEntityId`,
  `currentRenderedItemId`, `entityColor`, `alphaTestRef`, `renderStage`,
  `blendFunc`.
* Draw-profile host blocks, such as Mojang's `DynamicTransforms`, `Projection`,
  `Globals`, `ChunkSection`, or DH's `vertSharedUniformBlock`, are declared
  with the host's exact names and layouts. The host's own `setUniform` calls
  therefore keep working unchanged.
* Name conflicts are resolved per name and type. If two programs declare the
  same name with different types, the first type wins. Other declarations get
  their own member `name__<type>`, and a warning is emitted.

### 5.2 Resource binding table

A pack-global **BindingTable** maps every canonical resource name to
`(set, binding, kind)`:
* `set 1`: combined image samplers, i.e. all `sampler*` uniforms.
* `set 2`: storage images (`image*`) and SSBOs (`bufferObject.N` → binding `N`).

Aliases are canonicalized in the AST before assignment:
* `texture`, `tex` and `u_MainSampler` → `gtexture`
* `gcolor` → `colortex0`, `gdepth` → `colortex1`, `gnormal` → `colortex2`, `composite` → `colortex3`
* `gaux1..4` → `colortex4..7`
* `gdepthtex` → `depthtex0`
* `shadow`/`watershadow` follow the alias rule (`watershadow` declared ⇒
  `watershadow` = `shadowtex0`, `shadow` = `shadowtex1`; otherwise `shadow` = `shadowtex0`)
* `shadowcolor` → `shadowcolor0`
* `dhDepthTex` → `dhDepthTex0`

Each binding carries a `ResourceRef`. It tells the host what to bind:
`ColorTex{index}`, `DepthTex(i)`, `ShadowTex(i)`, `ShadowColor(i)`, `Noise`,
`Atlas`, `Lightmap`, `Normals`, `Specular`, `Overlay`, `DhDepthTex(i)`,
`DhBlockAtlas`, `CustomTexture(name)`, `Image(name)`, `ColorImage(i)`,
`ShadowColorImage(i)`, `Ssbo(i)`, `UniformBlock(name)` or
`Unknown(name)`.

Following GL texture-unit-0 semantics, an `Unknown` sampler aliases the atlas
in gbuffers and shadow programs and `colortex0` in fullscreen programs.

Composite-style programs ping-pong between main and alt targets. Each program
records the flip state of every `ColorTex` binding, so the host knows whether
to bind the main or alt image (§7).

## 6. Draw profiles (contract, data-driven)

A **draw profile** describes how a host draw path feeds geometry and per-draw
state to a program. It is the extensibility point for "other mods". Profiles
are TOML, embedded in `sb-transform`. A host (e.g. the Java mod, on behalf of
another mod) can register extra profiles at runtime.

```toml
name = "dh_terrain"                # unique id
host = "distanthorizons:blaze3d"   # informational
[[inputs]]   # vertex attributes, names MUST equal the host VertexFormat element names
name = "vPosition"   type = "uvec3"  location = 0
...
[[blocks]]   # host-provided uniform blocks, declared verbatim
name = "vertUniqueUniformBlock"  glsl = "vec3 uModelOffset;"
...
[[samplers]] # host samplers + which pack sampler names they satisfy
name = "uLightMap"  provides = ["lightmap"]
[semantics]  # GLSL expressions implementing compat semantics (vertex stage)
position   = "vec4(sb_dhWorldPos(), 1.0)"      # gl_Vertex
color      = "vColor"                          # gl_Color
uv0        = "vec4(0.5, 0.5, 0.0, 1.0)"        # gl_MultiTexCoord0
lightmap   = "vec4(sb_dhLight(), 0.0, 1.0)"    # gl_MultiTexCoord1 (0..240 range)
normal     = "sb_dhNormal()"                   # gl_Normal
entity     = "vec4(float(sb_dhBlockId()), 0.0, 0.0, 0.0)"  # mc_Entity
model_view = "..."   projection = "..."        # gl_ModelViewMatrix / gl_ProjectionMatrix
[code]       # helper functions injected before main
vertex = """ ... """
```

Built-in profiles:
* `fullscreen` is used by composite-style passes. It has **no vertex inputs**.
  Vertices are generated from `gl_VertexIndex` as a fullscreen triangle
  covering UV `[0,1]`. `gl_ProjectionMatrix` maps `[0,1]→[-1,1]`, `gl_Color`
  is 1, and the model-view, texture and normal matrices are identity, as in Iris.
* `vanilla_terrain`, `vanilla_entity` and `vanilla_generic` mirror Mojang
  26.3's core vertex formats and UBOs, plus the Iris extension attributes
  (`mc_Entity`, `mc_midTexCoord`, `at_tangent`, `at_midBlock`).
* `dh_terrain` and `dh_generic` mirror DH 3.3 `BLAZE_3D`: vertex format
  `vPosition` uvec3, `meta` uint, `vColor` vec4, `irisMaterial` uint,
  `irisNormal` uint, `textureTile` uint; blocks `vertUniqueUniformBlock`,
  `vertSharedUniformBlock`, `fragUniformBlock`; samplers `uLightMap`, `uBlockAtlas`.
* `test_scene` is used by `sb-runtime`'s synthetic scene. It is a plain,
  explicit format.

## 7. Pipeline model (CompiledPack) (contract)

Defined in `sb-core::model` and serialized as JSON. Binary payloads (SPIR-V
words and GLSL text) live in a separate blob table referenced by `BlobId`. The
JNI layer passes the JSON plus one concatenated byte buffer.

Key parts:
* `PackInfo`: name, source hash, ShaderBridge version, enabled feature flags.
* `OptionsModel`: options (boolean/value, defaults, allowed values, current
  values), screens, sliders, profiles and lang strings, so the host can build
  the options GUI.
* `IdMaps`: block, item and entity ids, render layers (`layer.*`).
* `DimensionPipeline[]`, one per world folder:
  * `targets`: `colortex0..31` (format, clear, clear color, mipmapped,
    size/scale, used flag), `shadowcolor0..7`, the shadow map (resolution,
    distance, fov, hardware filtering, mipmaps, nearest filtering), depth targets.
  * `settings`: every functional `shaders.properties` key, plus global consts
    (`sunPathRotation`, `ambientOcclusionLevel`, half-lives, `noiseTextureResolution`, …).
  * `uniforms`: `UniformLayout { frame: BlockLayout, draw: BlockLayout }` and
    `custom: Vec<CustomUniform>`. Each custom uniform has its expression
    source; the host-side evaluator in `sb-expr` is exposed through JNI.
  * `bindings`: the BindingTable.
  * `programs: Vec<Program>`:
    * name, kind, dimension folder, draw profile, `requires_raw_vulkan`
    * stage modules: SPIR-V blob, Vulkan GLSL blob, Renderpearl GLSL blob, entry point
    * `draw_buffers` (`RENDERTARGETS`)
    * output types per location, blend (global and per-buffer), alpha test,
      viewport `scale`/offset, `mipmap` targets
    * `bindings_used` (with `ResourceRef` and the main/alt choice)
    * vertex inputs (location, name, type, semantic), push constants
    * compute info (local size, `workGroups` / `workGroupsRender`, indirect)
    * `cull` / `backFace` overrides
  * `geometry`: map from `GeometryProgram` to program index, after fallback
    resolution. DH entries are tagged `native` or `synthesized`.
  * `passes`: an ordered list. Each entry is `PassGroup` (Setup, Begin, Shadow,
    ShadowComp, Prepare, GbuffersOpaque, Deferred, GbuffersTranslucent,
    Composite, Final) plus its programs, computes, and the **static flip
    schedule**: for each pass, the main/alt read/write state of every colortex,
    and the end-of-frame alt→main copies (as in Iris `CompositeRenderer`).
  * `gbuffer_attachments` / `shadow_attachments`: the sorted union of the
    colortex/shadowcolor indices written by all gbuffers/DH (resp. shadow)
    programs, when it fits in 8 attachments. All world geometry is then drawn
    in ONE render pass. Mojang's API fixes a pass's attachments when the pass
    is created, and Vulkan pipelines must match them. Each program's
    logical outputs are remapped to physical locations (`output_slots`) in
    that list, and unused slots are write-masked.
* `diagnostics`: severity, code, message, original file:line, program.

## 8. GLSL translation rules (sb-transform)

The input is preprocessed GLSL of any version. The output is
`#version 450` (or 460 if the source uses 460-only features),
**without a profile**, meeting Vulkan GLSL rules. glslang is the referee:
every emitted shader must compile with glslang targeting Vulkan 1.2.

Version and extensions:
* Parse with `max(src_version, 130)`, so `uint`/`uvec*` lex as types.
  Reserved words used as identifiers (`sample`, `input`, `output`, `filter`,
  `common`, `partition`, `active`, …) are escaped to `sb_kw_<name>` by the
  preprocessor when they are not keywords at the source version.
* Drop extensions that are core in 4.50 (`GL_ARB_shader_image_load_store`,
  `GL_ARB_explicit_attrib_location`, `GL_ARB_shading_language_packing`,
  `GL_ARB_texture_gather`, `GL_ARB_shader_texture_lod`,
  `GL_ARB_texture_query_levels`, `GL_ARB_shader_storage_buffer_object`,
  `GL_EXT_gpu_shader4`, `GL_ARB_gpu_shader5`, `GL_ARB_shader_bit_encoding`,
  `GL_ARB_separate_shader_objects`, `GL_ARB_compute_shader`, …).
* Keep extensions that glslang supports for Vulkan, such as `GL_KHR_shader_subgroup_*`.
* Drop extensions glslang does not know, with a warning.

Storage qualifiers:
* `attribute` → `in`
* `varying` → `out` in the producing stage, `in` in the consuming stage. In
  geometry shaders, `varying in` / `varying out` become `in` / `out`.
* Every stage `in`/`out` variable receives a location from the linker (phase B).
* Fragment outputs:
  * `gl_FragData[i]` (constant `i`) → `layout(location=i) out vec4 sb_FragData_i`
  * `gl_FragColor` → location 0
  * User `out` variables without a location: if there is one, it gets
    location 0. If there are several, they are numbered in the order of
    `outColor<i>` names, otherwise in declaration order, with a warning.
  * When `gl_FragData` is indexed dynamically, it becomes an `out vec4` array.

Built-in variables (vertex/pre-raster stages), via the draw profile's semantics:
* `gl_Vertex` → `position`
* `gl_Color` → `color`
* `gl_Normal` → `normal`
* `gl_MultiTexCoord0` → `uv0`
* `gl_MultiTexCoord1` and `gl_MultiTexCoord2` → `lightmap`
* `gl_MultiTexCoord3` → `mid_tex_coord`
* `gl_MultiTexCoord4..7` → `vec4(0,0,0,1)`
* `mc_Entity` → `entity`, `mc_midTexCoord` → `mid_tex_coord`,
  `at_tangent` → `tangent`, `at_midBlock` → `mid_block`, `at_velocity` → `vec3(0)`
* `vaPosition`, `vaColor`, `vaUV0`, `vaUV1`, `vaUV2`, `vaNormal` → the
  matching semantic. `chunkOffset`/`modelOffset` come from the profile, or 0.
* Matrices:
  * `gl_ModelViewMatrix` → `model_view`, `gl_ProjectionMatrix` → `projection`
  * `gl_ModelViewProjectionMatrix` → `projection * model_view`
  * `gl_NormalMatrix` → `normal_matrix` (default `mat3(transpose(inverse(model_view)))`)
  * `gl_TextureMatrix[0]` → `texture_matrix`. `gl_TextureMatrix[1]` (and `[2]`)
    → the lightmap matrix
    `mat4(vec4(1/256,0,0,0), vec4(0,1/256,0,0), vec4(0,0,1/256,0), vec4(1/32,1/32,1/32,1))`.
  * The `*Inverse`, `*Transpose` and `*InverseTranspose` variants use `inverse()`/`transpose()`.
* `ftransform()` → `(projection * (model_view * position))`
* `gl_FrontColor`, `gl_BackColor`, `gl_TexCoord[i]`, `gl_FogFragCoord`,
  `gl_FrontSecondaryColor` → generated varyings `sb_v_*`. In the fragment
  stage, `gl_Color`, `gl_TexCoord[i]`, `gl_FogFragCoord` and
  `gl_SecondaryColor` read the matching `sb_v_*` inputs.
* `gl_Fog.*` → `sb_Frame` members `fogColor`, `fogDensity`, `fogStart`,
  `fogEnd`, `fogScale`. `gl_ClipVertex` is removed.
* `gl_VertexID` → `gl_VertexIndex`, `gl_InstanceID` → `gl_InstanceIndex`.
* In fragment shaders, `gl_FragCoord` and `gl_FrontFacing` are unchanged (see §4).

Texture functions: the legacy names map to the core functions.
* `texture1D/2D/3D/Cube`, `texture2DRect` → `texture`
* `*Lod` → `textureLod`, `*Proj` → `textureProj`, `*ProjLod` → `textureProjLod`
* `*GradARB` and `*Grad` → `textureGrad`
* `shadow2D` / `shadow2DLod` / `shadow2DProj` → `vec4(texture(…))` etc.
  The result is splatted to vec4, preserving `.r`/`.x` swizzles.
* `texelFetch2D` → `texelFetch`; `textureSize2D` → `textureSize`.

Uniforms:
* Loose uniforms are removed and replaced with `sb_Frame` members, at the
  pack-global offsets.
* Identifiers keep their names, so `frameTimeCounter` is still
  `frameTimeCounter`. This works because members of an anonymous block are
  referenced unqualified.
* Uniform initializers (`uniform float x = 1.0;`) become the member's
  default value in the model.
* Uniform arrays become std140 arrays.
* Opaque uniforms are canonicalized (§5.2) and decorated per target.
  `sampler2DShadow` stays a shadow sampler; the host binds a compare sampler.
* Images keep their format qualifier. A missing format triggers
  `shaderStorageImageReadWithoutFormat`, and a diagnostic, if the image is read.

Alpha test (gbuffers/shadow):
* The fragment epilogue appends
  `if (!(sb_FragData_0.a <op> alphaTestRef)) discard;`.
* The op and reference come from `alphaTest.<prog>` or the program default:
  cutout programs use `GREATER 0.1` (Iris defaults).
* `alphaTestRef` lives in `sb_Draw`, so hosts can adjust it per draw.

Fixups for leniency in NVIDIA and old drivers (glslang is strict):
* Implicit int→float conversions in constructors, returns and assignments
  are made explicit using the analysis types.
* Missing `return` at the end of non-void functions gets a default return.
* `const` initializers that use non-constant built-ins are demoted from `const`.
* Global initializers that read uniforms move to the start of `main`.
* Varyings with mismatched types between stages are reconciled to the
  consumer's type, with a conversion in the producer.
* A fragment input with no producer gets a producer output initialized to 0.
* Duplicate declarations from multiple includes are deduplicated.
* Variables named after built-in functions that are later called
  (e.g. `texture`) are renamed.

Every rewrite is covered by unit tests on small snippets. The corpus test
(§11) is the integration referee.

## 9. Distant Horizons

Two strategies are selected per pack and recorded in the model:

* **Native.** The pack ships `dh_terrain` / `dh_water` / `dh_shadow` /
  `dh_generic`. These are translated with the `dh_*` profiles, and the pack
  gets `dhProjection*`, `dhNearPlane`, `dhFarPlane`, `dhRenderDistance`,
  `dhDepthTex0/1` and `dhMaterialId` (`int`, from `irisMaterial`).
  `DISTANT_HORIZONS`, `DISTANT_HORIZONS_TEXTURES` and `DH_BLOCK_*` are defined.
* **Synthesized.** The pack has no DH programs. `dh_terrain` and `dh_water`
  are generated from the pack's resolved `gbuffers_terrain` and
  `gbuffers_water` sources, compiled with the `dh_*` profile:
  * Compat built-ins are fed from the decoded LOD vertex.
  * `gtexture` binds the DH block atlas when `textureTile != 0`, otherwise a
    white texel.
  * `mc_Entity.x` is mapped from the DH material to a representative block id
    via `block.properties`, e.g. `DH_BLOCK_LEAVES` → the id of `minecraft:oak_leaves`.
  * `dh_shadow` is generated from `shadow`.

  The model also marks `dh.unified_projection = true`. The host renders vanilla
  terrain and LODs with **one** projection whose far plane is the DH far plane,
  and reports `far` = the DH far plane. The pack's deferred and composite
  passes then see a single consistent depth space, without reading any
  DH-specific textures.

## 10. Hosts

* **sb-runtime** (headless) executes a CompiledPack with `ash` on any Vulkan
  1.2 device. In CI it uses Mesa lavapipe. It renders a synthetic scene: a
  procedural voxel terrain with a generated block atlas, plus entities, sky,
  and DH-format LOD terrain beyond render distance. It supports all pass
  types, ping-pong flips, shadow maps, computes, images, SSBOs, blending,
  alpha test and viewport scales, and writes PNGs.
* **The Java mod** (`java/`, Fabric, MC 26.3) does the following:
  * Loads the native library via JNI and manages packs and options, with its own GUI.
  * Compiles packs off-thread and creates pack render targets as renderpearl `GpuTexture`s.
  * Swaps vanilla and DH `RenderPipeline`s for pack programs at
    `RenderPass#setPipeline`. Pipelines are built from the
    `Target::Renderpearl` GLSL through a custom `ShaderSource`, so the same
    path works on Mojang's GL and Vulkan backends.
  * Redirects color attachments to the pack's gbuffer targets.
  * Fills `sb_Frame` / `sb_Draw` from game state, evaluating custom uniforms natively.
  * Runs composite-style passes as fullscreen draws.
  * Routes `requires_raw_vulkan` programs to raw Vulkan using LWJGL Vulkan
    on Mojang's `VkDevice`, or disables them with a diagnostic when the
    backend is GL.
  * Integrates with DH through its API events and pipeline substitution.

## 11. Verification

* Unit tests in every crate.
* **Corpus test** (`shaderbridge validate <packs…>`): every program in every
  dimension of 8 real packs is preprocessed, translated, compiled and
  validated with `spirv-val`. The pass rate is the headline metric; the
  target is 100%.
* **Headless render test**: lavapipe renders several packs, including their
  DH programs and a synthesized DH program, for a few frames. It checks that
  pipelines link, that no Vulkan validation errors occur when the layers are
  installed, and that images are non-degenerate.
* Java: Loom compile against MC 26.3 + Fabric API + DH API, plus a JUnit
  smoke test that loads the native library and compiles a pack through JNI.
