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
entity = "..."      # vec4  mc_Entity (x = block id or -1, y = render type)
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
* host samplers,
* functions from `[code]`, and
* any builtin uniform name (e.g. `gbufferModelView`, `dhProjection`,
  `projectionMatrix`).

Builtin uniforms referenced this way are added to the pack's `sb_Frame` or
`sb_Draw` layout automatically. Semantics a profile does not define fall
back to the defaults in `defaults.toml`.
