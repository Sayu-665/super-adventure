# Draw profiles

A draw profile describes how a host draw path feeds a translated program:
its vertex attributes, host uniform blocks, host samplers, and GLSL
expressions implementing the OptiFine/Iris *compatibility semantics*
(`gl_Vertex`, `gl_Color`, `gl_MultiTexCoord*`, `mc_Entity`, matrices, …).
Hosts can register more profiles at runtime, which is how other mods' draw
paths are supported.

## Schema (TOML)

```toml
name = "unique_id"
description = "free text"
# Optional: the profile is used for fullscreen passes (no vertex buffer).
fullscreen = false
# Optional: positions are camera-relative world geometry (terrain, DH, Sodium). In the
# shadow pass (TransformOptions::is_shadow_pass) the model_view / projection semantics
# become shadowModelView / shadowProjection, as Iris shadow programs see them. Entity
# profiles leave it false: the host composes the shadow view into their model-view.
world_space = false
# Optional: host push constants, declared in every stage that references a member as
# `layout(push_constant) uniform sb_hPush { <members> };` (std430 offsets; members are
# referenced unqualified by semantics, globals and helper code).
push_constants = "vec3 u_RegionOffset; int u_CurrentTime; uint u_RegionID;"

[[inputs]]          # vertex attributes; `name` MUST equal the host VertexFormat element name
name = "Position"
type = "vec3"       # GLSL type the attribute is declared with
location = 0
# `instanced = true` marks per-instance attributes (informational)

[[blocks]]          # host std140 uniform blocks, declared verbatim with an instance name
name = "TerrainUniform"        # block type name (hosts such as Mojang match by this name)
instance = "sb_hTerrain"       # instance name used in expressions below
members = "mat4 ModelViewMat; ivec2 TextureSize;"

[[samplers]]        # host samplers; `provides` lists pack canonical sampler names they satisfy
name = "Sampler0"
type = "sampler2D"
provides = ["gtexture"]

[semantics]         # GLSL expressions evaluated in the vertex-stage prologue
position = "..."    # vec4  gl_Vertex (camera-relative position for world geometry)
color = "..."       # vec4  gl_Color
uv0 = "..."         # vec4  gl_MultiTexCoord0
lightmap = "..."    # vec4  gl_MultiTexCoord1/2, 0..240 range (x = block, y = sky)
normal = "..."      # vec3  gl_Normal
entity = "..."      # vec4  mc_Entity (x = block id or -1, y = render type; default (0, 0, 0, 1))
mid_tex_coord = "..." # vec4 mc_midTexCoord / gl_MultiTexCoord3
tangent = "..."     # vec4  at_tangent
mid_block = "..."   # vec4  at_midBlock
overlay = "..."     # ivec2 vaUV1 / overlay coordinates
model_view = "..."  # mat4  gl_ModelViewMatrix / modelViewMatrix
projection = "..."  # mat4  gl_ProjectionMatrix / projectionMatrix (GL-style, forward Z)
texture_matrix = "..."  # mat4 gl_TextureMatrix[0] / textureMatrix
lightmap_matrix = "..." # mat4 gl_TextureMatrix[1] / [2] (default: OptiFine lightmap matrix)
normal_matrix = "..."   # mat3 (default: mat3(transpose(inverse(model_view))))
chunk_offset = "..."    # vec3 chunkOffset / modelOffset (added by core-profile packs to vaPosition)

[[globals]]         # extra pack-visible variables, initialized in the vertex prologue;
                    # `varying = true` forwards them to later stages that reference them
name = "dhMaterialId"
type = "int"
stage = "vertex"
init = "int(irisMaterial)"

[code]              # helper functions injected before the pack's code
vertex = "..."
fragment = "..."
```

Expressions may use:
* the profile's inputs,
* block members through their instance names,
* push-constant members,
* host samplers,
* functions from `[code]`, and
* any builtin uniform name (e.g. `gbufferModelView`, `dhProjection`,
  `projectionMatrix`).

Builtin uniforms referenced this way are added to the pack's `sb_Frame` or
`sb_Draw` layout automatically (`sb_transform::PackBuilder::add_profile`, which also
adds `shadowModelView` / `shadowProjection` for `world_space` profiles).

Helper code may use `const int SB_<NAME>` constants supplied per program through
`TransformOptions::profile_constants` (emitted before the helper code; a missing
constant is `-1`), e.g. `SB_DH_BLOCK_ID_<n>` in `dh_terrain.toml`.

Built-in profiles: `fullscreen`, `vanilla_terrain`, `vanilla_terrain_basic`,
`vanilla_terrain_section` (per-section terrain path with the `ChunkSection` block),
`vanilla_block` (`BLOCK` vertices with `DynamicTransforms`: moving blocks, beacon beams,
block breaking), `vanilla_entity`, `vanilla_particle`, `vanilla_lines`, `vanilla_position`,
`vanilla_position_color`, `vanilla_position_color_lightmap` (leads), `vanilla_position_tex`,
`vanilla_position_tex_color`, `vanilla_text` (in-world text), `vanilla_clouds` (clouds
decoded from the `CloudFaces` texel buffer, no vertex buffer),
`dh_terrain`, `dh_generic` and `sodium_terrain` (Sodium 0.9 compact chunk format with
ShaderBridge's extension attributes and Sodium's 20-byte push constants). Semantics a profile does not define fall
back to the defaults in `defaults.toml`. The Java mod maps each vanilla 26.3 pipeline to one
of them (`dev.shaderbridge.render.mapping.VanillaPipelineTable`).

`dh_terrain_synth` (`sb_transform::DH_SYNTH_PROFILE`) is derived in code from
`dh_terrain` for DH programs synthesized from `gbuffers_terrain`/`gbuffers_water`/`shadow`:
only its lightmap differs, in the vanilla terrain convention those sources expect
(`gl_MultiTexCoord1`/`vaUV2` = 16 x light level with the OptiFine lightmap matrix, which
through `gl_TextureMatrix[1]` equals the `(level + 0.5) / 16` native DH programs read).
