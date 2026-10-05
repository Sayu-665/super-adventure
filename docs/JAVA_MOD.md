# ShaderBridge Java mod (Fabric, Minecraft 26.3)

This document describes how the Fabric mod in `java/` renders a compiled shader pack
(`CompiledPack`, `crates/sb-core/src/model.rs`) inside Minecraft 26.3. Minecraft 26.3 renders
through Mojang's renderpearl API, and its default backend is Vulkan. The host conventions the mod
implements are those of [ARCHITECTURE.md](ARCHITECTURE.md) §4–§7 and §9. The headless executor
`crates/sb-runtime` implements the same contract on raw Vulkan and is the reference for:

* the frame order, the flips and the depth copies;
* the shared gbuffer attachments;
* uniform filling;
* the shadow matrices.

> **Verification status.** The development machine has no GPU and no display, so the mod has
> never run in a game. Pure logic (frame sequencing, flips, attachment and write-mask planning,
> the pipeline table, uniform layouts, DH math, SPIR-V reflection and admission, ...) is
> unit-tested. Unit tests also check every mixin target, descriptor, `@At` point, `@Shadow`,
> `@Accessor` and `@Invoker` against the bytecode of the 26.3 game jar. Everything that drives
> Minecraft or the GPU is **compile-verified only**: render passes, texture creation, pipeline
> compilation, raw Vulkan commands and the DH takeover. [Verification](#verification) lists
> both sides.

## 1. Source layout

| Package (`dev.shaderbridge.…`) | What it does |
|---|---|
| `natives`, `model`, `pack`, `config`, `uniforms`, `gui` | Foundation (wave J1): JNI to the Rust library, the `CompiledPack` model, pack repository and compile sessions, settings, uniform providers, screens. |
| `render.pipeline` | Turns pack programs into renderpearl `RenderPipeline`s. SPIR-V is served through the compiler hook, compiled asynchronously and cached. Programs are checked for eligibility and resolved along the fallback chain or routed to the raw path. |
| `render.targets` | `colortexN`/`shadowcolorN` as main/alt pairs, depth copies, shadow maps, noise and custom textures, samplers, and the resolution of pack resources to texture views. |
| `render.mapping` | The table from vanilla pipelines to (geometry program, draw profile) (§4). |
| `render.frame` | Frame orchestration: `RenderBridge` (entry from the mixins), `PackRenderer`, the frame plan and flips, geometry, fullscreen and compute passes, mipmaps, pass copies, DH passes. |
| `render.draw` | Pipeline substitution inside ShaderBridge's passes: vanilla clones, uniform binding, the compiled-pipeline index. |
| `render.shadow` | The shadow pass: a terrain re-render for the shadow camera. |
| `render.raw` | The raw Vulkan path: compute and fullscreen programs on Mojang's `VkDevice`, extra device features, storage usage. |
| `dh` | Distant Horizons takeover (reflection and API, no mixins). |
| `compat.sodium` | Sodium detection (packs are refused while Sodium is loaded). |
| `mixin` | All mixins and accessors (§2). |

## 2. Integration points

Every target below is checked against `minecraft-merged-deobf-26.3.jar` by `MixinTargetsTest`
(`@Inject` selectors and handler parameters), `MixinMembersTest` (`@Shadow`, `@Accessor`,
`@Invoker`, `@ModifyVariable`, `@ModifyArg`, `@WrapOperation`) and `RawMixinsTest`
(`@WrapMethod`, `@ModifyReturnValue`).

Every injection uses `require = 0`. A hook that does not apply in another Minecraft version
leaves vanilla rendering in place, or makes ShaderBridge refuse or disable the pack with a message.
Accessors, invokers and `@Shadow` fields cannot be made optional in Mixin: if one of their targets
disappears, the game fails at class load (see [Limitations](#11-known-limitations)).

### Mixins (`shaderbridge.mixins.json`, client side)

| Mixin | Target and point | Why |
|---|---|---|
| `LevelRendererMixin` | `LevelRenderer.render(...)` HEAD | Starts a pack frame (`RenderBridge.beginLevel`): sync the pack, poll pipelines, resize targets, fill uniforms, clear, run everything before the opaque geometry (setup/begin, shadow, shadowcomp, prepare). |
| | `lambda$addSkyPass$0(GpuBufferSlice, SkyRenderState)` HEAD, cancellable | Runs `SkyRenderer.render` itself so the sky draws into a gbuffers pass. |
| | `lambda$addMainPass$0(GpuBufferSlice, Z, ChunkSectionsToRender, PreparedFrame, Z, Z)` HEAD, cancellable | Replaces the main pass body during a pack frame (`MainPass`, §3). |
| `LevelRendererAccess` | `@Invoker` `prepareTranslucents`, `executeSolid`, `executeClassicTransparency`, `executeOutline`, `executeSeeThrough`, `executeAlwaysOnTop`; `@Accessor` `chunkLayerSampler` | Vanilla's own drawing code runs inside ShaderBridge's passes. |
| `GameRendererMixin` | `useImprovedTransparency()` HEAD | Returns false while a pack is active: packs draw translucents in their gbuffers pass, never through vanilla OIT. |
| | `@ModifyArg` in `renderLevel` at `ProjectionMatrixBuffer.getBuffer(Matrix4f)` | Captures the level projection (view bobbing and nausea applied) for `gbufferProjection`. |
| `SkyRendererMixin` | `@WrapOperation` of `CommandEncoder.createRenderPass(Supplier, GpuTextureView, Optional, GpuTextureView, OptionalDouble)` in `SkyRenderer.render` | Hands the sky a gbuffers pass instead of the main target. |
| `FrontendRenderPassMixin` | `@ModifyVariable` `setPipeline` HEAD, `@Inject` `setPipeline` RETURN | Pipeline substitution in ShaderBridge passes, then binds `sb_Frame`, `sb_Draw` and the pack samplers (§4). |
| | `@Inject` `setUniform(String, GpuTextureView, GpuSampler)` RETURN | When vanilla binds another `Sampler0` after the pipeline (as `PreparedRenderType` does), rebinds what depends on the albedo: `sb_Draw` with `gtextureSize`/`atlasSize`, and pack samplers that sample the atlas. |
| | `@Inject` `close` HEAD; `@Shadow` `uniforms`, `backend`, `isClosed`, `pushedDebugGroups` | If vanilla code failed between `pushDebugGroup` and `popDebugGroup` in a ShaderBridge pass, pops the open groups. The pass then still ends and the failure reaches the frame's fail-soft handling. Without this, Mojang's single command encoder would stay "in a render pass" and the next pass (the GUI) would crash the game. |
| `RenderSystemMixin` | `RenderSystem.getCompiledPipelineNullable(RenderPipeline)` RETURN | Records compiled pipeline → `RenderPipeline` (`CompiledPipelineIndex`), so draws can be routed by the vanilla pipeline's location. |
| `GlslCompilerMixin` | `GlslCompiler.compileToSpv(String, String, ShaderType, ShaderDefines, ShaderSource)` HEAD, cancellable | Serves ShaderBridge's precompiled SPIR-V (`shaderbridge:spv/<n>` ids) as a `SPIRVModule` instead of running shaderc. |
| `CameraMixin` | `@ModifyArg` index 1 of `Camera.setupPerspective(FFFFF)V` in `Camera.update` | Extends the level projection's far plane to the DH far plane for synthesized DH LODs (§8). |
| `FrontendGpuDeviceAccess` | `@Accessor` `FrontendGpuDevice.backend` | Reaches `VulkanDevice` for the raw path. |
| `VulkanDeviceAccess` | `@Accessor` `VulkanDevice.enabledFeatures` | Which device features were actually enabled. |
| `VulkanFeatureSetsMixin` | `@ModifyReturnValue` `VulkanFeatureSets.optionalFeatureSets()` | Requests extra features as optional feature sets (§7). |
| `VulkanDeviceMixin` | `@WrapMethod` `VulkanDevice.createTexture(String, int, GpuFormat, int, int, int, int)` | Marks creation of render targets that packs bind as storage images. |
| `VulkanGpuTextureMixin` | `@ModifyArg` of `VkImageCreateInfo.usage(I)` in the `VulkanGpuTexture` constructor | Adds `VK_IMAGE_USAGE_STORAGE_BIT` to those targets. |

### Other hooks

* **Fabric events** (`ShaderBridgeClient`):
  * `JOIN` compiles the selected pack.
  * `DISCONNECT` releases the render resources. Fabric may fire it on a Netty thread, so the release is queued with `minecraft.execute`.
  * `CLIENT_STOPPING` releases the render resources and the native session.
  * `END_CLIENT_TICK` runs key bindings and game-state ticks.
  * A client resource reload listener rebuilds the renderer at the next frame.
* **Distant Horizons** is hooked by reflection and through its public API, without mixins (§8).
* **Access widener** (`shaderbridge.classtweaker`): `Biome.climateSettings` (rainfall), `BossHealthOverlay.events` (`bossBattle`), `CloudRenderer.texture` (`cloudTime`).

## 3. Pack lifecycle and frame sequence

### Activation

`RenderBridge.sync()` runs at the start of every `LevelRenderer.render`. It keeps a `PackRenderer`
(owning a `PackResources`) for the active pack and the current dimension folder
(`DimensionSelector`). The renderer is rebuilt when any of these changes:

* the pack (new, recompiled or unloaded);
* the dimension folder;
* the resources (reloaded).

A pack is **refused**, with a message to the player and vanilla rendering, when one of these holds:

* Sodium is loaded (§9).
* The pack was not compiled for `REVERSED_ZERO_TO_ONE` depth, or the device does not clip depth
  to [0, 1] (`DepthSupport`). Pack geometry shares Minecraft's main depth buffer. Minecraft 26.3
  clears that buffer to 0 and tests it with `GEQUAL`, so only reversed-Z packs can be drawn into it.
* Its render resources cannot be created.

A frame renders with the pack only once every composite-style pipeline and compute program is
compiled. Until then Minecraft renders vanilla. Pipelines compile on a background executor;
`PackPipelineCache.poll()` finishes them on the render thread.

### One frame

The order is the one `sb-runtime`'s `record_frame` uses. `FramePlan` transcribes it and
`FrameSequencerTraceTest` compares it with reference traces of the executor. The steps are:

1. **`LevelRenderer.render` HEAD** (`PackRenderer.beginFrame`). Outside any pass:
   1. Capture the game state and update `FrameState`.
   2. Upload `sb_Frame`, then write and upload the `sb_Draw` blocks of every draw kind seen so far.
   3. Clear targets per `ColorTarget.clear`, and clear every target on its first frame.
   4. Start the DH frame.
   5. Run `setup` (first frame only), `begin`, the **shadow pass** (§6) with its shadowcolor
      mipmaps, `shadowcomp` and `prepare`. Each pass starts by adopting its `flip_state`, and the
      flips follow `flips_after`.
2. **Sky pass**:
   1. `SkyRenderer.render` opens its pass through the wrapped `createRenderPass`. ShaderBridge
      returns a gbuffers pass instead.
   2. The sky pipelines are substituted with `gbuffers_skybasic` and `gbuffers_skytextured`.
3. **Main pass** (`MainPass`, replacing `lambda$addMainPass$0`):
   1. Terrain fog, the chunk sampler and `prepareTranslucents`. DH hands over its LODs here.
   2. The opaque DH LODs (`dh_terrain`), then the copy of the LOD depth to `dhDepthTex1` and
      `dhDepthTex0`.
   3. A **gbuffers pass**: the pack's `gbuffer_attachments` in their current textures plus
      Minecraft's main depth. In it, vanilla `executeSolid` draws opaque terrain and opaque
      features.
   4. The `depthtex2` and `depthtex1` copies of the main depth, then `deferred`.
   5. `dh_water`, then the `dhDepthTex0` copy.
   6. Another gbuffers pass, in which vanilla `executeClassicTransparency` draws the translucent
      features and terrain, clouds, weather and the world border.
   7. `composite`, then `final` into Minecraft's main color target. Without a `final` program,
      `colortex0` is blitted there. Then the `end_of_frame_copies` (alt → main).
4. After the main pass, vanilla `executeOutline`, `executeSeeThrough` and `executeAlwaysOnTop`
   run. They run whether or not the pack frame succeeded.

Composite-style programs are fullscreen draws. Each runs in its own pass over its output targets
(the textures `FlipState.write` selects), as six vertices of the `fullscreen` profile, and reads
its inputs per `BindingUse.use_alt`. Targets a program lists in `mipmap` get their mip levels
generated just before it runs (`MipGenerator`). Computes are dispatched through renderpearl or the
raw path (`ComputeDispatcher`). Dispatches are sized from `workGroups`/`workGroupsRender`;
shadow-pass computes are sized over the shadow map.

### Fail-soft

The following abandon the frame, release the pack's render resources and show the error, after
which Minecraft renders vanilla until another pack (or a recompile) is activated:

* an exception anywhere in a pack frame, including inside a substituted vanilla draw;
* a frame that never reached its main pass (a missing hook);
* an inactive SPIR-V hook.

Diagnostics that do not stop the pack go to the log through `PipelineDiagnostics`, which
deduplicates them. Examples: programs that fall back or are skipped, unsupported features,
missing resources.

## 4. Pipeline substitution

### Pack pipelines

`PackPipelineFactory` builds one `RenderPipeline` per (program, draw profile, draw shape,
attachment layout):

* **Location**: `shaderbridge:<packhash>/<folder>/<program>/<profile>/<shape>/<layout>`.
* **Shaders**: ids `shaderbridge:spv/<n>`, served by `GlslCompilerMixin` from `SpirvModules`
  (memAlloc'd copies, because `SPIRVModule` rewrites and frees them). `PackShaderSource` returns a
  placeholder for these ids.
* **Bind group layout**: every descriptor the SPIR-V declares, named as SPIRV-Cross names them.
* **Vertex bindings**: from the draw profile (`ProfileVertexFormats`). Topology, polygon mode and
  depth state come from the vanilla pipeline it replaces. The depth state is unchanged in reversed
  mode. Culling comes from `program.cull`, else from the vanilla pipeline; the shadow pass never
  culls.
* **Color targets**: the pass's attachment layout. A slot is written when the program maps an
  output to it with the matching numeric class; other slots get write mask 0. There is one shared
  blend function: Mojang's builder accepts only one.

A program is **ineligible** for renderpearl, and goes to the raw path or falls back along its
`GeometryChain`, in any of these cases:

* it is marked `requires_raw_vulkan`;
* it is a compute program;
* it has stages other than vertex + fragment;
* it uses more than 128 bytes of push constants;
* it has conflicting or more than 32 descriptors, or descriptor kinds renderpearl cannot bind;
* it fails the stage-interface checks of Mojang's `PipelineBuilder`;
* it declares SPIR-V capabilities the device did not enable. This is checked against the enabled
  features (`EnabledFeatures`, `SpirvCapabilities`), the same check the raw path uses;
* it needs per-attachment write masks or blending without `independentBlend`.

### Inside ShaderBridge's passes

`ActivePasses` marks the one ShaderBridge pass vanilla code is drawing into. For every
`setPipeline` there (`GeometryPasses.Draws`):

1. Look up the vanilla `RenderPipeline` (`CompiledPipelineIndex`).
2. Route its location through the table below (`PipelineRouter` keeps a mapping only if the
   profile's vertex bindings match).
3. Resolve the mapped slot's program for that profile along the fallback chain
   (`DrawSubstitution`, `ProgramResolver.geometry`). Variants for profiles the pack was not
   compiled for are compiled on demand (`OnDemandVariants`, `session.compileVariant`, on a
   background thread).
4. If the pack pipeline is ready, bind it. `sb_Frame`, the `sb_Draw` slice of the draw's
   `DrawKey` and every pack sampler (main or alt per `ColorReads`) are bound after it. Host
   samplers stay with the host draw path, which binds them.
5. Otherwise draw the vanilla pipeline adapted to the pass (`VanillaClones`):
   * In gbuffers passes, the vanilla color output goes to `fallback_tex` in slot 0 when the device
     has `independentBlend` or the pass has one attachment; otherwise only depth is written.
   * In the shadow pass, nothing is written.

   Clones are kept for the process, so Mojang's pipeline cache never accumulates copies. A
   pipeline that cannot be adapted is bound as is only if it fits the pass. Otherwise the draw
   fails with a clear message, and the frame fails soft.

### Vanilla pipeline table (`VanillaPipelineTable`)

Locations are `minecraft:pipeline/<name>`. A unit test checks the table against every
`RenderPipelines` field of the 26.3 jar, so a new vanilla pipeline is noticed. The table follows
Iris 26.3 (`IrisPipelines`) where Iris maps a pipeline to one program.

| Vanilla pipelines | gbuffers program | Shadow program | Profile |
|---|---|---|---|
| `solid_terrain(_multidraw)` | `terrain_solid` | `shadow_solid` | `vanilla_terrain_section` / `vanilla_terrain_basic` (MDI) |
| `cutout_terrain(_multidraw)` | `terrain_cutout` | `shadow_cutout` | same |
| `translucent_terrain(_multidraw)` | `water` | `shadow_water` | same |
| `solid_block`, `cutout_block` | `terrain_solid`, `terrain_cutout` | `shadow_cutout` | `vanilla_block` |
| `translucent_block` | `block` | `shadow_water` | `vanilla_block` |
| `crumbling` | `damagedblock` | none | `vanilla_block` |
| `beacon_beam_opaque/translucent` | `beaconbeam` | `shadow_entities` | `vanilla_block` |
| `end_portal`, `end_gateway` | `block` | `shadow_block` | `vanilla_position` |
| `entity_solid(_offset_forward)`, `entity_cutout(_cull/_z_offset/_dissolve)`, `armor_cutout_no_cull(_glint)`, `armor_decal_cutout_no_cull`, `armor_translucent`, `energy_swirl`, `end_crystal_beam`, `item_cutout`, `entity_solid_glint` | `entities` | `shadow_entities` | `vanilla_entity` |
| `entity_translucent(_cull)`, `item_translucent(_glint)`, `breeze_wind`, `banner_pattern` | `entities_translucent` | `shadow_entities` | `vanilla_entity` |
| `entity_shadow` | `entities_translucent` | none | `vanilla_entity` |
| `eyes`, `entity_translucent_emissive` | `spidereyes` | `shadow_entities` | `vanilla_entity` |
| `glint` | `armor_glint` | none | `vanilla_position_tex` |
| `lightning`, `dragon_rays` | `lightning` | `shadow_lightning` | `vanilla_position_color` |
| `opaque_particle` / `translucent_particle` | `particles` / `particles_translucent` | `shadow` | `vanilla_particle` |
| `weather` | `weather` | none | `vanilla_particle` |
| `sky`, `stars` / `sunrise_sunset` | `skybasic` | none | `vanilla_position` / `vanilla_position_color` |
| `celestial` / `end_sky` | `skytextured` | none | `vanilla_position_tex` / `vanilla_position_tex_color` |
| `clouds`, `flat_clouds` | `clouds` | none | `vanilla_clouds` |
| `lines*`, `secondary_block_outline` | `line` | none | `vanilla_lines` |
| `leash` | `basic` | `shadow` | `vanilla_position_color_lightmap` |
| `text`, `text_grayscale`, `text(_grayscale)_polygon_offset` | `entities_translucent` | `shadow_entities` | `vanilla_text` |
| `text(_grayscale)_see_through` | `entities_translucent` | none | `vanilla_position_tex_color` |
| `world_border` | `textured` | none | `vanilla_position_tex` |
| GUI, debug, outline, lightmap, sprite animation, blits, `water_mask`, `oit_*`, `wireframe*` | vanilla | | |

Program names omit the `gbuffers_` prefix. In 26.3 the `*_glint` pipelines draw the model and its
glint in one pass, so they use the model's program; only `glint` uses `gbuffers_armor_glint`.

## 5. Render targets and textures

`PackTargets` (with `TargetPlanner`) holds the pack's render targets:

* **`colortexN`, `shadowcolorN`**:
  * Each is a main/alt pair of `GpuTexture`s (`ColorPair`) with usage attachment, sampled, copy
    source and copy destination.
  * Sizes follow `TargetSize`. Shadow resolution is clamped to 16..8192 and to the device limit
    per format.
  * Formats are the renderable form of the pack format.
  * A full mip chain is allocated for targets a program requests mipmaps of.
* **Clears** follow `ColorTarget.clear`/`clear_color`, with defaults as in `sb-runtime`. Every
  target is cleared on its first frame after creation or resize.
* **Depth**:
  * `depthtex0` is Minecraft's main depth.
  * `depthtex1` and `depthtex2` are copies taken after the opaque geometry.
  * `shadowtex0` is the shadow pass's depth attachment; `shadowtex1` is its copy taken before
    translucent casters. Both are `D32_FLOAT`.
* **Textures**: noise (Iris layout), white/black/flat-normal/no-specular defaults, and custom
  textures from the pack (directory or zip) or resource packs (`PackTextures`). Raw 1D/3D textures
  are uploaded only on the raw path.
* **Every attachment slot has a texture.** A slot whose target is missing, repeated or of another
  size gets a throwaway sink texture of the slot's format and pass size (`SinkTextures`). A null
  attachment would need `VK_FORMAT_UNDEFINED` in every pipeline of the pass, since
  `dynamicRenderingUnusedAttachments` is not enabled.
* **Feedback reads**: ShaderBridge's geometry passes attach all `gbuffer_attachments` (Iris
  attaches only a program's own draw buffers). Before each geometry pass, the attached targets
  that the pass's programs sample are copied (`FeedbackReads`, `PassCopies`), and the programs
  sample the copy, as `sb-runtime` does. This covers `depthtex0` and `colortexN` in gbuffers
  passes, and `shadowtex0` and `shadowcolorN` in the shadow pass.
* **Mipmaps**: `MipGenerator` fills mip levels with Minecraft's screen blit, cloned per format
  (`BlitPipelines`). It runs after the shadow pass for `shadowcolorNMipmap` and before a
  composite program for its `mipmap` targets. Filtering is linear for formats every Vulkan device
  can filter, nearest otherwise. Integer targets get no mipmaps and are sampled at their base level.

`TextureResolver` resolves each `ResourceRef` to a view and sampler (`SamplerChoice`). A sampler
the pack did not name (GL texture unit 0) is the draw's albedo in geometry programs, and
`colortex0` in its current texture elsewhere.

## 6. Uniforms and the shadow pass

### Uniforms

* `sb_Frame` is filled once per frame from `FrameState`, using the `uniforms` providers. Custom
  uniforms are evaluated natively.
* `sb_Draw` has one block per **draw kind** (`DrawKey`). A kind is identified by:
  * the pipeline;
  * `renderStage` (Iris phase numbers);
  * shadow or not, which selects the model-view and projection;
  * `alphaTestRef`;
  * `blendFunc`;
  * the albedo size (`gtextureSize`, and `atlasSize` when the albedo is one of Minecraft's texture
    atlases).

  Buffers may not be written inside a pass, so a kind seen for the first time uses the frame's
  default block until the next frame.
* Host blocks (`Globals`, `Projection`, `Fog`, `DynamicTransforms`, `TerrainUniform`,
  `ChunkSection`, ...) are bound by the vanilla draw path itself.

### Shadow pass

1. `ShadowRenderer` re-prepares the camera-visible chunk sections for the shadow camera
   (`ShadowSections.CAMERA_VISIBLE`, MDI when the level uses it).
2. It grows Minecraft's shared quad index buffer to the requested count. Vanilla grows it only
   later in the frame, in `prepareTranslucents`.
3. It draws opaque terrain, then the opaque DH LODs (`dh_shadow`), the `shadowtex0` →
   `shadowtex1` copy, and translucent terrain. Each group is drawn in a pass on
   `shadow_attachments` + `shadowtex0`.

Vanilla pipelines are substituted with the shadow programs; pipelines without one draw nothing.
The shadow matrices come from `ShadowMatrices`, as in `sb-runtime`.

## 7. Raw Vulkan path (`render.raw`)

The raw path is available on Minecraft's Vulkan backend only. On OpenGL every program that needs
it is declined with a message.

**What runs there.** Compute programs (setup, begin, prepare, deferred, composite, final and
shadowcomp computes, and geometry-pass computes) and composite-style programs that renderpearl
cannot express run as `VkPipeline`s on Mojang's `VkDevice`:

* They use their own descriptor set layouts and pools, and dynamic rendering for fullscreen draws.
* Each use is recorded in a transient command buffer with full memory barriers before and after.
* Images stay in `GENERAL`.

**What it adds.**

* Custom images (`image.*`, 1D/2D/3D, screen-relative).
* SSBOs (`bufferObject.*`, with initial data).
* Raw textures, converted to the target format.
* Stand-ins for descriptors that are missing, each with a diagnostic.

**Device features.** These are requested as Mojang optional feature sets and enabled only where
supported: `independentBlend`, geometry and tessellation shaders, stores and atomics in vertex
and fragment stages, storage image formats, cube arrays, gather extended, sample-rate shading,
clip/cull distance, 64-bit and 16-bit types, and dynamic indexing. Admission (`RawAdmission`)
checks every program's SPIR-V capabilities, descriptor plan and compute limits against what was
actually enabled.

**Storage images.** Render targets a pack binds as storage images get
`VK_IMAGE_USAGE_STORAGE_BIT` (`StorageUsage`, `VulkanDeviceMixin`, `VulkanGpuTextureMixin`), for
formats that support it.

**Never on the raw path.** Geometry programs (`gbuffers_*`, `shadow*`, `dh_*`) cannot be recorded
inside Minecraft's open passes. Geometry and tessellation programs are therefore rejected and fall
back along their chain.

## 8. Distant Horizons (`dh`)

The DH integration is written for DH 3.3.x (verified against 3.3.4 sources and jar). It works by
reflection and through the DH API, and fails soft.

**Takeover.**

* A `java.lang.reflect.Proxy` replaces `LodRenderer.INSTANCE.terrainRenderer`. It copies the
  opaque render call's buffer list (16-byte vertices, `u32` indices) and draws nothing.
* The far-fade and TAA renderers, and the vanilla-fade singleton, are replaced with proxies that
  draw nothing.
* Through the API, DH's apply pass and fog are cancelled, its SSAO is turned off, its frustum
  culling is disabled (the shadow pass needs every section) and its translucent pass is deferred.
* Everything is restored when no pack renders.
* If DH's internals do not match, DH rendering is switched off through the API while a pack
  renders, and the player is told once.

**Drawing** (`DistantPasses`):

* `dh_terrain`, `dh_water` and `dh_shadow` are drawn with the slot's draw profile, into passes on
  the shared gbuffers (or shadow) attachments.
* The host blocks `vertSharedUniformBlock` and `vertUniqueUniformBlock` are written camera
  relative: `uModelOffset = minCorner - camera` in double, `uCameraPos = 0`.
* Opaque LODs are drawn before vanilla opaque terrain, and `dh_water` before vanilla translucents,
  as for Iris.

**Modes** (`DhMode`):

* **NATIVE**: the pack's own `dh_*` programs, with a separate `D32` LOD depth.
  * `dhDepthTex1` is copied after `dh_terrain`.
  * `dhDepthTex0` is copied after `dh_terrain` and again after `dh_water`, so it always holds the
    LODs drawn so far, like the live texture Iris binds.
  * LODs are also drawn in the vanilla area.
  * `dhProjection` spans DH's near plane to `dhFarPlane = (lodChunks*16 + 512)*sqrt(2)`.
* **SYNTHESIZED**: programs generated from `gbuffers_*`.
  * Minecraft's depth is shared, with one unified projection (`CameraMixin` extends the far plane).
  * Sections entirely inside the vanilla area are skipped.
* **OFF**: no LODs are drawn.

## 9. Sodium (`compat.sodium`)

Sodium replaces Minecraft's terrain renderer, so ShaderBridge's terrain hooks never see terrain.
Its 20-byte mesh also lacks attributes the `sodium_terrain` profile reads. While Sodium is loaded,
ShaderBridge does not activate packs and shows: "Sodium <version> is installed … remove Sodium to
use shader packs". Minecraft and Sodium render as usual.

## 10. Configuration

`config/shaderbridge.json` (`ShaderBridgeConfig`, edited through the GUI where exposed):

| Key | Meaning |
|---|---|
| `enabled` | Shader packs on/off. |
| `selectedPack` | File name in `shaderpacks/`. |
| `depthMode` | `auto` (default: reversed-Z [0,1] when the device clips depth to [0,1], else GL [-1,1]), `reversed`, `forward`. **Only reversed-Z packs on a [0,1] device render in game.** Other modes exist to compare translations with `sb-runtime`; packs compiled with them are refused with a message. |
| `validate` | Run spirv-val while compiling. |
| `debugDumpGlsl` | Write translated GLSL to `shaderbridge/debug/`. |
| `compileThreads` | Native compile threads, 0 = automatic. |
| `showDiagnosticsInChat` | Compile reports in chat instead of toasts. |

Per-pack options are stored by `PackOptionValues` and edited in `PackOptionsScreen`.

## 11. Known limitations

### Not verified in a game

No GPU and no display were available, so the mod has never run in Minecraft. Mixin injection at
runtime, every GPU call and the actual images are unverified (see [Verification](#verification)).

### Rendering gaps

* **Terrain vertices.** Minecraft 26.3's chunk meshes (`BLOCK` format) carry no normals, block
  ids, mid-texture coordinates, tangents or `at_midBlock`. The `vanilla_terrain_basic` and
  `vanilla_terrain_section` profiles therefore feed:
  * normal (0, 1, 0);
  * `mc_Entity` = -1;
  * `mc_midTexCoord` = the vertex's own UV.

  Water detection by block id, waving foliage, material ids and normal-based terrain lighting do
  not work. Iris extends the chunk vertex format; ShaderBridge does not yet. A diagnostic says so
  for every pack with terrain programs.
* **Block entities.** They draw with the entity programs. Iris tells them apart by rendering phase
  and uses `gbuffers_block`; Minecraft 26.3 batches both into the same feature draws.
* **Entity shadows.** Entities, block entities and the player cast no shadows. The prepared
  feature frame is baked for the camera and can be used once. Shadow culling uses the
  camera-visible sections, not a shadow frustum (`ShadowSections` is the hook for one).
* **Hand.** Hand rendering stays vanilla and is drawn after `final`.
* **Per-draw ids.** `entityId`, `blockEntityId` and `heldItemId` are -1, and `centerDepthSmooth`
  is not sampled from depth.
* **Composite passes.** Viewport scale and offset (`scale.<program>`) are not applied
  (diagnostic). Gbuffer attachments with a scaled size are not drawn by geometry: they get sinks.
* **New draw kinds.** `sb_Draw` values of a draw kind seen for the first time take effect one
  frame late.
* **Without `independentBlend`.** Vanilla fallback draws in a multi-attachment gbuffers pass write
  depth only, and pack programs that leave attachments untouched fall back or are skipped.
* **Feedback copies.** Which programs draw in a pass is unknown in advance, so the targets sampled
  by any geometry program of the pass kind are copied before every such pass. Programs that
  sample a target they also write read its contents from before the pass.
* **Fabric API.** Fabric API's world-render `END_MAIN` event (an injection at
  `lambda$addMainPass$0` RETURN) does not fire while a pack frame replaces the main pass.
* **Vanilla mipmap blit.** Mip levels are generated with a 2×2 box (bilinear) downsample,
  point-sampled for formats without guaranteed linear filtering (32-bit floats, 16-bit
  normalized).

### Integrations

* **Sodium:** packs are refused while it is loaded.
* **Distant Horizons:**
  * DH generic objects (`dh_generic`: beacons, clouds, API objects) are not drawn while a pack
    renders.
  * Shadow LODs use the previous frame's list.
  * Synthesized LODs overlap vanilla terrain in sections crossing the vanilla edge.
  * `reduceOverdrawWithFastMovement` is not reproduced in `dhNearPlane`.
  * Frustum culling is off while earth curvature is enabled.
  * The unified projection takes effect one frame after it is requested.
* **Raw path:**
  * GL backend: no raw path.
  * Raw fullscreen draws ignore viewport scale and generate no mipmaps.
  * Cube, array and multisampled images, runtime arrays and more than 4 descriptor sets are
    rejected.
  * Synchronization is a full barrier around every use.

### Robustness

* **Depth modes.** Only `REVERSED_ZERO_TO_ONE` on devices with [0, 1] clip depth renders. On
  OpenGL without `GL_ARB_clip_control` (macOS) packs are refused.
* **Hard mixin targets.** Accessors, invokers and `@Shadow` fields (`LevelRendererAccess`,
  `FrontendGpuDeviceAccess`, `VulkanDeviceAccess`, the `@Shadow` fields of
  `FrontendRenderPassMixin`) are hard requirements of Mixin. If a future Minecraft version renames
  them, the game fails at start-up rather than disabling shaders. All injections are optional.
* **Unadaptable pipelines.** A vanilla pipeline drawn in a ShaderBridge pass that neither came
  from Minecraft's pipeline cache nor fits the pass (another mod's custom pipeline) fails the
  frame, and the pack is disabled with a message.
* **Diagnostics** are logged and summarized in toasts or chat. There is no in-game list.

## Verification

### Unit-tested (JUnit, `./gradlew build`)

* **Frame sequencing.** `FramePlanTest`, `FlipStateTest`, `FrameSequencerTest` and
  `FrameSequencerTraceTest` compare against reference traces of `sb-runtime` for real compiled
  packs: Complementary Reimagined, Photon, Rethinking Voxels, Glimmer and the Tutorial pack.
* **Attachments, reads and draw state.**
  * `PassAttachmentsTest`: sinks in every missing slot.
  * `ColorReadsTest`, `ImageChoiceTest`: main and alt selection, including unnamed samplers.
  * `DispatchSizeTest`.
  * `DrawKeyTest`: albedo sizes.
  * `FeedbackReadsTest`: copies of attached targets; `DepthSupportTest`: the depth-mode gate.
  * `MipGeneratorTest`.
* **Pipelines.**
  * `PackPipelineFactoryTest`, including SPIR-V capability rejection.
  * `AttachmentPlannerTest`, `PipelinePartsTest`, `SpirvReflectorTest`, `ProgramResolverTest`,
    `PackPipelineCacheTest` (with fake devices), `GeometryChainTest`, `ProfileVertexFormatsTest`,
    `OnDemandVariantsTest`.
* **Substitution.** `VanillaPipelineTableTest` (against every 26.3 pipeline), `DrawSubstitutionTest`,
  `VanillaClonesTest` (process-wide clones, the fit check, blit clones), `UniformBinderTest`
  (albedo rebinding), `ActivePassesTest`.
* **Targets.** `TargetPlannerTest`, `PackTargetsTest` (fake device, including level views),
  `PackTexturesTest`, `SamplerChoiceTest` (integer targets at their base level),
  `TextureDataTest`, `PackFilesTest`.
* **Shadow.** `ShadowPlanTest`.
* **Raw path.** `DescriptorPlannerTest`, `RawAdmissionTest`, `SpirvCapabilitiesTest`,
  `RawTextureDataTest`, `CommandPlanTest`, `ResourceSizesTest`, `ComputeLimitsTest`,
  `ColorStatesTest`, `StorageUsageTest`, `StorageTargetsTest`, `RawFeatureTest`,
  `SpirvBlockSizesTest`.
* **Distant Horizons.** `DhHostBlocksTest`, `DhPlanesTest`, `DhModeTest`, `LodSelectionTest`,
  `LodUniformsTest`, `DhDepthTargetsTest`, `RendererSwapTest`, `DhInternalsTest` (against the DH
  3.3.4 jar), `CameraFarPlaneTest`, `DistantBindingsTest`.
* **Mixin targets.** `MixinTargetsTest`, `MixinMembersTest`, `RawMixinsTest`, `GameTargetsTest`.
* **Native library.** The native smoke tests (`nativeTest`) load the Rust library and compile
  corpus packs.

### Compile-verified only (no GPU, no game)

* **Frame orchestration.** `RenderBridge`, `PackRenderer`, `MainPass`, `GeometryPasses`,
  `FullscreenPasses`, `ComputeDispatcher`, `DistantPasses`/`DistantFrame`, `MipGenerator.generate`,
  `PassCopies`, `SinkTextures`, `AtlasTextures`, `MinecraftHost`.
* **Shadow pass.** `ShadowRenderer`, `ShadowSections`.
* **Pipeline compilation on a real `GpuDevice`.** `PackPipelineCache` and `ProgramResolver` with
  real compiles, and `SessionVariantCompiler`.
* **Every Vulkan call of the raw path.** `VulkanContext`, `VmaImage`/`VmaBuffer`,
  `PipelineObjects`, `GraphicsPipelines`, `DescriptorAllocator`, `CommandRecorder`,
  `FullscreenRendering`, `RawResources`, `RawBindings`, `RawShaderProgram`, `VulkanRawPath`.
* **The DH takeover in a running game.** `DistantHorizons`, `DhApiControl`.
* **Every mixin at runtime.** The mixins are only checked statically against the jar.
* **`ShaderBridgeClient` lifecycle hooks.**
