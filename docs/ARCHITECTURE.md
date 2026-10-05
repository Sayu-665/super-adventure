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

ShaderBridge fills this gap. It translates any pack to strict Vulkan GLSL 4.60
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
  sb-transform   compat GLSL -> Vulkan GLSL 460 (AST rewriting, linking), draw profiles
  sb-compile     glslang -> SPIR-V, spirq reflection, validation
  sb-pipeline    orchestration: pack -> CompiledPack (programs, passes, targets, DH), caches
  sb-runtime     headless Vulkan executor (ash) for validation and rendering
  sb-jni         cdylib exposing the API to the Java mod via JNI
  sb-cli         `shaderbridge` command-line tool
java/            Fabric mod for MC 26.3 (Java 25)
docs/            this file, and JAVA_MOD.md (how the Fabric mod renders a CompiledPack)
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
   sb-transform phase C (rewrite+emit): Vulkan GLSL 460 per stage
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
   * `ReversedZeroToOne`, the Java mod's default and the only mode it renders
     in game (§10). It shares depth with vanilla and DH, following Iris's
     DepthTransformer:
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
6. Hosts must create pipelines with tessellation stages with
   `VkPipelineTessellationDomainOriginStateCreateInfo { domainOrigin = LOWER_LEFT }`
   (core in Vulkan 1.1), as `sb-runtime` does. GL's tessellation domain origin is
   the lower left corner. Vulkan's default, upper left, mirrors the domain, so
   every triangle the tessellator generates gets the opposite winding. With
   `frontFace = CLOCKWISE` and back-face culling, the visible faces of tessellated
   geometry would then be culled (shrimple's tessellated terrain disappeared).

### 4.1 Distant Horizons conventions (contract)

Packs read DH state through uniforms. Hosts follow DH 3.3 under Iris:

* **Near plane.** `dhNearPlane` and the near plane of `dhProjection` (and
  `dhProjectionInverse`, `dhPreviousProjection`) are DH's near clip plane while a
  shader pack is active: `RenderUtil.getNearClipPlaneInBlocks()`, which DH's API
  exposes as `IDhApiRenderProxy.getNearClipPlaneDistanceInBlocks` and which Iris
  uses for both (`DHCompat.getProjection`). With a shader pack the overdraw
  prevention is 0.2, so the distance is `max(0.2 × vanilla render distance in
  blocks, 1)`, moved to the frustum corner of DH's fixed 70° field of view:
  `near / sqrt(1 + tan²(35°) · (aspect² + 1))`. DH's own LOD projection may use
  a closer plane; packs never see it. `dhFarPlane` and the far plane of
  `dhProjection` are `(DH render distance in blocks + 512) · √2` (Iris
  `DHCompatInternal.getFarPlane`; DH uses the same far clip plane while Iris is
  loaded). This applies to native programs; with the unified projection of
  synthesized programs (below) `dhProjection` is the level projection and
  `dhNearPlane` is Minecraft's near plane, 0.05.
* **Native `dh_*` programs** draw LODs into a depth buffer of their own with
  `dhProjection`: `dhDepthTex0` holds the LODs drawn so far, `dhDepthTex1` is its
  copy taken after the opaque LODs, before `dh_water`. The LODs **cover the
  vanilla area as well**. DH keeps LODs for all loaded terrain and only clips them
  at the near plane above, and packs discard LOD fragments inside the vanilla
  range themselves (their `dh_terrain` and `gbuffers_terrain` distance fades meet
  at about `far`). A host that leaves the vanilla area free of LODs opens holes at
  the transition.
* **Synthesized DH programs** (`DhStrategy::Synthesized`, §9) share the vanilla
  depth buffer and one **unified projection** (`dh.unified_projection`): the level
  projection's far plane is the DH far plane, `far` reports it, and
  `dhProjection` equals `gbufferProjection`. There is no separate depth buffer and
  no pack discard to hide LODs under vanilla terrain, so LODs are drawn **only
  where vanilla terrain does not surely cover the ground**. `sb-runtime` generates
  LOD cells only outside the vanilla chunk square; the Java mod skips every LOD
  section that lies entirely within (render distance − 1 chunk) of the camera, so
  sections crossing the edge overlap vanilla terrain instead of leaving a gap.
* **`dh_shadow`** draws in the shadow pass into **shadowcolor** targets on the
  shadow depth, like the `shadow` programs. Iris gives it a framebuffer of
  shadowcolor0/1 (`IrisRenderingPipeline.createDHFramebufferShadow`). Its
  `draw_buffers` are therefore shadowcolor indices, as for `shadow*` and
  `shadowcomp`, never colortex indices. Iris attaches shadowcolor0/1 whatever the
  program's `RENDERTARGETS` says, so `sb-pipeline` gives a pack's own `dh_shadow`
  the draw buffers `[0, 1]` and ignores its directive: fragment output `i` writes
  shadowcolor `i`, outputs past 1 are discarded, and a directive that would route
  differently (e.g. `RENDERTARGETS: 1`) gets the info diagnostic
  `dir.dh-shadow-draw-buffers`. Outputs the shader does not declare are not
  written (sb-runtime masks them). A synthesized `dh_shadow` (§9) keeps the
  draw buffers of the `shadow` program it is generated from.
* **`dh_generic`** vertices are 20 bytes: `vPosition` `RGB32_FLOAT`, `aColor`
  `RGBA8_UNORM` (normalized; DH's shader comment says `RGBA_FLOAT_COLOR`, but
  `BlazeDhGenericObjectRenderer` binds `RGBA_UBYTE_COLOR`), `aMaterial` `R8_UINT`
  and three padding bytes.

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
  all of these. So are vertex + fragment programs whose stage interface Mojang's
  `PipelineBuilder` cannot express (diagnostic `xf.renderpearl-interface`) and
  programs with more descriptors than `DeviceCaps::max_descriptors_per_program`
  (32 push descriptors on Mojang 26.3; `xf.too-many-descriptors`). §10 says
  what a host does with them.

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
  their own member `sb_as_<type>_<name>` (e.g. `sb_as_float_worldTime`), and a
  warning is emitted. Conflicting sampler/image declarations of one resource name
  get bindings named the same way (`sb_as_shadow_shadowtex1`, `sb_as_uint_colortex2`;
  §5.2). Derived names use the reserved `sb_as_` prefix (the translator renames
  every non-uniform pack identifier starting with `sb_` to `sbu_*`) and never contain
  `__`, which GLSL reserves; collisions get a `_<n>` counter. See `sb_uniforms::naming`.

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
`ColorTex(i)`, `DepthTex(i)`, `ShadowTex(i)`, `ShadowTexHw(i)` (the
hardware-filtering variant `shadowtex0HW`), `ShadowColor(i)`, `Noise`,
`Atlas`, `Lightmap`, `Normals`, `Specular`, `Overlay`, `DhDepthTex(i)`,
`DhBlockAtlas`, `White` (a constant 1×1 white texture), `CustomTexture(id)`,
`Image(name)`, `ColorImage(i)`, `ShadowColorImage(i)`, `Ssbo(i)`,
`UniformBlock(name)` or `Unknown(name)`.

Following GL texture-unit-0 semantics, an `Unknown` sampler aliases the atlas
in gbuffers and shadow programs, `White` in DH programs (whose `gtexture` is
white as well, §9) and `colortex0` in fullscreen programs.

Each binding also carries a `ResourceKind`. In the model it describes what the
translated shaders declare, which is what their SPIR-V reflection reports and
what the host binds:
* Rectangle samplers (`sampler2DRect`, `isampler2DRect`, `usampler2DRect`,
  `sampler2DRectShadow`) are declared as their 2D equivalents, so their kind
  has dim `2d`. Vulkan has no rectangle textures; the translator converts
  their texel coordinates to normalized ones (`sb-transform` `rect.rs`). A raw
  `TEXTURE_RECTANGLE` custom texture keeps `2d_rect` in its `CustomTexture` id
  and is created as a 2D texture.
* `shadow: true` means a comparison sampler. The host binds a sampler with
  comparison enabled, using the compare op of the depth mode (§4).
* Hosts without comparison samplers set `DeviceCaps::comparison_samplers =
  false` (Mojang's 26.3 `GpuSampler` cannot compare). The translator then
  emulates `sampler2DShadow`, and `sampler2DRectShadow` after lowering it: the
  shaders declare a plain `sampler2D` and compare in code (a 2×2
  percentage-closer filter over `textureGather`). These bindings have
  `shadow: false`, so the host binds the depth texture with a plain,
  non-comparison sampler. Each program's `BindingUse::shadow_emulated` is
  `true` for them. The flag is informational; `shadow` alone decides the
  sampler, and the flag defaults to `false` when absent. Array and cube
  comparison samplers are never emulated and keep `shadow: true`.

Composite-style programs ping-pong between main and alt targets. Each program
records the flip state of every `ColorTex` binding, so the host knows whether
to bind the main or alt image (§7).

## 6. Draw profiles (contract, data-driven)

A **draw profile** describes how a host draw path feeds geometry and per-draw
state to a program. It is the extensibility point for "other mods". Profiles
are TOML, embedded in `sb-transform`. A host (e.g. the Java mod, on behalf of
another mod) can register extra profiles at runtime
(`sb_pipeline::CompileSettings::extra_profiles`; JNI `registerProfile`, used by
every later compile).

```toml
# Excerpt of profiles/dh_terrain.toml; profiles/README.md has the full schema.
name = "dh_terrain"                # unique id
description = "..."                # free text
world_space = true                 # camera-relative world geometry: in the shadow pass,
                                   # model_view/projection become the shadow matrices
[[inputs]]   # vertex attributes, names MUST equal the host VertexFormat element names
name = "vPosition"  type = "uvec3"  location = 0
...
[[blocks]]   # host std140 uniform blocks, declared verbatim with an instance name
name = "vertUniqueUniformBlock"  instance = "sb_hDhUnique"  members = "vec3 uModelOffset;"
...
[[samplers]] # host samplers + which pack sampler names they satisfy
name = "uLightMap"  type = "sampler2D"  provides = ["lightmap"]
[semantics]  # GLSL expressions implementing compat semantics (vertex stage)
position   = "vec4(sb_dhCameraRelativePosition(), 1.0)"   # gl_Vertex
color      = "vColor"                                      # gl_Color
uv0        = "vec4(0.0, 0.0, 0.0, 1.0)"                    # gl_MultiTexCoord0
normal     = "sb_dhNormal(irisNormal)"                     # gl_Normal
entity     = "vec4(float(sb_dhPackBlockId(irisMaterial)), 0.0, 0.0, 1.0)"  # mc_Entity
model_view = "gbufferModelView"   projection = "dhProjection"
[[globals]]  # extra pack-visible variables initialized in the vertex prologue
name = "dhMaterialId"  type = "int"  stage = "vertex"  init = "int(irisMaterial)"
[code]       # helper functions injected before the pack's code
vertex = """ ... """
```

Profiles may also declare host push constants (`push_constants`, std430
offsets) and mark themselves `fullscreen` (no vertex buffer). Semantics a profile
leaves out come from `profiles/defaults.toml`. Builtin uniforms a profile's
expressions reference are added to the pack's `sb_Frame`/`sb_Draw` layout.

Built-in profiles (embedded in `sb-transform`, listed by `shaderbridge profiles`):
* `fullscreen` is used by composite-style passes. It has **no vertex inputs**.
  Vertices are generated from `gl_VertexIndex` as Iris' fullscreen quad, UV
  `[0,1]^2` drawn as two triangles (6 vertices; not one covering triangle,
  because packs remap `gl_Position` into sub-rectangles such as bloom tiles).
  `gl_ProjectionMatrix` maps `[0,1]→[-1,1]`, `gl_Color` is 1, and the
  model-view, texture and normal matrices are identity, as in Iris.
* The `vanilla_*` profiles mirror Minecraft 26.3's vertex formats and uniform
  blocks, one per family of vanilla pipelines (the Java mod's
  `VanillaPipelineTable` maps every vanilla pipeline to one, JAVA_MOD.md §4):
  * `vanilla_terrain` (MultiDrawIndirect terrain path) and
    `vanilla_terrain_section_ext` (per-section path, `ChunkSection` block) read
    Mojang's `BLOCK` elements plus the extension attributes the Java mod meshes
    chunk sections with while a pack is active (52-byte vertices): `sb_Normal`,
    `sb_Entity`, `sb_MidTexCoord`, `sb_Tangent` and `sb_MidBlock`, which feed
    `gl_Normal`, `mc_Entity`, `mc_midTexCoord`, `at_tangent` and `at_midBlock`.
    `vanilla_terrain_basic` and `vanilla_terrain_section` read the unmodified
    `BLOCK` format (normal up, `mc_Entity.x = -1`).
  * `vanilla_block` (block models drawn outside chunk meshes, beacon beams,
    block breaking), `vanilla_entity` (entities, block entities, items, hand),
    `vanilla_particle`, `vanilla_lines`, `vanilla_text`, `vanilla_clouds` (no
    vertex buffer: faces are decoded from the `CloudFaces` texel buffer),
    `vanilla_position`, `vanilla_position_color`,
    `vanilla_position_color_lightmap`, `vanilla_position_tex` and
    `vanilla_position_tex_color`.
* `dh_terrain` and `dh_generic` mirror DH 3.3 `BLAZE_3D`. `dh_terrain`: vertex
  format `vPosition` uvec3, `meta` uint, `vColor` vec4, `irisMaterial` uint,
  `irisNormal` uint, `textureTile` uint; blocks `vertUniqueUniformBlock`,
  `vertSharedUniformBlock`; samplers `uLightMap`, `uBlockAtlas` (`dhBlockAtlas`).
  `dh_generic` (beacon beams, clouds, API boxes): `vPosition` vec3, `aColor` vec4
  (`RGBA8_UNORM`), `aMaterial` uint; block `vertUniformBlock`; sampler `uLightMap`
  (§4.1). `dh_terrain_synth` (`sb_transform::DH_SYNTH_PROFILE`) is derived from
  `dh_terrain` in code for synthesized DH programs (§9); only its lightmap
  differs, in the vanilla terrain convention their sources expect.
* `sodium_terrain` mirrors Sodium 0.9's terrain path on Minecraft 26.3: Sodium's
  20-byte compact chunk vertex (`a_Position`, `a_Color`, `a_TexCoord`,
  `a_LightAndData`) followed by ShaderBridge's extension (`sb_Entity`,
  `sb_Normal`, `sb_MidTexCoord`, `sb_MidBlock`; 36 bytes per vertex, there is
  no tangent attribute), Sodium's 20-byte push constants (`u_RegionOffset`,
  `u_CurrentTime`, `u_RegionID`), its `u_Globals` block and its samplers
  `u_BlockTex` and `u_LightTex` (§10).

Every geometry slot is compiled for a default profile
(`sb_transform::default_profile_for`): `vanilla_terrain` for the terrain,
water, damaged-block and terrain shadow programs (`shadow`, `shadow_solid`,
`shadow_cutout`, `shadow_water`), `vanilla_entity` for the entity-like programs
(entities, block entities, hand, items, glint, spider eyes, lightning) and
their shadow programs, `dh_terrain`/`dh_generic` for the DH programs, and the
matching `vanilla_*` profile for the rest. Other profiles are
compiled as variants (§7). `sb-runtime`'s synthetic scene draws with a subset
of these profiles (`vanilla_terrain`, `vanilla_entity`, `vanilla_position`,
`vanilla_position_tex`, `dh_terrain`/`dh_terrain_synth` and `sodium_terrain`,
plus `fullscreen`), with byte-exact vertex buffers.

## 7. Pipeline model (CompiledPack) (contract)

Defined in `sb-core::model` and serialized as JSON. Binary payloads (SPIR-V
words and GLSL text) live in a separate blob table referenced by `BlobId`. The
JNI layer passes the JSON plus one concatenated byte buffer.

Key parts:
* `PackInfo`: name, source hash, ShaderBridge version, enabled feature flags.
  The source hash is the compile-cache key (§7.1). It covers the pack files,
  option values, environment, ShaderBridge version and the translator revision,
  so a changed translator never reuses cached output.
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
  * `uniforms`: `UniformLayout { frame: BlockLayout, draw: BlockLayout }`, and
    `custom_uniforms: Vec<CustomUniform>`. Each custom uniform has its expression
    source; the host-side evaluator in `sb-expr` is exposed through JNI.
  * `bindings`: the BindingTable.
  * `programs: Vec<Program>`:
    * name (with its folder, e.g. `world0/gbuffers_terrain`), kind, draw
      profile, `requires_raw_vulkan`, `synthesized_from`
    * stage modules: SPIR-V blob, Vulkan GLSL blob, Renderpearl GLSL blob, entry point
    * `draw_buffers` (`RENDERTARGETS`)
    * output types per location, blend (global and per-buffer), alpha test,
      viewport `scale`/offset, `mipmap` targets
    * `bindings_used` (with `ResourceRef`, the main/alt choice and
      `shadow_emulated`, §5.2)
    * vertex inputs (location, name, type, semantic), push constants
    * compute info (local size, `workGroups` / `workGroupsRender`, indirect)
    * `cull` / `backFace` overrides
  * `geometry`: map from `GeometryProgram` to a `GeometrySlot` after fallback
    resolution: `program` (index of the program drawing the slot with its
    default draw profile, §6) and `resolved_from` (the program actually used,
    which may be a fallback). A gbuffers slot whose whole fallback chain is
    missing gets Iris's fallback program (vanilla-like, into colortex0;
    `synthesized_from = "<iris fallback>"`). A slot is absent when every
    program of its chain failed to compile (the host then draws that geometry
    unshaded into `settings.fallback_tex`), and for shadow and DH geometry the
    pack has no program for. The dimension's `distant_horizons.strategy` says
    whether the DH slots hold the pack's own (`native`) or `synthesized`
    programs; synthesized ones carry `synthesized_from` (the gbuffers or shadow
    program they were generated from, §9).
  * `GeometrySlot.variants` (absent in older models = empty): the same program
    translated for other draw profiles, as profile → program index, each with
    the `use_alt` flags of the slot's pass. It lists every variant the compile
    produced: those other slots needed, and those a host requested through
    `CompileSettings::profile_overrides` (slot → profiles). Variants compiled
    later are not listed: `sb_pipeline::compile_variant` (JNI `compileVariant`)
    produces any other (slot, profile) variant on demand, for every built-in
    profile and every profile registered before the compile, with the folder's
    layout and binding table. The Java mod compiles its Sodium variants
    (`sodium_terrain`) this way.
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
  * `end_of_frame_copies`: colortex indices copied alt → main at the end of the
    frame (flipped an odd number of times and not cleared, as in Iris).
  * `distant_horizons`: `strategy` (`native`, `synthesized` or `disabled`),
    `unified_projection` (set for synthesized programs, §4.1) and
    `shadow_enabled` (a `dh_shadow` slot exists).
* `diagnostics`: severity, code, message, original file:line, program.

### 7.1 Compile caching

Compiles are cached at two levels, both keyed by content, so a stale result is
never reused:

* **On disk.** With `CompileSettings::cache_dir` (JNI `settingsJson.cacheDir`),
  `compile_pack` stores `<key>.json` + `<key>.bin` and reuses them when the key
  matches and the model's `format_version` is current. The key
  (`sb_pipeline::cache_key`, also `PackInfo::source_hash`) is a blake3 hash of
  the pack's content hash, the normalized option values, the environment, the
  ShaderBridge version, the model format version, the dimension filter, the
  language, the validation flag, the extra (registered) profiles, the
  `profile_overrides` and the **translator revision**
  (`sb_pipeline::TRANSLATOR_REVISION`). `sb-pipeline/build.rs` computes that
  revision as a hash of `src/**`, `profiles/**`, `Cargo.toml` and `build.rs` of
  every crate whose code determines a compile's output (`sb-core`, `sb-pack`,
  `sb-preprocess`, `sb-expr`, `sb-uniforms`, `sb-transform`, `sb-compile`,
  `sb-pipeline`), plus the workspace `Cargo.lock`, root `Cargo.toml` (dependency
  features, patches) and `.cargo/config.toml` (glslang's build flags). A
  changed translator therefore invalidates every cache, even without a version
  bump.
* **In memory, per `PackSession`** (one per open pack in `sb-jni`). The session
  keeps the preprocessed and analyzed stages, glslang's SPIR-V and reflection,
  `spirv-val` results, the Renderpearl compile check and whole translated
  **variants** (one program compiled for one draw profile). A variant is keyed
  by everything its translation reads: the folder fingerprint (environment,
  validation flag, `sb_Frame`/`sb_Draw` layout, member index, binding table),
  the program's resource context, the draw profile, the analysis keys and files
  of its stages, its kind and the full transform options of every target
  (class, shadow pass, alpha test, output slots, profile constants, depth mode,
  ...). A recompile after an option change therefore re-translates only the
  programs whose inputs changed, and `compile_variant` reuses the same cache
  (`CompileStats::variants_cached` counts a compile's reuses). Memory stays
  bounded in a long-lived session: the preprocessed-stage and variant caches
  keep two generations (entries the latest compile did not use are dropped at
  the next one), and the other tables are cleared when they reach 20,000
  entries.

## 8. GLSL translation rules (sb-transform)

The input is preprocessed GLSL of any version. The output is always
`#version 460`, **without a profile**, meeting Vulkan GLSL rules. glslang is
the referee: every emitted shader must compile with glslang targeting Vulkan 1.2.

Version and extensions:
* Parse as GLSL 4.60 and, if that fails, again at the source version clamped to
  130..460, so `uint`/`uvec*` always lex as types.
  Reserved words used as identifiers (`sample`, `input`, `output`, `filter`,
  `common`, `partition`, `active`, …) are escaped to `sb_kw_<name>` by the
  preprocessor when they are not keywords at the source version.
* Drop extensions that are core in 4.60 (`GL_ARB_shader_image_load_store`,
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
  `sampler2DShadow` stays a shadow sampler and the host binds a compare
  sampler, unless the host has no comparison samplers; then the comparison is
  emulated (§5.2).
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
* Optional, **off by default**: `min`, `max` and `clamp` on floats can be made to
  return the non-NaN operand, as on NVIDIA and AMD hardware. GLSL leaves
  `min(x, NaN)` undefined, and so does SPIR-V's `FMin`. With
  `CompileOptions::nan_tolerant_min_max`, `sb-compile` rewrites GLSL.std.450
  `FMin`/`FMax`/`FClamp` to `NMin`/`NMax`/`NClamp` in the emitted SPIR-V (integer
  forms are untouched). `sb-pipeline` compiles with the default, so the model's
  SPIR-V is glslang's output unchanged. The `Target::Renderpearl` GLSL text is
  never affected (when a host compiles it itself).

  The default rests on these measurements (October 2026, lavapipe / LLVM 20):
  * `NMin`'s result is always one of the results `FMin` may return, and
    `spirv-val` accepts the rewritten modules (sb-compile tests). The rewrite is
    nevertheless **not output-neutral**, presumably because the driver compiles
    the two forms differently: kappa-shader (480×270, 3 frames) differs in 59
    pixels by 1/255 with the rewrite, already in its shadow pass.
  * lavapipe already returns the non-NaN operand for `FMin`/`FMax`/`FClamp`
    (scalar and vector, constant and non-constant operands, both operand orders).
    Renders of arc-shader, Bliss, Complementary Reimagined and photon are
    pixel-identical with and without the rewrite.
  * The rewrite does **not** remove arc-shader's black specks on far LOD
    silhouettes. Those NaNs are created before any `min`/`max`/`clamp`: in
    `deferred4`, the view position reconstructed from depth
    (`unproject(gbufferProjectionInverse * clip)`) is non-finite for about 20
    pixels, which makes the SSGI weight `giF = NoL / (l + 1)` NaN. `deferred5`'s
    bilateral blur then spreads it (`0 × NaN`). When the reconstruction was
    instrumented, the NaNs disappeared, so this is a codegen-sensitive precision
    issue that is still open.

  With no pack that it fixes and a measurable change of output, the rewrite stays
  opt-in. It is the tool to reach for if a driver turns out to exploit `FMin`'s
  undefined NaN result.

Every rewrite is covered by unit tests on small snippets. The corpus test
(§11) is the integration referee.

## 9. Distant Horizons

`sb-pipeline` selects one strategy per world folder and records it in
`DimensionPipeline::distant_horizons` (§7):

* **Disabled.** The compile environment says DH is absent
  (`CompileEnvironment::distant_horizons = false`). No DH slots are compiled.
* **Native.** The pack ships `dh_terrain` or `dh_water` (that analyzes). Its
  `dh_terrain` / `dh_water` / `dh_shadow` / `dh_generic` programs are translated
  with the `dh_terrain` / `dh_generic` profiles (`dh_water` falls back to
  `dh_terrain`, as in Iris), and `dh_shadow` is skipped when
  `dhShadow.enabled=false`.
* **Synthesized.** The pack has neither. Each DH slot the pack has no program
  for is drawn by the pack's program for its source slot, translated as that DH
  program: `dh_terrain` (and `dh_generic`) from the `gbuffers_terrain` chain,
  `dh_water` from the `gbuffers_water` chain, and `dh_shadow` from `shadow`
  (only when `shadow.enabled` and `dhShadow.enabled` are not false and a
  `shadow` program exists). A DH program the pack does ship (e.g. only
  `dh_shadow`) is kept. The synthesized programs (`synthesized_from` = the
  source program):
  * use the `dh_terrain_synth` profile (`dh_generic` for `dh_generic`), whose
    compat built-ins are fed from the decoded LOD vertex, with the lightmap in
    the vanilla terrain convention their sources were written for;
  * get `mc_Entity.x` = the pack's `block.properties` id of a representative
    block of the LOD's DH material (e.g. `DH_BLOCK_LEAVES` → the id of
    `minecraft:oak_leaves`, -1 when the pack maps none), through the
    `SB_DH_BLOCK_ID_<n>` profile constants.

  The model also marks `unified_projection = true`: the host renders vanilla
  terrain and LODs with **one** projection whose far plane is the DH far plane,
  shares the vanilla depth buffer and reports `far` = the DH far plane (§4.1).
  The pack's deferred and composite passes then see a single consistent depth
  space, without reading any DH-specific textures.

With DH in the environment, `DISTANT_HORIZONS` and `DISTANT_HORIZONS_TEXTURES`
are defined (`DH_BLOCK_*` always are), and every DH program sees
`dhProjection*`, `dhNearPlane`, `dhFarPlane`, `dhRenderDistance`,
`dhDepthTex0/1` and `dhMaterialId` (`int`, from `irisMaterial`). As in Iris
(which builds DH programs without a texture), `gtexture` and the overlay sampler
of a DH program sample a constant white texture (`ResourceRef::White`); DH's
block atlas is `dhBlockAtlas`, also reachable through the `dh_terrain`
profile's `dh_sampleTexture()` helper.

### 9.1 Drawing the LODs (hosts)

Both hosts draw LODs with the slot programs per §4.1 and Iris's order:

* **Frame order.** Opaque LODs (`dh_terrain`) are drawn **before** vanilla
  opaque geometry, and `dhDepthTex1` is copied from the LOD depth before
  `deferred` (the Java mod copies it, and `dhDepthTex0`, right after
  `dh_terrain`). `dh_water` is drawn **before** vanilla translucent geometry,
  as DH 3.3 draws its deferred translucent pass at the start of Minecraft's
  translucent chunk layer, so vanilla water blends over the LOD water behind it
  (the Java mod copies `dhDepthTex0` again after it). In the shadow pass,
  `dh_shadow` draws the opaque LODs after the opaque terrain casters and before
  the `shadowtex1` copy; the Java mod uses the previous frame's LOD list there,
  because DH hands its list over only later in the frame.
* **Depth and projection.** Native programs draw into a separate LOD depth
  buffer with `dhProjection`, over the whole LOD area including the vanilla area;
  synthesized programs draw into Minecraft's depth with the unified projection,
  and only where vanilla terrain does not surely cover the ground (§4.1).
* **Host resources.** The host binds the `dh_terrain` profile's resources
  itself: `uLightMap` (Minecraft's lightmap), `uBlockAtlas` (DH's LOD block
  atlas), `vertSharedUniformBlock` per pass and one `vertUniqueUniformBlock`
  per LOD buffer. The profile's `gl_Vertex` is camera-relative:
  `vPosition + uModelOffset - uCameraPos`. `sb-runtime` passes DH's values (the
  buffer's minimum corner and the camera position); the Java mod passes
  `uModelOffset` = minimum corner − camera, computed in double precision, and
  `uCameraPos = 0`, so far-away coordinates keep their precision.

The **Java mod** gets the LODs from DH 3.3 (verified against the 3.3.4 sources
and jar) by reflection and through DH's API, without mixins, and fails soft:

* **Takeover.** While a pack renders, a `java.lang.reflect.Proxy` stands in for
  `LodRenderer.terrainRenderer`: it copies the render list of DH's opaque
  render call (16-byte vertices, `u32` indices; opaque and translucent buffers)
  and draws nothing. Proxies that draw nothing replace DH's far-fade, TAA and
  vanilla-fade renderers. Through the API, DH's apply pass and fog are
  cancelled, its SSAO is turned off, its frustum culling is disabled (the
  shadow pass needs every section) and its translucent pass is deferred. DH's
  LOD block atlas keeps being updated. Everything is put back as soon as a frame
  renders without a pack.
* **Modes** (`DhMode`): `OFF` (DH absent or not rendering, the integration
  unavailable, or the strategy `disabled`), `NATIVE`, or `SYNTHESIZED`
  (strategy `synthesized` with `unified_projection`).
* **Culling.** Each pass culls LOD sections against its own view (camera or
  shadow projection; boxes over the section footprint and the level's height),
  except while DH's earth curvature is on.
* **Unified projection.** `CameraMixin` extends Minecraft's level projection to
  the DH far plane when the frame orchestration requests it, which takes effect
  one frame later. DH counts as rendering, and synthesized LODs are drawn, only
  in frames whose projection really reaches the DH far plane; a hook that never
  applies is reported after three frames.
* **Failures.** If DH's internals do not match, or reading its render list
  fails, DH is given back and its rendering is switched off through its API
  while packs render, and the player is told once.
* **Generic objects** (beacon beams, clouds, API objects). DH's own generic
  rendering is switched off through its API while a pack renders; the Java mod
  replays DH's generic renderer for the frame right after the opaque LODs, with
  every render pass it opens redirected to a pass on the pack's gbuffers and the
  LOD depth, where its pipelines draw with the `dh_generic` program.

`sb-runtime` builds DH-format LOD terrain itself (4-block cells, one buffer per
128×128-block region) for the full DH square with native programs, and outside
the vanilla chunk square with synthesized ones.

## 10. Hosts

* **sb-runtime** (headless) executes a CompiledPack with `ash` on any Vulkan
  1.2 device. In CI it uses Mesa lavapipe. It renders a synthetic scene: a
  procedural voxel terrain with a generated block atlas, plus entities, sky,
  and DH-format LOD terrain (§9.1). It supports all pass types, ping-pong
  flips, shadow maps, computes, images, SSBOs, geometry and tessellation
  stages, blending, alpha test and viewport scales, and writes PNGs. It is the
  reference for the frame order, flips and depth copies the Java mod follows.
* **The Java mod** (`java/`, Fabric, MC 26.3; details in [JAVA_MOD.md](JAVA_MOD.md)) does the following:
  * Loads the native library via JNI and manages packs and options, with its own GUI.
  * Compiles packs off-thread (one `PackSession` per pack, with the caches of
    §7.1) and creates pack render targets as renderpearl `GpuTexture`s.
  * Builds renderpearl `RenderPipeline`s from the packs' SPIR-V, which a hook on Mojang's
    `GlslCompiler` serves in place of compiled GLSL, and swaps them in for vanilla pipelines at
    `RenderPass#setPipeline` inside its own gbuffers and shadow passes. Variants
    for profiles the pack was not compiled for are compiled on demand
    (`compileVariant`, §7).
  * Runs Minecraft's own draw code inside render passes on the pack's gbuffer targets.
  * While a pack renders, meshes chunk sections in an extended 52-byte vertex
    format (the `vanilla_terrain` layout: normals, block ids, mid-texture
    coordinates, tangents, `at_midBlock`), with a full chunk rebuild when it
    switches.
  * Fills `sb_Frame` / `sb_Draw` from game state, evaluating custom uniforms natively.
  * Runs composite-style passes as fullscreen draws.
  * **Raw Vulkan path.** On Minecraft's Vulkan backend, compute programs and
    composite-style programs renderpearl cannot express run as `VkPipeline`s on
    Mojang's `VkDevice` (LWJGL Vulkan): their own descriptor sets, dynamic
    rendering for fullscreen draws, a transient command buffer per use with full
    barriers around it. It adds custom images, SSBOs and raw textures, requests
    the extra device features packs need (geometry/tessellation shaders,
    `independentBlend`, storage image formats, ...) as Mojang optional feature
    sets, adds `VK_IMAGE_USAGE_STORAGE_BIT` to render targets packs bind as
    storage images, and admits a program only if its SPIR-V capabilities,
    descriptors and compute limits fit what the device enabled. Geometry
    programs (`gbuffers_*`, `shadow*`, `dh_*`) cannot be recorded inside
    Minecraft's open passes, so those needing raw Vulkan fall back along their
    chain. On the OpenGL backend there is no raw path: such programs are
    declined with a diagnostic.
  * Takes over DH's terrain renderer and draws its LODs with the pack's `dh_*`
    programs (§9.1).
  * **Sodium** 0.9 replaces Minecraft's terrain renderer but still draws into
    the render pass Minecraft hands it, so its terrain lands in ShaderBridge's
    gbuffers and shadow passes. Its integration mixins
    (`shaderbridge-sodium.mixins.json`) are applied only when Sodium is
    installed, and then all together or not at all: a Mixin config plugin
    checks every Sodium and Minecraft member they hook or call in the class
    files before any class loads. While a pack is active, Sodium meshes a
    36-byte vertex (its 20-byte compact vertex, written by its own encoder, plus
    ShaderBridge's extension) that the `sodium_terrain` profile (§6) decodes,
    and Sodium's terrain pipelines are routed to the terrain programs compiled
    for `sodium_terrain`. If Sodium is installed but the integration cannot
    apply, or Sodium's vertex is not the one ShaderBridge extends, packs are
    refused with a message and Sodium renders untouched.
  * Renders only reversed-Z packs (`ReversedZeroToOne` on a device with [0, 1] clip depth), because
    pack geometry shares Minecraft's depth buffer.

## 11. Verification

* Unit tests in every crate, plus integration tests per crate: differential
  tests of the preprocessor against JCPP (Iris's preprocessor) and glslang, a
  `java.util.Properties` oracle for the properties parser, rule-by-rule
  translation tests (`sb-transform/tests/rules`), std140 offsets checked against
  glslang and `spirv-val`, the draw profiles checked against the host formats
  (`sb-transform/tests/sodium_profile.rs`, `sb-runtime/tests/profile_layouts.rs`)
  and robustness tests on malformed input.
* **Corpus tests**: `shaderbridge validate <packs…>`, and the `corpus` tests of
  `sb-preprocess`, `sb-transform`, `sb-compile`, `sb-pipeline` and others,
  preprocess, translate, compile and validate (`spirv-val`) every program in
  every dimension of real packs: a small corpus by default, the extended corpus
  on request (`SB_CORPUS_DIRS`, `SB_PIPELINE_FULL_CORPUS`). The pass rate is the
  headline metric (the README reports it); the target is 100%. Tests skip when
  no corpus is present.
* **Headless render tests** (`sb-runtime`; real packs with the
  `pipeline-tests` feature, `#[ignore]`d because they take minutes): lavapipe
  renders packs, including native and synthesized DH programs, for a few
  frames. They check that pipelines link, that the Khronos validation layer
  (with synchronization validation) reports nothing, that images are
  non-degenerate, and that forward- and reversed-Z translations of the same
  pack render the same image. A test pack's terrain programs built for
  `sodium_terrain` must render the same image as for `vanilla_terrain`.
* **Java**: JUnit tests of the mod's pure logic (frame sequencing against
  `sb-runtime` traces, attachment and pipeline planning, uniform layouts, DH
  math, SPIR-V reflection and admission, ...); every mixin target, descriptor
  and injection point checked against the bytecode of the Minecraft 26.3 jar
  (and of the Sodium jar for the Sodium mixins); native smoke tests that load
  the Rust library and compile corpus packs through JNI. Everything that drives
  Minecraft or the GPU is compile-verified only; JAVA_MOD.md ("Verification")
  lists both sides.
