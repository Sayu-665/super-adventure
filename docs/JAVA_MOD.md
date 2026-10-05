# ShaderBridge Java mod (Fabric, Minecraft 26.3)

This document describes how the Fabric mod in `java/` renders a compiled shader pack
(`CompiledPack`, `crates/sb-core/src/model.rs`) inside Minecraft 26.3. Minecraft 26.3 renders
through Mojang's renderpearl API, with an OpenGL and a Vulkan backend. **26.3 starts on OpenGL by
default** (`PreferredGraphicsApi.getBackendsToTry` tries OpenGL first unless the player picks
Vulkan); Vulkan is opt-in under Options → Video Settings → Graphics API → "Prefer Vulkan
(Experimental)", and becomes the default in the 26.4 snapshots. The mod renders packs on both
backends, but the raw Vulkan path (§7: compute programs, custom images and SSBOs, composite viewport
scale) exists only on Vulkan. The host conventions the mod
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
| `render.draw` | Pipeline substitution inside ShaderBridge's passes: vanilla clones, uniform binding, the compiled-pipeline index; render pass redirection (`PassRedirect`) and viewports (`PassViewport`). |
| `render.shadow` | The shadow pass: terrain re-rendered for the shadow camera (casters culled against the shadow frustum) and the frame's prepared entities and block entities drawn again with shadow-camera transforms. |
| `render.raw` | The raw Vulkan path: compute and fullscreen programs on Mojang's `VkDevice`, extra device features, storage usage. |
| `dh` | Distant Horizons takeover (reflection and API, no mixins). |
| `compat.sodium` | The Sodium 0.9 integration (§9): its all-or-nothing target check, the extended Sodium terrain vertex and its encoder, the routing of Sodium's terrain pipelines, and Sodium's sections for the shadow pass. Its optional mixins are in `mixin.sodium`. |
| `mixin` | All mixins and accessors (§2). |

## 2. Integration points

Every target below is checked against `minecraft-merged-deobf-26.3.jar` by `MixinTargetsTest`
(`@Inject` selectors and handler parameters), `MixinMembersTest` (`@Shadow`, `@Accessor`,
`@Invoker`, `@ModifyVariable`, `@ModifyArg`, `@WrapOperation`), `RawMixinsTest`
(`@WrapMethod`, `@ModifyReturnValue`), `RedirectMixinsTest` (`@Redirect` of a constructor) and
`ExpressionMixinsTest` (`@ModifyExpressionValue`, and the calls the main-pass takeover wraps: each
made once in `lambda$addMainPass$0`, in the order the takeover assumes).

Every injection uses `require = 0`. A hook that does not apply in another Minecraft version
leaves vanilla rendering in place, or makes ShaderBridge refuse or disable the pack with a message.
Accessors, invokers and `@Shadow` fields cannot be made optional in Mixin: if one of their targets
disappears, the game fails at class load (see [Limitations](#11-known-limitations)).

### Mixins (`shaderbridge.mixins.json`, client side)

| Mixin | Target and point | Why |
|---|---|---|
| `LevelRendererMixin` | `LevelRenderer.render(...)` HEAD | Starts a pack frame (`RenderBridge.beginLevel`): sync the pack, poll pipelines, resize targets, fill uniforms, clear. |
| | `@ModifyExpressionValue` of `FeatureRenderDispatcher.prepareFrame(SubmitNodeStorage)` in `render` | Takes the frame's prepared features and runs everything before the opaque geometry (setup/begin, the shadow pass, which draws those features for the shadow camera, shadowcomp, prepare). |
| | `@WrapOperation` of `SkyRenderer.render(GpuBufferSlice, SkyRenderState)` in `lambda$addSkyPass$0` | Ends the sky's gbuffers pass (`SkyRendererMixin` opens it); a failure abandons the frame. |
| | In `lambda$addMainPass$0(GpuBufferSlice, Z, ChunkSectionsToRender, PreparedFrame, Z, Z)`: HEAD; `@WrapOperation` of `CommandEncoder.createRenderPass(Supplier, GpuTextureView, Optional, GpuTextureView, OptionalDouble)`, of `executeSolid` and of `executeClassicTransparency`; RETURN | The main pass takeover (§3). Nothing of the body is cancelled: the pass it creates is ShaderBridge's opaque gbuffers pass, and the pack's passes run around the two calls. Other mods' hooks in the body, Fabric API's world render events (`END_MAIN` at RETURN included), still run. RETURN checks that the pack frame ended. |
| `GameRendererMixin` | `useImprovedTransparency()` HEAD | Returns false while a pack is active: packs draw translucents in their gbuffers pass, never through vanilla OIT. |
| | `@ModifyArg` in `renderLevel` at `ProjectionMatrixBuffer.getBuffer(Matrix4f)` | Captures the level projection (view bobbing and nausea applied) for `gbufferProjection`. |
| `SkyRendererMixin` | `@WrapOperation` of `CommandEncoder.createRenderPass(Supplier, GpuTextureView, Optional, GpuTextureView, OptionalDouble)` in `SkyRenderer.render` | Hands the sky a gbuffers pass instead of the main target. |
| `FrontendRenderPassMixin` | `@WrapMethod` `setPipeline` | Pipeline substitution in ShaderBridge passes, then binds `sb_Frame`, `sb_Draw` and the pack samplers (§4). A pipeline that can be drawn in the pass in no form is not bound. |
| | `@Inject` HEAD, cancellable, of every draw method (`draw*`, `multiDraw*`, `drawIndexed*`, `drawMultipleIndexed`) and `pushConstants` | Drops the draws of a pipeline that was not bound, until the next pipeline. |
| | `@WrapMethod` `setUniform(String, GpuBufferSlice)` | Lets a ShaderBridge pass swap a uniform block as vanilla binds it: the feature shadow pass's `DynamicTransforms` (§6). |
| | `@Inject` `setUniform(String, GpuTextureView, GpuSampler)` RETURN | When vanilla binds another `Sampler0` after the pipeline (as `PreparedRenderType` does), rebinds what depends on the albedo: `sb_Draw` with `gtextureSize`/`atlasSize`, and pack samplers that sample the atlas. |
| | `@Inject` `close` HEAD; `@Shadow` `uniforms`, `backend`, `isClosed`, `pushedDebugGroups` | If vanilla code failed between `pushDebugGroup` and `popDebugGroup` in a ShaderBridge pass, pops the open groups. The pass then still ends and the failure reaches the frame's fail-soft handling. Without this, Mojang's single command encoder would stay "in a render pass" and the next pass (the GUI) would crash the game. |
| `DynamicGpuDataMixin` | `DynamicGpuData.writeTransform(Transform)` and `writeTransforms(Transform[])` RETURN | While a pack frame prepares its features, records every `DynamicTransforms` block written, so the shadow pass can write shadow-camera copies of them (§6). |
| `FrontendCommandEncoderMixin` | `FrontendCommandEncoder.createRenderPass(RenderPassDescriptor)` HEAD, cancellable (every other overload delegates to it) | While a redirection is armed (`PassRedirect`), returns a ShaderBridge pass instead: DH's generic object renderer draws into the pack's gbuffers (§8). |
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
   1. Capture the game state and update `FrameState` (`centerDepthSmooth` from the newest
      finished centre-depth readback, below).
   2. Upload `sb_Frame`; write the two default `sb_Draw` blocks into this frame's `sb_Draw` pages.
   3. Clear targets per `ColorTarget.clear`, and clear every target on its first frame. The depth
      copies (`depthtex1`, `depthtex2`, `shadowtex1`) are cleared only on their first frame: as in
      Iris, programs that run before this frame's copy see the previous frame's depth.
   4. Start the DH frame.
2. **Once Minecraft has prepared the frame's features** (`FeatureRenderDispatcher.prepareFrame`,
   `PackRenderer.beforeGeometry`), still outside any pass: `setup` (on the first frame, and again
   after a resize recreated the screen-sized targets, images and buffers, as Iris does), `begin`,
   the **shadow pass** (§6) with its shadowcolor mipmaps, `shadowcomp` and `prepare`. Each pass
   starts by adopting its `flip_state`, and the flips follow `flips_after`. (If the feature hook
   did not apply, these run at the start of the sky or main pass, without entity shadows.)
3. **Sky pass**:
   1. `SkyRenderer.render` opens its pass through the wrapped `createRenderPass`. ShaderBridge
      returns a gbuffers pass instead.
   2. The sky pipelines are substituted with `gbuffers_skybasic` and `gbuffers_skytextured`.
4. **Main pass** (`lambda$addMainPass$0`, run as Minecraft wrote it):
   1. Terrain fog, the chunk sampler and `prepareTranslucents`. DH hands over its LODs here.
   2. Where Minecraft creates its render pass: the opaque DH LODs (`dh_terrain`), DH's generic
      objects (`dh_generic`, §8), then the copy of the LOD depth to `dhDepthTex1` and
      `dhDepthTex0` (both include the generic objects). Minecraft gets a **gbuffers pass** instead of its own: the pack's
      `gbuffer_attachments` in their current textures plus Minecraft's main depth.
   3. In it, vanilla `executeSolid` draws opaque terrain and opaque features.
   4. Before `executeClassicTransparency`: the gbuffers pass is closed; the centre texel of the
      main depth is copied into a readback buffer (`CenterDepthProbe`); the `depthtex2` and
      `depthtex1` copies; `deferred`; `dh_water` and the `dhDepthTex0` copy.
   5. `executeClassicTransparency` draws the translucent features and terrain, clouds, weather and
      the world border into another gbuffers pass.
   6. `composite`, then `final` into Minecraft's main color target. Without a `final` program,
      `colortex0` is blitted there. Then the `end_of_frame_copies` (alt → main). This ends the
      pack frame.
   7. Minecraft closes the pass it holds (already closed), and `executeOutline`,
      `executeSeeThrough` and `executeAlwaysOnTop` draw into its main target. They, and the
      rest of the body with other mods' injections, run whether or not the pack frame succeeded.

`sb_Draw` blocks live in host-visible, coherent buffer pages, one set per frame in flight
(`DrawUniforms`), each set reused only once the fence of its frame has signalled. A draw kind seen
for the first time in a frame has its block written right away, through a buffer mapping, from
inside the pass (a mapped write needs no transfer command), so new draw kinds take effect in the
frame they first appear.

Composite-style programs are fullscreen draws. Each runs in its own pass over its output targets
(the textures `FlipState.write` selects), as six vertices of the `fullscreen` profile, and reads
its inputs per `BindingUse.use_alt`. Targets a program lists in `mipmap` get their mip levels
generated just before it runs (`MipGenerator`). Computes are dispatched through renderpearl or the
raw path (`ComputeDispatcher`). Dispatches are sized from `workGroups`/`workGroupsRender`;
shadow-pass computes are sized over the shadow map.

### Fail-soft

The following abandon the frame, release the pack's render resources and show the error, after
which Minecraft renders vanilla until another pack (or a recompile) is activated:

* an exception anywhere in a pack frame, including inside a substituted vanilla draw (after a
  failure in the opaque geometry, the translucent geometry of that frame is not drawn);
* a frame that did not end with the main pass (a missing hook);
* an inactive SPIR-V hook.

Diagnostics that do not stop the pack go to the log through `PipelineDiagnostics`, which
deduplicates them. Examples: programs that fall back or are skipped, unsupported features,
missing resources, draws that are skipped, and chunk terrain without the extended vertex (only
when its format switch is unavailable). Diagnostics of variants compiled on demand are logged. The pack screen lists them ("Rendering diagnostics"),
for the pack that renders or, after it stopped, with the reason it stopped.

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
* **Blend**: `blend.<program>` or the program file's own override (shadow programs: none;
  `gbuffers_spidereyes`: additive). Without either (`Program.inheritBlend`), the pipeline keeps the
  blend of the vanilla pipeline it replaces, as Iris does: one `gbuffers_terrain` draws solid and
  cutout terrain unblended and water blended. Draws ShaderBridge makes from a draw profile (DH
  LODs) use the slot's `GeometrySlot.blend`. Per-buffer overrides apply on top. Because the blend
  depends on the slot, the slot is part of the pipeline's identity (`PipelineShape.forSlot`).
* **Alpha test**: the comparison is compiled into the program; the reference is the slot's
  (`GeometrySlot.drawn`): `sb_Draw.alphaTestRef` is 0.5 for cutout terrain, 0.1 for water and
  entities, and a value every alpha passes for solid terrain, unless the pack sets
  `alphaTest.<program>`.

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

1. Look up the vanilla `RenderPipeline` (`CompiledPipelineIndex`). A pipeline that did not come
   from Minecraft's pipeline cache (another mod compiled it itself) is drawn as is if it fits the
   pass; otherwise its draws are skipped, with a diagnostic (step 5).
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
   pipeline that cannot be adapted is bound as is only if it fits the pass (never in the shadow
   pass). Otherwise it is not bound and its draws and push constants are dropped until the next
   `setPipeline`, with a diagnostic; the frame goes on without them.

Two passes route differently. The feature shadow pass (§6) draws only what casts shadows
(particles and weather do not) and swaps each `DynamicTransforms` block for its shadow-camera
copy. The DH generic pass (§8) draws every pipeline with the `dh_generic` program, if the
pipeline's vertex bindings match its draw profile.

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
* **Switching** (`ChunkMeshFormat`). With no pack rendering, or with Sodium (which meshes terrain
  itself, §9), every hook returns Minecraft's own values: meshing and drawing are vanilla. When a pack starts or stops rendering
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
  * Sizes follow `TargetSize` (relative sizes truncated, as Iris does). Shadow resolution is
    clamped to 16..8192 and to the device limit per format.
  * Formats are the renderable form of the pack format.
  * A full mip chain is allocated for targets a program requests mipmaps of.
* **Clears** follow `ColorTarget.clear`/`clear_color`, with defaults as in `sb-runtime`. Every
  target is cleared on its first frame after creation or resize.
* **Depth**:
  * `depthtex0` is Minecraft's main depth.
  * `depthtex1` and `depthtex2` are copies taken after the opaque geometry, cleared only when they
    are created (programs before the copy read the previous frame's, as in Iris).
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
`colortex0` in its current texture elsewhere. Render targets and shadow maps are filtered linearly
only if the device can filter their format (`SAMPLED_IMAGE_FILTER_LINEAR` on Vulkan, as
`sb-runtime` checks; every non-integer format on OpenGL); otherwise they are sampled nearest.

## 6. Uniforms and the shadow pass

### Uniforms

* `sb_Frame` is filled once per frame from `FrameState`, using the `uniforms` providers. Custom
  uniforms are evaluated natively.
* `sb_Draw` has one block per **draw kind** (`DrawKey`). A kind is identified by:
  * the pipeline;
  * `renderStage` (Iris phase numbers);
  * shadow or not, which selects the model-view and projection;
  * `alphaTestRef` (the slot's reference, §4);
  * `blendFunc`;
  * the albedo size (`gtextureSize`, and `atlasSize` when the albedo is one of Minecraft's texture
    atlases).

  A kind's block is written when the kind is first drawn in a frame, into mapped pages of the
  frame (§3); a frame with more kinds than `DrawSlots` holds gives the rest the default block.
* `centerDepthSmooth`: after the opaque geometry the centre texel of the main depth is copied
  into one of three small readback buffers (`CenterDepthProbe`); the newest copy the GPU has
  finished (one or two frames old) is decoded (`CenterDepth`: `D32_FLOAT` or `D16_UNORM`,
  reversed-Z turned into forward depth) and smoothed with `centerDepthHalflife`.
* `heldItemId`/`heldItemId2` come from `item.properties` (the held stack's item model, else its
  item id). `entityId` and `blockEntityId` stay -1 (see §11).
* Host blocks (`Globals`, `Projection`, `Fog`, `DynamicTransforms`, `TerrainUniform`,
  `ChunkSection`, ...) are bound by the vanilla draw path itself.

### Shadow pass

1. `ShadowRenderer` prepares the shadow casters' chunk sections for the shadow camera through
   Minecraft's own path (`prepareChunkRenders`, MDI when the level uses it). Casters are
   culled against the shadow camera (`ShadowSections.SHADOW_FRUSTUM`, `ShadowCulling`): every
   built section within the shadow render distance whose box intersects the
   `shadowProjection * shadowModelView` frustum, seen by the player's camera or not. The distance
   follows `shadowDistanceRenderMul` (negative: the render distance; positive:
   `shadowDistance * shadowDistanceRenderMul`, at most the render distance); `shadow.culling=false`
   culls by distance only. Minecraft's visible-section list is swapped for the casters during the
   call and restored after it. With Sodium, Sodium's sections for the camera are drawn (§9).
2. It grows Minecraft's shared quad index buffer to the requested count. Vanilla grows it only
   later in the frame, in `prepareTranslucents`.
3. It runs the steps of `ShadowPlan`: opaque terrain; the **entities** step; the opaque DH LODs
   (`dh_shadow`); the `shadowtex0` → `shadowtex1` copy; translucent terrain. Each group is drawn in
   a pass on `shadow_attachments` + `shadowtex0`.

**Entity shadows.** The shadow pass runs once Minecraft has prepared the frame's features
(entities, block entities, the player and held items, all baked camera-relative, with each draw's
transform in a `DynamicTransforms` block written at preparation). While they are prepared,
`DynamicGpuDataMixin` records every block written (`ShadowTransforms`). The entities step writes
a copy of each with `ModelViewMat = shadowModelView * inverse(gbufferModelView) * ModelViewMat`
(the camera's view rotation undone, then the shadow camera's model-view: the matrix `sb-runtime`
uses for entities in its shadow pass), then executes the prepared opaque features
(`PreparedFrame.executeSolid`) into a shadow pass that swaps each block for its copy as the draws
bind it. Particles and weather cast no shadows. The step runs when the pack enables any of
`shadowEntities`, `shadowBlockEntities` and `shadowLightBlockEntities`. Translucent features cast no
shadows. A failure in this step turns feature shadows off for the pack, with a diagnostic.

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
back along their chain. The raw path builds fullscreen and compute pipelines only, and
`GraphicsPipelines` refuses tessellation stages outright, so the
`VkPipelineTessellationDomainOriginStateCreateInfo` (`LOWER_LEFT`) that `sb-runtime` chains for
tessellated geometry (ARCHITECTURE §4) has no counterpart here. A future raw geometry path must
chain it.

**Viewport scale.** Raw fullscreen draws use the `scale.<program>` viewport (`ViewportRect`, as
`sb-runtime` computes it; the scissor stays the whole target).

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
  as for Iris (and `sb-runtime`).

**Generic objects** (`dh_generic`: beacon beams, DH clouds, objects added through DH's API). While
DH's output is held, DH's own generic rendering is switched off through its API config
(`genericRendering().renderingEnabled()`, restored afterwards) and ShaderBridge replays DH's
generic renderer for the frame instead (`DhGenericHandles`: the frame's `RenderParams.genericRenderer`,
captured when DH hands over its LODs, and `IDhGenericRenderer.render(RenderParams, IProfilerWrapper,
boolean)`, called for the SSAO groups and then the others). The replay runs after the opaque LODs,
before the LOD depth copies, with `PassRedirect` armed: every render pass DH's renderer opens (one
per box group, on DH's own textures) is replaced by a gbuffers pass on the LOD depth in which every
pipeline draws with the `dh_generic` program (`GeometryPasses.openDistantGeneric`). DH binds its own
`vertUniformBlock` and `uLightMap`. Nothing is replayed when the player has generic rendering off,
the pack has no `dh_generic` program or no shared gbuffers attachments, or DH's members differ; a
failure stops the replay for the session with a warning.

**Modes** (`DhMode`):

* **NATIVE**: the pack's own `dh_*` programs, with a separate `D32` LOD depth.
  * `dhDepthTex1` is copied after `dh_terrain` and the generic objects.
  * `dhDepthTex0` is copied at the same point and again after `dh_water`, so it always holds the
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
  resolved as for the vanilla terrain vertex (`render.chunk.BlockIdTable`: Iris' precedence, filters
  on missing properties ignored) against the world's block tags, so terrain gets the same ids with
  and without Sodium.
* **Translucent sorting.** Sodium copies translucent quads for sorting and encodes the pieces of
  quads it splits after their block was meshed. Their vertices are tagged with the block's data
  when they enter the sorter (`TranslucentGeometryCollectorMixin`), and the tags travel through
  Sodium's vertex copies (`ChunkVertexMixin`); geometry that belongs to no block (other mods'
  mesh appenders) gets id -1 and no `at_midBlock`.
* **Switching** (`SodiumTerrain`, `MeshPlan`). The vertex is chosen when Sodium creates its
  section manager and chunk builder (`SodiumWorldRenderer.initRenderer`), because every buffer of
  that renderer uses it: extended while ShaderBridge renders a pack in the dimension
  (`RenderBridge.packActive()`, so a refused pack or one without a pipeline for the dimension costs
  no remesh), Sodium's own otherwise. Minecraft's own chunk mesh format (§4) stays vanilla while
  Sodium draws terrain: its meshes are never built, so switching it would only reload Sodium's
  renderer a second time. When that changes, or a pack with other block ids becomes active, Sodium's
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
  * Switching packs on or off remeshes every loaded chunk section, as in Iris (with Sodium, only Sodium remeshes, §9).
  * Blocks drawn outside chunk meshes (moving pistons, falling blocks) keep the `vanilla_block`
    profile, without the extension attributes.
* **Block entities.** They draw with the entity programs. Iris tells them apart by rendering phase
  and uses `gbuffers_block`; Minecraft 26.3 batches both into the same feature draws.
* **Entity shadows** (§6). Casters are the features Minecraft prepared for the camera: entities
  outside the camera's view, and the player in first person, cast no shadow (Iris renders
  entities for the shadow camera separately). `shadowEntities`, `shadowPlayer`,
  `shadowBlockEntities` and `shadowLightBlockEntities` cannot be told apart, since the features
  come in one batch: any of the three non-player directives enables them all (the pack's
  diagnostics say so). `entityShadowDistanceMul` is not applied. Translucent features cast no
  shadows.
* **Shadow terrain culling.** Only sections Minecraft has built cast shadows. Minecraft builds,
  and rebuilds after changes, only the sections the camera sees, so terrain never yet seen, or
  changed while out of view, casts no (or a stale) shadow. `shadow.culling=reversed`/`safe_zone`
  culls like `true`. Multi-draw-indirect is chosen from the previous frame's setting.
* **Hand.** Hand rendering stays vanilla and is drawn after `final`.
* **Per-draw ids.** `entityId` and `blockEntityId` are -1: Minecraft 26.3 batches the draws of
  many entities and block entities into one prepared feature draw, so no draw belongs to a single
  one (Iris carries the id in an extra vertex attribute of its own entity vertex format).
  `centerDepthSmooth` follows the depth one or two frames late (readback without a stall).
* **Composite passes.** On the OpenGL backend, viewport scale and offset (`scale.<program>`) are
  not applied (diagnostic): the viewport is set through the Vulkan render pass's command buffer.
  Gbuffer attachments with a scaled size are not drawn by geometry: they get sinks.
* **Without `independentBlend`.** Vanilla fallback draws in a multi-attachment gbuffers pass write
  depth only, and pack programs that leave attachments untouched fall back or are skipped.
* **Feedback copies.** Which programs draw in a pass is unknown in advance, so the targets sampled
  by any geometry program of the pass kind are copied before every such pass. Programs that
  sample a target they also write read its contents from before the pass.
* **Fabric API.** Fabric API's world render events fire during pack frames. Those inside the main
  pass (`START_MAIN` to `AFTER_TRANSLUCENT_FEATURES`) see ShaderBridge's gbuffers pass as
  the main pass; what other mods draw there is substituted like vanilla draws. `END_MAIN` fires
  after `final`.
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
    once and remeshes every section; terrain drawn before the extended meshes exist stays unshaded
    (in `fallback_tex`). The extended vertex costs 80% more terrain vertex memory (36 instead of
    20 bytes) while a pack renders.
  * `mc_chunkFade` is 1 (Sodium's fade-in is not reproduced) and `at_tangent` is derived from the
    normal (the Sodium vertex has no tangent attribute, unlike ShaderBridge's extended vanilla
    vertex).
  * `layer.*` render-layer overrides of `block.properties` are not applied.
* **Distant Horizons:**
  * DH generic objects (`dh_generic`, §8) are drawn by replaying DH's generic renderer through
    reflection on `RenderParams.genericRenderer` and `IDhGenericRenderer.render`; another DH
    version that changes them stops generic objects (with a log warning), not the LODs. Generic
    objects cast no shadows.
  * Shadow LODs use the previous frame's list.
  * Synthesized LODs overlap vanilla terrain in sections crossing the vanilla edge.
  * `reduceOverdrawWithFastMovement` is not reproduced in `dhNearPlane`.
  * Frustum culling is off while earth curvature is enabled.
  * The unified projection takes effect one frame after it is requested.
* **Raw path:**
  * GL backend (Minecraft 26.3's default): no raw path. Compute programs, custom images, SSBOs
    and composite viewport scales need the player to select Vulkan.
  * Raw fullscreen draws generate no mipmaps.
  * Cube, array and multisampled images, runtime arrays and more than 4 descriptor sets are
    rejected.
  * Synchronization is a full barrier around every use.

### Robustness

* **Depth modes.** Only `REVERSED_ZERO_TO_ONE` on devices with [0, 1] clip depth renders. On
  OpenGL without `GL_ARB_clip_control` (macOS) packs are refused.
* **Hard mixin targets.** Accessors, invokers and `@Shadow` fields (`FrontendGpuDeviceAccess`,
  `VulkanDeviceAccess`, the `@Shadow` fields of `FrontendRenderPassMixin`) are hard requirements
  of Mixin. If a future Minecraft version renames
  them, the game fails at start-up rather than disabling shaders. All injections are optional.
  The Sodium mixins have no such members, and their targets are checked before they are applied
  (§9).
* **Unadaptable pipelines.** A pipeline drawn in a ShaderBridge pass that can be drawn there in no
  form (another mod's custom pipeline that neither came from Minecraft's pipeline cache nor fits
  the pass) is skipped with its draws, with a diagnostic; that mod's geometry is missing while
  the pack renders.
* **Diagnostics** are logged, summarized in toasts or chat, and listed in the pack screen.
* **Closing a pack** never waits for a native call in flight (a compile or an on-demand variant
  on a worker thread): the native session is released when that call returns.

## Verification

### Unit-tested (JUnit, `./gradlew build`)

* **Frame sequencing.** `FramePlanTest`, `FlipStateTest`, `FrameSequencerTest` and
  `FrameSequencerTraceTest` compare against reference traces of `sb-runtime` for real compiled
  packs: Complementary Reimagined, Photon, Rethinking Voxels, Glimmer and the Tutorial pack.
* **Attachments, reads and draw state.**
  * `PassAttachmentsTest`: sinks in every missing slot.
  * `ColorReadsTest`, `ImageChoiceTest`: main and alt selection, including unnamed samplers.
  * `DispatchSizeTest`.
  * `DrawKeyTest`: albedo sizes; `sb_Draw` blocks written when a kind is first drawn in a frame.
  * `CenterDepthTest`: centre-depth decoding (formats, reversed-Z); `ViewportRectTest`:
    `scale.<program>` viewports and the Vulkan pass member they are set through.
  * `FeedbackReadsTest`: copies of attached targets; `DepthSupportTest`: the depth-mode gate.
  * `MipGeneratorTest`.
* **Pipelines.**
  * `PackPipelineFactoryTest`, including SPIR-V capability rejection.
  * `AttachmentPlannerTest`, `PipelinePartsTest`, `SpirvReflectorTest`, `ProgramResolverTest`,
    `PackPipelineCacheTest` (with fake devices), `GeometryChainTest`, `ProfileVertexFormatsTest`,
    `OnDemandVariantsTest`, `CompiledVariantsTest` (the slot's variants by draw profile).
  * `RawAdmissionTest`, including that tessellated programs never reach the raw path.
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
  (albedo rebinding), `ActivePassesTest` (skipped pipelines, swapped uniform blocks),
  `PassRedirectTest`.
* **Targets.** `TargetPlannerTest`, `PackTargetsTest` (fake device, including level views),
  `PackTexturesTest`, `SamplerChoiceTest` (integer targets at their base level),
  `TextureDataTest`, `PackFilesTest`.
* **Shadow.** `ShadowPlanTest` (including the entities step), `ShadowTransformsTest` (recorded
  transforms and their shadow-camera model-view), `ShadowCullingTest` (shadow render distance,
  section radius, column range, frustum test against an orthographic shadow camera).
* **Raw path.** `DescriptorPlannerTest`, `RawAdmissionTest`, `SpirvCapabilitiesTest`,
  `RawTextureDataTest`, `CommandPlanTest`, `ResourceSizesTest`, `ComputeLimitsTest`,
  `ColorStatesTest`, `StorageUsageTest`, `StorageTargetsTest`, `RawFeatureTest`,
  `SpirvBlockSizesTest`.
* **Distant Horizons.** `DhHostBlocksTest`, `DhPlanesTest`, `DhModeTest`, `LodSelectionTest`,
  `LodUniformsTest`, `DhDepthTargetsTest`, `RendererSwapTest`, `DhInternalsTest` (against the DH
  3.3.4 jar, including the generic renderer handles), `CameraFarPlaneTest`, `DistantBindingsTest`.
* **Metadata.** `ModMetadataTest`: the Distant Horizons `suggests`/`breaks` ranges.
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
* **Mixin targets.** `MixinTargetsTest`, `MixinMembersTest`, `RawMixinsTest`, `RedirectMixinsTest`, `ExpressionMixinsTest`, `GameTargetsTest`.
* **Native library.** `nativeTest`: `NativeSmokeTest` loads the Rust library and compiles a
  minimal pack through JNI; `NativeCorpusSmokeTest` compiles ComplementaryReimagined through JNI
  when a corpus is given (`-Pshaderbridge.corpus` or `SB_CORPUS_DIR`) and is skipped otherwise.
* **Sessions and slot state.** `PackSessionTest` (closing never waits for a running native call),
  `GeometrySlotTest` (per-slot blend and alpha reference), `TerrainVertexNoteTest`, and the
  inheritance case of `AttachmentPlannerTest`.

### Compile-verified only (no GPU, no game)

* **Frame orchestration.** `RenderBridge`, `PackRenderer`, `GeometryPasses`, `CenterDepthProbe`,
  `DrawUniforms` (mapped pages and fences), `PassViewport`,
  `FullscreenPasses`, `ComputeDispatcher`, `DistantPasses`/`DistantFrame`, `MipGenerator.generate`,
  `PassCopies`, `SinkTextures`, `AtlasTextures`, `MinecraftHost`.
* **Shadow pass.** `ShadowRenderer`, `ShadowSections` (the caster walk over the view area), the
  feature shadow pass.
* **Chunk mesh format in a running game.** `ChunkMeshFormat`'s switch and rebuild, the block id
  table against the live registries and tags, and meshing through the section compiler hooks.
* **Pipeline compilation on a real `GpuDevice`.** `PackPipelineCache` and `ProgramResolver` with
  real compiles, and `SessionVariantCompiler`.
* **Every Vulkan call of the raw path.** `VulkanContext`, `VmaImage`/`VmaBuffer`,
  `PipelineObjects`, `GraphicsPipelines`, `DescriptorAllocator`, `CommandRecorder`,
  `FullscreenRendering`, `RawResources`, `RawBindings`, `RawShaderProgram`, `VulkanRawPath`.
* **The DH takeover in a running game.** `DistantHorizons`, `DhApiControl`, the generic object
  replay (`DhGenericHandles` calls, `FrontendCommandEncoderMixin` redirection).
* **The Sodium integration in a running game.** `SodiumMixinPlugin` under Mixin, the vertex
  switch and Sodium reloads (`SodiumTerrain`), the block id table against the live registries and
  tags, the meshing hooks on Sodium's worker threads, the shadow-pass sections, and every Sodium
  mixin at runtime.
* **Every mixin at runtime.** The mixins are only checked statically against the jar.
* **`ShaderBridgeClient` lifecycle hooks.**
