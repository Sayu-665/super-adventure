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
| `render.chunk` | The extended chunk vertex format (§4): chunk sections meshed with normals, block ids, mid-texture coordinates, tangents and `at_midBlock` while a pack renders, the pack's block id table, the extended clones of the terrain pipelines, and the format switch with its full chunk rebuild. |
| `render.frame` | Frame orchestration: `RenderBridge` (entry from the mixins), `PackRenderer`, the frame plan and flips, geometry, fullscreen and compute passes, mipmaps, pass copies, DH passes. |
| `render.draw` | Pipeline substitution inside ShaderBridge's passes: vanilla clones, uniform binding, the compiled-pipeline index. |
| `render.shadow` | The shadow pass: a terrain re-render for the shadow camera. |
| `render.raw` | The raw Vulkan path: compute and fullscreen programs on Mojang's `VkDevice`, extra device features, storage usage. |
| `dh` | Distant Horizons takeover (reflection and API, no mixins). |
| `compat.sodium` | The Sodium 0.9 integration (§9): its all-or-nothing target check, the extended Sodium terrain vertex and its encoder, the routing of Sodium's terrain pipelines, and Sodium's sections for the shadow pass. Its optional mixins are in `mixin.sodium`. |
| `mixin` | All mixins and accessors (§2). |

## 2. Integration points

Every target below is checked against `minecraft-merged-deobf-26.3.jar` by `MixinTargetsTest`
(`@Inject` selectors and handler parameters), `MixinMembersTest` (`@Shadow`, `@Accessor`,
`@Invoker`, `@ModifyVariable`, `@ModifyArg`, `@WrapOperation`), `RawMixinsTest`
(`@WrapMethod`, `@ModifyReturnValue`) and `RedirectMixinsTest` (`@Redirect` of a constructor).

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
| `LevelExtractorMixin` | `LevelExtractor.extract(DeltaTracker, Camera, F)` HEAD | Switches the chunk mesh format when a pack starts or stops rendering, or its block ids change, with a full chunk rebuild (§4, terrain vertices). |
| `ChunkSectionLayerMixin` | `@ModifyReturnValue` `ChunkSectionLayer.pipeline(Z)` and `vertexFormat()` | While the extended format is active: the extended terrain pipeline clones and vertex format. Minecraft derives the section builders' format, the vertex size of the section buffer heaps and every draw's base vertex from these. |
| `ChunkSectionsToRenderMixin` | `@ModifyArg` indices 5 and 6 of `renderLayers(...)` in `renderGroup` and `renderOit` | The wireframe and order-independent-transparency pipeline overrides draw the extended meshes with their extended clones. |
| `SectionCompilerMixin` | `@Redirect` of `new BufferBuilder(ByteBufferBuilder, PrimitiveTopology, VertexFormat)` in `getOrBeginLayer`; `@WrapOperation` of `RenderSectionRegion.getBlockState(BlockPos)` and `FluidRenderer.tesselate(...)` in `compile` | Layers in the extended format get ShaderBridge's builder; the block (and fluid) being meshed is recorded for every quad's `mc_Entity` and `at_midBlock`. |
| `FrontendGpuDeviceAccess` | `@Accessor` `FrontendGpuDevice.backend` | Reaches `VulkanDevice` for the raw path. |
| `VulkanDeviceAccess` | `@Accessor` `VulkanDevice.enabledFeatures` | Which device features were actually enabled. |
| `VulkanFeatureSetsMixin` | `@ModifyReturnValue` `VulkanFeatureSets.optionalFeatureSets()` | Requests extra features as optional feature sets (§7). |
| `VulkanDeviceMixin` | `@WrapMethod` `VulkanDevice.createTexture(String, int, GpuFormat, int, int, int, int)` | Marks creation of render targets that packs bind as storage images. |
| `VulkanGpuTextureMixin` | `@ModifyArg` of `VkImageCreateInfo.usage(I)` in the `VulkanGpuTexture` constructor | Adds `VK_IMAGE_USAGE_STORAGE_BIT` to those targets. |

### Sodium mixins (`shaderbridge-sodium.mixins.json`, client side)

Optional (`"required": false`, `defaultRequire` 0) and applied all together or not at all by
`SodiumMixinPlugin` (§9). Their targets are checked against the Sodium 0.9.3-alpha.1 jar the build
compiles against (Modrinth Maven, `compileOnly`; nothing of Sodium is bundled) and against Minecraft
26.3 by `SodiumTargetsTest` and `SodiumMixinsTest`; they were also checked with `javap` against
Sodium 0.9.2 for 26.3.

| Mixin | Target and point | Why |
|---|---|---|
| `ChunkMeshFormatsMixin` | `ChunkMeshFormats.getCurrent()` HEAD, cancellable | While a pack is active, Sodium meshes the extended terrain vertex. |
| `SodiumWorldRendererMixin` | `SodiumWorldRenderer.initRenderer()` HEAD; `setupTerrain(...)` after its call to `processChunkEvents()` | Chooses the vertex when Sodium creates its section manager; reloads Sodium's renderer when the active pack (or its block ids) needs another one. |
| `ChunkBuilderMeshingTaskMixin` | `@WrapOperation` of `BlockRenderer.renderModel(...)` and `FluidRenderer.render(...)` in `ChunkBuilderMeshingTask.execute(...)` | The block being meshed on the worker thread (id, position, emission, fluid flag) for the extension attributes. |
| `TranslucentGeometryCollectorMixin` | `TranslucentGeometryCollector.appendQuad(...)` HEAD | Tags translucent quads before the sorter copies them, so quads it splits and encodes later keep their block's data. |
| `ChunkVertexMixin` | `ChunkVertexEncoder.Vertex`: tag fields (`VertexTags`); `copyVertexTo(Vertex, Vertex)` TAIL | Carries the tags through the sorter's vertex copies. |
| `DefaultChunkRendererMixin` | `DefaultChunkRenderer.render(...)` HEAD | In a ShaderBridge pass, binds the block atlas as `Sampler0` (the draw's albedo) before Sodium binds its pipeline. |
| `LevelRendererSodiumMixin` | `LevelRenderer.prepareChunkRenders(Matrix4fc, Z)` HEAD, cancellable | During pack frames, answers the shadow pass with Sodium's sections (Minecraft has none with Sodium). |
| `VanillaPipelineTableMixin` | ShaderBridge's `VanillaPipelineTable.lookup(Identifier)` HEAD, cancellable | Routes `sodium:pipeline/*_terrain` to the terrain programs with the `sodium_terrain` profile (`SodiumPipelines`). |

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

* Sodium is loaded, but ShaderBridge's Sodium integration cannot be applied to it, or Sodium's
  chunk vertex is not the one ShaderBridge extends (§9).
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
| `solid_terrain(_multidraw)` | `terrain_solid` | `shadow_solid` | `vanilla_terrain_section_ext` / `vanilla_terrain` (MDI); `vanilla_terrain_section` / `vanilla_terrain_basic` without the extended format |
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

### Terrain vertices (`render.chunk`)

Minecraft 26.3 meshes chunk sections in the 28-byte `BLOCK` format: position, color, atlas
coordinates and lightmap. While a pack renders, ShaderBridge meshes them in an extended 52-byte
format instead, as Iris extends Sodium's (`TerrainVertexFormat`, the layout of the
`vanilla_terrain` profile):

| Element | Format | Pack attribute | Content |
|---|---|---|---|
| `sb_Normal` | `RGBA8_SNORM` | `gl_Normal` | Normalized cross product of the quad's diagonals. |
| `sb_Entity` | `RG16_SINT` | `mc_Entity` | `block.properties` id (-1 if unmapped), render type 0 for block models and 1 for fluids (Iris' terrain encoding). |
| `sb_MidTexCoord` | `RG32_FLOAT` | `mc_midTexCoord` | Average of the quad's four atlas coordinates. |
| `sb_Tangent` | `RGBA8_SNORM` | `at_tangent` | Direction of growing `u` across the quad, orthogonal to the normal; `w` = +1 when `v` grows along `cross(tangent, normal)`, -1 for mirrored mappings. A quad without atlas area gets its cube face's tangent. |
| `sb_MidBlock` | `RGBA8_SINT` | `at_midBlock` | `(block centre - vertex) * 64`, rounded and clamped to a byte; `w` = the block's light emission. |

* **Block ids** (`BlockIdTable`, `BlockIdMapping`) are resolved once per pack from the
  `CompiledPack` id maps with Iris' precedence: every block entry before any tag entry, each in file
  order, the first match of a state wins. Property filters accept OptiFine's value lists
  (`age=6,7`); a filter on a property the block does not have is ignored, as in Iris. Fluid quads
  carry the id of the fluid's block (`FluidState.createLegacyBlock()`, so `minecraft:water` maps
  water), and `at_midBlock.w` is the emission of the block at the position, for fluids too (Iris).
* **Meshing.** `SectionCompiler.getOrBeginLayer` creates an `ExtendedTerrainBufferBuilder` for
  layers in the extended format. Minecraft's block renderer, Fabric's renderer API and the fluid
  renderer write the `BLOCK` elements through it as usual: with a format other than `BLOCK`,
  `BufferBuilder` writes element by element, byte-identical to its `BLOCK` fast path. The builder
  records the block of every quad (set by the `getBlockState` and fluid hooks of `compile`) and
  fills the extension attributes of the whole mesh when the layer is built, before the
  translucent layer's quads are sorted.
* **Drawing.** `ChunkSectionLayer.pipeline` returns clones of the terrain pipelines with the
  extended format in vertex slot 0 (`ExtendedTerrainPipelines`, located at
  `shaderbridge:extended_terrain/<namespace>/<path>`). Minecraft's shaders read the `BLOCK`
  elements at unchanged offsets and the pipeline builder skips the others, so the clones draw
  vanilla terrain from extended meshes: in frames without the pack, in the wireframe view, with
  order-independent transparency, and as the vanilla pipeline of fallback draws (`VanillaClones`
  copies their vertex layout). `VanillaPipelineTable` maps an extended location like the vanilla
  pipeline with the extended profile (`vanilla_terrain` on the MultiDrawIndirect path, whose
  programs the pack is compiled for by default, `vanilla_terrain_section_ext` on the per-section
  path); `PipelineRouter` falls back to the basic profile if the buffers lack the extension.
* **Switching** (`ChunkMeshFormat`). With no pack rendering every hook returns Minecraft's own
  values: meshing and drawing are vanilla. When a pack starts or stops rendering
  (`RenderBridge.packActive()`), or a new pack's block ids differ, the start of the next
  `LevelExtractor.extract` switches the format and rebuilds every chunk section: it releases the
  section meshes and the section dispatcher (`LevelRenderer.resetLevelRenderData`) and marks the
  level changed, as a world change does, so the dispatcher is recreated with the new vertex size
  before the frame uses it. Meshes of one format are never stored or drawn with the other. The
  first frame of a pack still draws the `BLOCK` meshes (with the basic profiles); the world is
  then remeshed. If the layer hooks did not apply, the format stays `BLOCK` and the switch is
  logged as unavailable.

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

## 9. Sodium (`compat.sodium`, `mixin.sodium`)

Sodium 0.9 replaces Minecraft's terrain renderer: its own chunk meshes (a 20-byte compact vertex),
render pipelines (`sodium:pipeline/<layer>_terrain`), render lists and draw path. Minecraft's main
pass still hands it the render pass to draw into (`SodiumChunkSection.renderGroup` →
`DefaultChunkRenderer.render`), so Sodium's terrain lands in ShaderBridge's gbuffers passes, where
the pipeline substitution (§4) swaps its pipelines for the pack's. The integration is written
against Sodium 0.9.2 and 0.9.3-alpha.1 for Minecraft 26.3 and copies no Sodium code.

**All or nothing.** `SodiumMixinPlugin` decides once, while Mixin loads
`shaderbridge-sodium.mixins.json` and before any class is loaded: the integration's mixins are
applied only if Sodium is installed and every member they hook or call exists in Sodium's and
Minecraft's class files (`SodiumTargets`, read through Mixin's bytecode provider). Otherwise none
is applied, Sodium renders untouched, and `SodiumCompat` refuses packs with "Sodium <version> is
installed, and ShaderBridge cannot shade its terrain: <what is missing>; update ShaderBridge or
Sodium, or remove Sodium to use shader packs". Packs are also refused if Sodium's compact vertex
is not the one ShaderBridge extends, or the extended vertex no longer feeds the `sodium_terrain`
profile (`SodiumTerrain.layoutProblems`).

**Terrain vertex** (`TerrainVertexLayout`, `ExtendedChunkVertex`). While a pack is active Sodium
meshes a 36-byte vertex: Sodium's 20 bytes, written by Sodium's own compact encoder (so they are
exactly what Sodium's shader and the profile decode), followed by ShaderBridge's extension:

| Offset | Element | Format | Pack attribute | Content |
|---|---|---|---|---|
| 0 | `a_Position` | `RG32_UINT` | `gl_Vertex` | Sodium: 20-bit section-local position; the profile adds `u_RegionOffset` and the section's offset in its region. |
| 8 | `a_Color` | `RGBA8_UNORM` | `gl_Color` | Sodium: colour times ambient occlusion. |
| 12 | `a_TexCoord` | `RG16_UINT` | `gl_MultiTexCoord0` | Sodium: 15-bit coordinate, nudged towards the quad centre by `u_TexCoordShrink`. |
| 16 | `a_LightAndData` | `RGBA8_UINT` | `gl_MultiTexCoord1` | Sodium: block and sky light (16 L + 8; the profile subtracts 8), material bits, section index. |
| 20 | `sb_Entity` | `R32_UINT` | `mc_Entity` | `((block id + 1) << 1) \| is fluid`: the `block.properties` id (-1 if unmapped) and render type 1 for fluids, 0 for block models. |
| 24 | `sb_Normal` | `RGBA8_SNORM` | `gl_Normal` | Normalized cross product of the quad's diagonals (the outward normal of the face drawn; flipped fluid faces point the other way). |
| 28 | `sb_MidTexCoord` | `RG16_UINT` | `mc_midTexCoord` | Average of the quad's texture coordinates times 32768. |
| 32 | `sb_MidBlock` | `RGBA8_SNORM` | `at_midBlock` | `(block centre - vertex) * 64`, rounded and clamped to ±127; `w` = the light emission of the block at the position. |

* **Block data.** Around Sodium's calls that mesh a block model or a fluid, the block is recorded
  for the worker thread (`ChunkBuilderMeshingTaskMixin`, `BlockContext`). Fluid quads carry the
  id of the fluid's block (`FluidState.createLegacyBlock()`, so `minecraft:water` maps water in a
  waterlogged stair). Ids come from a table of every block state built when the vertex is chosen,
  with `IdMapLookup` and the world's block tags.
* **Translucent sorting.** Sodium copies translucent quads for sorting and encodes the pieces of
  quads it splits after their block was meshed. Their vertices are tagged with the block's data
  when they enter the sorter (`TranslucentGeometryCollectorMixin`), and the tags travel through
  Sodium's vertex copies (`ChunkVertexMixin`); geometry that belongs to no block (other mods'
  mesh appenders) gets id -1 and no `at_midBlock`.
* **Switching** (`SodiumTerrain`, `MeshPlan`). The vertex is chosen when Sodium creates its
  section manager and chunk builder (`SodiumWorldRenderer.initRenderer`), because every buffer of
  that renderer uses it: extended when a pack is active (`ShaderBridge.activePack()`), Sodium's
  own otherwise. When that changes, or a pack with other block ids becomes active, Sodium's
  renderer is reloaded at the start of its next terrain update, right after chunk events are
  processed, where Sodium itself reloads after a render distance change; every section is meshed
  again. Sodium memoizes its terrain pipelines per pass with the vertex format they were built
  for, so the memo is cleared when the format changes. Sodium's own shader reads its four
  elements by name and skips the extension, so Sodium keeps drawing the extended meshes with its
  own pipelines when the pack does not draw a frame.

**Program swap.** `VanillaPipelineTableMixin` routes Sodium's three terrain pipelines through
`SodiumPipelines` to the programs of the vanilla terrain pipelines they stand for
(`gbuffers_terrain_solid`, `gbuffers_terrain_cutout`, `gbuffers_water`; `shadow_solid`,
`shadow_cutout`, `shadow_water`) with the `sodium_terrain` profile. Sodium names its pipelines
after `ChunkSectionLayer.pipeline(false)`; paths of ShaderBridge's extended vanilla terrain clones
(`extended_terrain/minecraft/pipeline/...`) map alike. From there the substitution is the vanilla
one: `PipelineRouter` keeps the mapping when Sodium's pipeline carries the profile's attributes
(only the extended vertex does; otherwise the draw stays vanilla, into `fallback_tex`), programs
are compiled for `sodium_terrain` on demand (`OnDemandVariants`), and `PackPipelineFactory`
builds them with Sodium's vertex binding, topology, depth state and 20-byte push-constant range.
When Sodium binds its pipeline, `FrontendRenderPassMixin` binds the pack pipeline with `sb_Frame`,
`sb_Draw` and the pack samplers; Sodium then binds its own `u_Globals`, `u_SectionTimeInfo`,
`u_LightTex` and `u_BlockTex` (which provide the pack's `lightmap` and `gtexture`) and pushes
`u_RegionOffset`, `u_CurrentTime` and `u_RegionID` for every region (std430 offsets 0, 12, 16, as
the profile declares them). `DefaultChunkRendererMixin` first binds the block atlas as `Sampler0`,
the albedo `sb_Draw` is sized by (`atlasSize`, `gtextureSize`) and unnamed pack samplers resolve
to. Classic transparency is forced as for vanilla terrain, so Sodium draws translucent terrain
with `renderGroup` in the translucent gbuffers pass; its order-independent-transparency pipelines
are never used.

**Shadow pass.** With Sodium, Minecraft's own chunk sections are empty: Sodium replaces the call
to `LevelRenderer.prepareChunkRenders` in the level render and never calls the method. During a
pack frame, `LevelRendererSodiumMixin` answers the shadow pass's call (`ShadowSections`) with
Sodium's sections: `SodiumTerrain.shadowSections` prepares Sodium's draw commands for its current
render lists (outside any pass, as Sodium does later in the frame), with the camera's position
and matrices, and returns a `SodiumChunkSection`. The shadow renderer then draws its opaque and
translucent groups into the shadow pass, where Sodium's pipelines become the pack's shadow
programs, or draw nothing if the pack has none.

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

* **Terrain vertices.** Packs see normals, block ids, mid-texture coordinates, tangents and
  `at_midBlock` on chunk terrain (§4). Remaining gaps:
  * `layer.*` render-layer overrides of `block.properties` are not applied; blocks keep
    Minecraft's chunk layers.
  * The block id table is built when the pack starts rendering; block tags changed later by a
    data pack reload are not picked up until the next pack or world change.
  * Ids outside the 16-bit range wrap, as in Iris' `RG16_SINT` attribute.
  * Switching packs on or off remeshes every loaded chunk section, as in Iris.
  * Blocks drawn outside chunk meshes (moving pistons, falling blocks) keep the `vanilla_block`
    profile, without the extension attributes.
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

* **Sodium** (§9):
  * Only checked against Sodium 0.9.2 and 0.9.3-alpha.1 for Minecraft 26.3, and never run in a
    game. Another Sodium version that moved a hooked member makes ShaderBridge refuse packs.
  * Shadow casters are Sodium's camera-visible sections, in the draw batches Sodium prepared for
    the camera: with Sodium's block-face culling on, faces turned away from the camera (by whole
    section) are missing from the shadow map. Closed geometry still casts its shadow.
  * The shadow pass is the first terrain draw of the frame, so it writes Sodium's per-frame
    terrain uniforms (with the camera's matrices, as the main pass would); their fog colour is the
    previous frame's. Only Sodium's own shader reads it (fallback draws while a program compiles).
  * Activating or deactivating a pack, or a pack with other block ids, reloads Sodium's renderer
    and remeshes every section; terrain drawn before the extended meshes exist stays unshaded
    (in `fallback_tex`). The extended vertex costs 80% more terrain vertex memory (36 instead of
    20 bytes) while a pack is active.
  * `mc_chunkFade` is 1 (Sodium's fade-in is not reproduced) and `at_tangent` is derived from the
    normal (the Sodium vertex has no tangent attribute, unlike ShaderBridge's extended vanilla
    vertex).
  * Block ids resolve with `IdMapLookup` (the first matching entry in file order, blocks and tags
    alike); the vanilla terrain path applies Iris' precedence (blocks before tags). They differ only
    for packs whose tag entry precedes a block entry matching the same state.
  * `layer.*` render-layer overrides of `block.properties` are not applied.
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
  The Sodium mixins have no such members, and their targets are checked before they are applied
  (§9).
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
* **Terrain vertices.** `TerrainVertexEncoderTest` (normals and tangents of every cube face, mirrored and
  rotated mappings, degenerate quads, `SNORM8` and `at_midBlock` packing, in-place encoding),
  `BlockIdMappingTest` (entry grammar, Iris precedence, property filters, unknown blocks),
  `TerrainVertexFormatTest` (the format equals the `vanilla_terrain` layout and starts with
  `BLOCK`), `ExtendedTerrainPipelinesTest` (clones of every 26.3 terrain pipeline),
  `ExtendedTerrainBufferBuilderTest` (vertex memory byte-identical to Minecraft's `BLOCK` builder;
  needs LWJGL's allocator), `ChunkMeshFormatTest` (switch decisions, vanilla hooks), and the
  extended routes in `VanillaPipelineTableTest`.
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
* **Sodium** (against the Sodium 0.9.3-alpha.1 jar on the test class path).
  * `SodiumTargetsTest`: every member the integration needs exists; a missing method, call site,
    class or static modifier is named; the all-or-nothing decision.
  * `SodiumMixinsTest`: every mixin of `shaderbridge-sodium.mixins.json` (targets, call sites,
    handler parameters) and that `SodiumTargets` checks each hooked member beforehand.
  * `ExtendedChunkVertexTest`: the encoder with Sodium's real compact encoder: Sodium's 20 bytes
    unchanged and decoding with the `sodium_terrain` formulas, the extension bytes.
  * `TerrainVertexLayoutTest` (Sodium's compact vertex, the profile's inputs, Sodium's bind groups
    against the profile's host blocks and samplers), `TerrainExtensionTest` (normals of every cube
    face, diagonal, flipped and degenerate quads; `mc_Entity`, mid-texture and mid-block packing),
    `SodiumPipelinesTest`, `MeshPlanTest`, `SodiumCompatTest`.
  * Rust: `crates/sb-transform/tests/sodium_profile.rs` pins the profile to Sodium's vertex,
    push constants, `u_Globals` and samplers.
* **Mixin targets.** `MixinTargetsTest`, `MixinMembersTest`, `RawMixinsTest`, `RedirectMixinsTest`, `GameTargetsTest`.
* **Native library.** The native smoke tests (`nativeTest`) load the Rust library and compile
  corpus packs.

### Compile-verified only (no GPU, no game)

* **Frame orchestration.** `RenderBridge`, `PackRenderer`, `MainPass`, `GeometryPasses`,
  `FullscreenPasses`, `ComputeDispatcher`, `DistantPasses`/`DistantFrame`, `MipGenerator.generate`,
  `PassCopies`, `SinkTextures`, `AtlasTextures`, `MinecraftHost`.
* **Shadow pass.** `ShadowRenderer`, `ShadowSections`.
* **Chunk mesh format in a running game.** `ChunkMeshFormat`'s switch and rebuild, the block id
  table against the live registries and tags, and meshing through the section compiler hooks.
* **Pipeline compilation on a real `GpuDevice`.** `PackPipelineCache` and `ProgramResolver` with
  real compiles, and `SessionVariantCompiler`.
* **Every Vulkan call of the raw path.** `VulkanContext`, `VmaImage`/`VmaBuffer`,
  `PipelineObjects`, `GraphicsPipelines`, `DescriptorAllocator`, `CommandRecorder`,
  `FullscreenRendering`, `RawResources`, `RawBindings`, `RawShaderProgram`, `VulkanRawPath`.
* **The DH takeover in a running game.** `DistantHorizons`, `DhApiControl`.
* **The Sodium integration in a running game.** `SodiumMixinPlugin` under Mixin, the vertex
  switch and Sodium reloads (`SodiumTerrain`), the block id table against the live registries and
  tags, the meshing hooks on Sodium's worker threads, the shadow-pass sections, and every Sodium
  mixin at runtime.
* **Every mixin at runtime.** The mixins are only checked statically against the jar.
* **`ShaderBridgeClient` lifecycle hooks.**
