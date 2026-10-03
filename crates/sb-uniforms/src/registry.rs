//! The builtin-uniform registry: every loose uniform the host can supply to a pack.
//!
//! The list covers the OptiFine standard uniforms, the Iris additions (Iris 1.11,
//! `common/src/main/java/net/irisshaders/iris/uniforms/*`), the Distant Horizons
//! uniforms, the core-profile names (`modelViewMatrix`, ...) and a few values that
//! ShaderBridge itself needs to replace fixed-function state (`gl_Fog`).
//!
//! Types follow Iris wherever OptiFine and Iris disagree (Iris is what packs are
//! tested against today):
//!
//! * `hideGUI` is `bool` (OptiFine documents `int`). Iris accepts `int` and `bool`
//!   declarations of every `int`/`bool` scalar uniform, see [`is_type_compatible`].
//! * `biome`, `biome_category` and `biome_precipitation` are `int` uniforms (OptiFine
//!   only offers them as `float` custom-uniform parameters).
//!
//! OptiFine-only values (`bossBattle`, `spriteBounds`, `instanceId`,
//! `terrainTextureSize`, `terrainIconSize` and the custom-uniform parameters
//! `is_alive`, `is_child`, ...) are included as well, so that hosts can provide them.

use sb_core::GlslType;
use sb_core::ScalarKind;
use sb_core::model::UniformSource;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::LazyLock;

/// How often a builtin value changes, which decides the uniform block it lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Frequency {
    /// Once per frame (or less often): member of `sb_Frame` (set 0, binding 0).
    Frame,
    /// Per draw call: member of `sb_Draw` (set 0, binding 1).
    Draw,
}

impl Frequency {
    /// GLSL block name of the uniform block holding values of this frequency.
    pub fn block_name(self) -> &'static str {
        match self {
            Self::Frame => crate::layout::FRAME_BLOCK_NAME,
            Self::Draw => crate::layout::DRAW_BLOCK_NAME,
        }
    }

    /// Binding (in [`crate::layout::UNIFORM_SET`]) of the block holding values of this
    /// frequency.
    pub fn binding(self) -> u32 {
        match self {
            Self::Frame => crate::layout::FRAME_BINDING,
            Self::Draw => crate::layout::DRAW_BINDING,
        }
    }
}

/// Values of [`BuiltinUniform::source`]: who defines the uniform.
pub mod sources {
    /// Documented by OptiFine (`shaders.txt` / `shaders.properties`); also provided by Iris
    /// unless the description says otherwise.
    pub const OPTIFINE: &str = "optifine";
    /// Iris extension.
    pub const IRIS: &str = "iris";
    /// Core-profile (OptiFine 1.17+) per-draw transform names.
    pub const CORE: &str = "core";
    /// Distant Horizons values (provided by Iris when DH is loaded).
    pub const DH: &str = "dh";
    /// ShaderBridge-internal replacement values (fixed-function state).
    pub const SHADERBRIDGE: &str = "shaderbridge";
    /// Values of the Voxy LOD mod (an "other mods" integration: packs read them when
    /// Voxy renders LODs; hosts without Voxy upload zeros).
    pub const VOXY: &str = "voxy";
    /// Every valid source string.
    pub const ALL: [&str; 6] = [OPTIFINE, IRIS, CORE, DH, SHADERBRIDGE, VOXY];
}

/// A builtin uniform the host supplies by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinUniform {
    /// GLSL name, exactly as packs declare it.
    pub name: &'static str,
    /// Canonical GLSL type (Iris type where OptiFine and Iris disagree).
    pub ty: GlslType,
    /// Whether the value lives in `sb_Frame` or `sb_Draw`.
    pub frequency: Frequency,
    /// Short human-readable description (semantics and range).
    pub description: &'static str,
    /// One of [`sources::ALL`]: `optifine`, `iris`, `core`, `dh`, `shaderbridge` or `voxy`.
    pub source: &'static str,
}

const fn frame(
    name: &'static str,
    ty: GlslType,
    source: &'static str,
    description: &'static str,
) -> BuiltinUniform {
    BuiltinUniform {
        name,
        ty,
        frequency: Frequency::Frame,
        description,
        source,
    }
}

const fn draw(
    name: &'static str,
    ty: GlslType,
    source: &'static str,
    description: &'static str,
) -> BuiltinUniform {
    BuiltinUniform {
        name,
        ty,
        frequency: Frequency::Draw,
        description,
        source,
    }
}

use GlslType as T;
use sources::{CORE, DH, IRIS, OPTIFINE, SHADERBRIDGE, VOXY};

static BUILTINS: &[BuiltinUniform] = &[
    // ---------------------------------------------------------------- camera / player
    frame(
        "cameraPosition",
        T::VEC3,
        OPTIFINE,
        "Camera position in world space",
    ),
    frame(
        "previousCameraPosition",
        T::VEC3,
        OPTIFINE,
        "cameraPosition of the previous frame",
    ),
    frame(
        "eyeAltitude",
        T::FLOAT,
        OPTIFINE,
        "Y coordinate of the camera entity's eyes",
    ),
    frame(
        "cameraPositionInt",
        T::IVEC3,
        IRIS,
        "Integer part (floor) of cameraPosition",
    ),
    frame(
        "cameraPositionFract",
        T::VEC3,
        IRIS,
        "Fractional part of cameraPosition (0..1)",
    ),
    frame(
        "previousCameraPositionInt",
        T::IVEC3,
        IRIS,
        "Integer part of previousCameraPosition",
    ),
    frame(
        "previousCameraPositionFract",
        T::VEC3,
        IRIS,
        "Fractional part of previousCameraPosition",
    ),
    frame(
        "eyePosition",
        T::VEC3,
        IRIS,
        "World-space position of the player's eyes",
    ),
    frame(
        "relativeEyePosition",
        T::VEC3,
        IRIS,
        "Player eye position relative to the camera",
    ),
    frame(
        "playerLookVector",
        T::VEC3,
        IRIS,
        "World-space direction the player is looking in",
    ),
    frame(
        "playerBodyVector",
        T::VEC3,
        IRIS,
        "World-space direction of the player's body",
    ),
    frame(
        "vehicleLookVector",
        T::VEC3,
        IRIS,
        "World-space direction the ridden vehicle faces",
    ),
    frame(
        "relativeVehiclePosition",
        T::VEC3,
        IRIS,
        "Ridden vehicle position relative to the camera",
    ),
    frame(
        "upPosition",
        T::VEC3,
        OPTIFINE,
        "Up direction in view space, length 100",
    ),
    frame(
        "eyeBrightness",
        T::IVEC2,
        OPTIFINE,
        "Light at the eyes: x = block, y = sky, 0..240",
    ),
    frame(
        "eyeBrightnessSmooth",
        T::IVEC2,
        OPTIFINE,
        "eyeBrightness smoothed with eyeBrightnessHalflife",
    ),
    frame(
        "centerDepthSmooth",
        T::FLOAT,
        OPTIFINE,
        "Depth at the screen centre, smoothed with centerDepthHalflife",
    ),
    frame(
        "firstPersonCamera",
        T::BOOL,
        IRIS,
        "The camera is in first-person mode",
    ),
    // ---------------------------------------------------------------- player status
    frame(
        "isEyeInWater",
        T::INT,
        OPTIFINE,
        "Camera medium: 0 air, 1 water, 2 lava, 3 powder snow",
    ),
    frame(
        "isSpectator",
        T::BOOL,
        IRIS,
        "The player is in spectator mode",
    ),
    frame(
        "isRightHanded",
        T::BOOL,
        IRIS,
        "The player's main hand is the right hand",
    ),
    frame(
        "blindness",
        T::FLOAT,
        OPTIFINE,
        "Blindness effect strength, 0..1",
    ),
    frame(
        "darknessFactor",
        T::FLOAT,
        OPTIFINE,
        "Darkness effect strength, 0..1 (1.19+)",
    ),
    frame(
        "darknessLightFactor",
        T::FLOAT,
        OPTIFINE,
        "Lightmap pulsing caused by the darkness effect, 0..1 (1.19+)",
    ),
    frame(
        "nightVision",
        T::FLOAT,
        OPTIFINE,
        "Night vision effect strength, 0..1",
    ),
    frame(
        "playerMood",
        T::FLOAT,
        OPTIFINE,
        "Player mood (cave ambience), 0..1",
    ),
    frame(
        "constantMood",
        T::FLOAT,
        IRIS,
        "Player mood without the reset when a cave sound plays, 0..1",
    ),
    frame(
        "currentPlayerHealth",
        T::FLOAT,
        IRIS,
        "Player health, 0..maxPlayerHealth (-1 without a player)",
    ),
    frame("maxPlayerHealth", T::FLOAT, IRIS, "Maximum player health"),
    frame(
        "currentPlayerHunger",
        T::FLOAT,
        IRIS,
        "Player food level, 0..maxPlayerHunger",
    ),
    frame(
        "maxPlayerHunger",
        T::FLOAT,
        IRIS,
        "Maximum player food level (20)",
    ),
    frame("currentPlayerArmor", T::FLOAT, IRIS, "Player armor value"),
    frame(
        "maxPlayerArmor",
        T::FLOAT,
        IRIS,
        "Maximum player armor value (50)",
    ),
    frame("currentPlayerAir", T::FLOAT, IRIS, "Player air supply"),
    frame("maxPlayerAir", T::FLOAT, IRIS, "Maximum player air supply"),
    frame("isRiding", T::BOOL, IRIS, "The player rides an entity"),
    frame(
        "vehicleInWater",
        T::BOOL,
        IRIS,
        "The ridden vehicle is in water",
    ),
    frame(
        "inSwimmingAnimation",
        T::BOOL,
        IRIS,
        "The player is in the swimming pose",
    ),
    frame(
        "feetInWater",
        T::BOOL,
        IRIS,
        "The player's feet are in water",
    ),
    frame(
        "isElytraFlying",
        T::BOOL,
        IRIS,
        "The player is gliding with an elytra",
    ),
    frame("heavyFog", T::BOOL, IRIS, "The current biome has thick fog"),
    frame(
        "is_sneaking",
        T::BOOL,
        IRIS,
        "The player is sneaking (OptiFine: custom-uniform parameter)",
    ),
    frame(
        "is_sprinting",
        T::BOOL,
        IRIS,
        "The player is sprinting (OptiFine: custom-uniform parameter)",
    ),
    frame(
        "is_hurt",
        T::BOOL,
        IRIS,
        "The player is hurt (OptiFine: custom-uniform parameter)",
    ),
    frame(
        "is_invisible",
        T::BOOL,
        IRIS,
        "The player is invisible (OptiFine: custom-uniform parameter)",
    ),
    frame(
        "is_burning",
        T::BOOL,
        IRIS,
        "The player is burning (OptiFine: custom-uniform parameter)",
    ),
    frame(
        "is_on_ground",
        T::BOOL,
        IRIS,
        "The player is on the ground (OptiFine: custom-uniform parameter)",
    ),
    frame(
        "is_alive",
        T::BOOL,
        OPTIFINE,
        "The player is alive (OptiFine-only custom-uniform parameter)",
    ),
    frame(
        "is_child",
        T::BOOL,
        OPTIFINE,
        "The camera entity is a child (OptiFine-only custom-uniform parameter)",
    ),
    frame(
        "is_glowing",
        T::BOOL,
        OPTIFINE,
        "The player is glowing (OptiFine-only custom-uniform parameter)",
    ),
    frame(
        "is_in_lava",
        T::BOOL,
        OPTIFINE,
        "The player is in lava (OptiFine-only custom-uniform parameter)",
    ),
    frame(
        "is_in_water",
        T::BOOL,
        OPTIFINE,
        "The player is in water (OptiFine-only custom-uniform parameter)",
    ),
    frame(
        "is_ridden",
        T::BOOL,
        OPTIFINE,
        "The player is ridden by an entity (OptiFine-only custom-uniform parameter)",
    ),
    frame(
        "is_riding",
        T::BOOL,
        OPTIFINE,
        "The player rides an entity (OptiFine-only custom-uniform parameter)",
    ),
    frame(
        "is_wet",
        T::BOOL,
        OPTIFINE,
        "The player is wet (OptiFine-only custom-uniform parameter)",
    ),
    frame(
        "hideGUI",
        T::BOOL,
        OPTIFINE,
        "The GUI is hidden (F1); bool as in Iris, OptiFine declares it as int",
    ),
    // ---------------------------------------------------------------- system
    frame("viewWidth", T::FLOAT, OPTIFINE, "Viewport width in pixels"),
    frame(
        "viewHeight",
        T::FLOAT,
        OPTIFINE,
        "Viewport height in pixels",
    ),
    frame("aspectRatio", T::FLOAT, OPTIFINE, "viewWidth / viewHeight"),
    frame(
        "screenBrightness",
        T::FLOAT,
        OPTIFINE,
        "Brightness (gamma) option, 0..1",
    ),
    frame(
        "frameCounter",
        T::INT,
        OPTIFINE,
        "Frame index, 0..720719 then wraps",
    ),
    frame(
        "frameTime",
        T::FLOAT,
        OPTIFINE,
        "Duration of the last frame in seconds",
    ),
    frame(
        "frameTimeCounter",
        T::FLOAT,
        OPTIFINE,
        "Run time in seconds, wraps after 3600 s",
    ),
    frame(
        "currentColorSpace",
        T::INT,
        IRIS,
        "Output color space selected in the Iris settings",
    ),
    frame(
        "currentDate",
        T::IVEC3,
        IRIS,
        "Local date: year, month (1..12), day (1..31)",
    ),
    frame(
        "currentTime",
        T::IVEC3,
        IRIS,
        "Local time: hour, minute, second",
    ),
    frame(
        "currentYearTime",
        T::IVEC2,
        IRIS,
        "Seconds since the start of the year, seconds until its end",
    ),
    frame(
        "textureFilteringMode",
        T::INT,
        IRIS,
        "Texture filtering option (TEXTURE_FILTERING feature)",
    ),
    frame(
        "anisotropicFiltering",
        T::INT,
        IRIS,
        "Anisotropic filtering level, 0 when disabled",
    ),
    // ---------------------------------------------------------------- ids
    frame(
        "heldItemId",
        T::INT,
        OPTIFINE,
        "item.properties id of the main-hand item (-1 if unmapped)",
    ),
    frame(
        "heldItemId2",
        T::INT,
        OPTIFINE,
        "item.properties id of the off-hand item (-1 if unmapped)",
    ),
    frame(
        "heldBlockLightValue",
        T::INT,
        OPTIFINE,
        "Light emitted by the main-hand item, 0..15",
    ),
    frame(
        "heldBlockLightValue2",
        T::INT,
        OPTIFINE,
        "Light emitted by the off-hand item, 0..15",
    ),
    frame(
        "heldBlockLightColor",
        T::VEC3,
        IRIS,
        "Light color of the main-hand item",
    ),
    frame(
        "heldBlockLightColor2",
        T::VEC3,
        IRIS,
        "Light color of the off-hand item",
    ),
    draw(
        "entityId",
        T::INT,
        OPTIFINE,
        "entity.properties id of the entity being drawn",
    ),
    draw(
        "blockEntityId",
        T::INT,
        OPTIFINE,
        "block.properties id of the block entity being drawn",
    ),
    draw(
        "currentRenderedItemId",
        T::INT,
        IRIS,
        "item.properties id of the item being drawn",
    ),
    frame(
        "vehicleId",
        T::INT,
        IRIS,
        "entity.properties id of the ridden vehicle",
    ),
    frame(
        "currentSelectedBlockId",
        T::INT,
        IRIS,
        "block.properties id of the block under the crosshair",
    ),
    frame(
        "currentSelectedBlockPos",
        T::VEC3,
        IRIS,
        "Camera-relative position of the block under the crosshair",
    ),
    // ---------------------------------------------------------------- world
    frame(
        "sunPosition",
        T::VEC3,
        OPTIFINE,
        "Sun position in view space, length 100",
    ),
    frame(
        "moonPosition",
        T::VEC3,
        OPTIFINE,
        "Moon position in view space, length 100",
    ),
    frame(
        "shadowLightPosition",
        T::VEC3,
        OPTIFINE,
        "Sun or moon (whichever casts shadows) in view space, length 100",
    ),
    frame(
        "sunAngle",
        T::FLOAT,
        OPTIFINE,
        "Sun angle, 0..1 (0.25 = noon)",
    ),
    frame(
        "shadowAngle",
        T::FLOAT,
        OPTIFINE,
        "Shadow light angle, 0..0.5",
    ),
    frame("moonPhase", T::INT, OPTIFINE, "Moon phase, 0..7"),
    frame(
        "worldTime",
        T::INT,
        OPTIFINE,
        "World time in ticks, 0..23999",
    ),
    frame(
        "worldDay",
        T::INT,
        OPTIFINE,
        "World day (world ticks / 24000)",
    ),
    frame("rainStrength", T::FLOAT, OPTIFINE, "Rain strength, 0..1"),
    frame(
        "wetness",
        T::FLOAT,
        OPTIFINE,
        "rainStrength smoothed with wetnessHalflife/drynessHalflife",
    ),
    frame(
        "thunderStrength",
        T::FLOAT,
        IRIS,
        "Thunderstorm strength, 0..1",
    ),
    frame(
        "lightningBoltPosition",
        T::VEC4,
        IRIS,
        "Camera-relative position of the active lightning bolt (w = 1 when present)",
    ),
    frame(
        "endFlashPosition",
        T::VEC3,
        IRIS,
        "View-space direction of the End flash",
    ),
    frame(
        "endFlashIntensity",
        T::FLOAT,
        IRIS,
        "End flash intensity, 0..1",
    ),
    frame(
        "previousEndFlashIntensity",
        T::FLOAT,
        IRIS,
        "endFlashIntensity of the previous frame",
    ),
    frame("cloudTime", T::FLOAT, IRIS, "Vanilla cloud animation time"),
    frame(
        "cloudHeight",
        T::FLOAT,
        IRIS,
        "Cloud height of the dimension",
    ),
    frame(
        "ambientLight",
        T::FLOAT,
        IRIS,
        "Ambient light level of the dimension",
    ),
    frame(
        "bedrockLevel",
        T::INT,
        IRIS,
        "Lowest block Y of the dimension",
    ),
    frame(
        "heightLimit",
        T::INT,
        IRIS,
        "Height of the dimension in blocks",
    ),
    frame(
        "logicalHeightLimit",
        T::INT,
        IRIS,
        "Logical height of the dimension (portals, chorus)",
    ),
    frame("seaLevel", T::INT, IRIS, "Sea level of the dimension"),
    frame(
        "hasCeiling",
        T::BOOL,
        IRIS,
        "The dimension has a ceiling (nether)",
    ),
    frame("hasSkylight", T::BOOL, IRIS, "The dimension has skylight"),
    frame(
        "bossBattle",
        T::INT,
        OPTIFINE,
        "Boss bar: 1 custom, 2 ender dragon, 3 wither, 4 raid (OptiFine only)",
    ),
    // ---------------------------------------------------------------- biome
    frame(
        "biome",
        T::INT,
        IRIS,
        "Biome id (BIOME_* constants); OptiFine: float custom-uniform parameter",
    ),
    frame(
        "biome_category",
        T::INT,
        IRIS,
        "Biome category (CAT_* constants)",
    ),
    frame(
        "biome_precipitation",
        T::INT,
        IRIS,
        "Precipitation: 0 none, 1 rain, 2 snow (PPT_* constants)",
    ),
    frame(
        "rainfall",
        T::FLOAT,
        IRIS,
        "Biome downfall (humidity), 0..1",
    ),
    frame("temperature", T::FLOAT, IRIS, "Biome temperature"),
    // ---------------------------------------------------------------- rendering
    frame("near", T::FLOAT, OPTIFINE, "Near plane distance (0.05)"),
    frame(
        "far",
        T::FLOAT,
        OPTIFINE,
        "Far plane distance (render distance)",
    ),
    frame("fogColor", T::VEC3, OPTIFINE, "Fog color"),
    frame("skyColor", T::VEC3, OPTIFINE, "Sky color"),
    frame(
        "fogMode",
        T::INT,
        OPTIFINE,
        "Fog mode: GL_LINEAR (9729), GL_EXP (2048), GL_EXP2 (2049), 0 = off",
    ),
    frame(
        "fogShape",
        T::INT,
        OPTIFINE,
        "Fog shape: 0 sphere, 1 cylinder",
    ),
    frame("fogDensity", T::FLOAT, OPTIFINE, "Fog density, 0..1"),
    frame(
        "fogStart",
        T::FLOAT,
        OPTIFINE,
        "Fog start distance in blocks",
    ),
    frame("fogEnd", T::FLOAT, OPTIFINE, "Fog end distance in blocks"),
    draw(
        "alphaTestRef",
        T::FLOAT,
        OPTIFINE,
        "Alpha test reference of the current draw",
    ),
    draw(
        "entityColor",
        T::VEC4,
        OPTIFINE,
        "Entity tint (hurt flash, creeper swelling): rgb color, a = strength",
    ),
    draw(
        "blendFunc",
        T::IVEC4,
        OPTIFINE,
        "Current blend factors: srcRGB, dstRGB, srcAlpha, dstAlpha (GL enums)",
    ),
    draw(
        "atlasSize",
        T::IVEC2,
        OPTIFINE,
        "Size of the bound texture atlas in pixels (0 when no atlas is bound)",
    ),
    draw(
        "gtextureSize",
        T::IVEC2,
        IRIS,
        "Size of the texture bound as gtexture in pixels",
    ),
    draw(
        "gtextureId",
        T::INT,
        IRIS,
        "Identifier of the texture bound as gtexture",
    ),
    frame(
        "textureReloadCount",
        T::INT,
        IRIS,
        "Number of resource reloads",
    ),
    draw(
        "renderStage",
        T::INT,
        OPTIFINE,
        "Render stage of the current draw (MC_RENDER_STAGE_* constants)",
    ),
    frame(
        "chunkFadeTimeInv",
        T::FLOAT,
        IRIS,
        "1 / chunk fade-in time in seconds",
    ),
    frame("pi", T::FLOAT, IRIS, "The constant 3.14159265"),
    draw(
        "spriteBounds",
        T::VEC4,
        OPTIFINE,
        "Atlas sprite bounds (u0, v0, u1, v1) of the current draw (OptiFine only)",
    ),
    draw(
        "instanceId",
        T::INT,
        OPTIFINE,
        "Instance index when countInstances > 1, 0 = original (OptiFine only)",
    ),
    frame(
        "terrainTextureSize",
        T::IVEC2,
        OPTIFINE,
        "Terrain atlas size (OptiFine only, documented as unused)",
    ),
    frame(
        "terrainIconSize",
        T::INT,
        OPTIFINE,
        "Terrain atlas icon size (OptiFine only, documented as unused)",
    ),
    // ---------------------------------------------------------------- matrices
    frame(
        "gbufferModelView",
        T::MAT4,
        OPTIFINE,
        "Camera model-view matrix (world-relative-to-camera to view space)",
    ),
    frame(
        "gbufferModelViewInverse",
        T::MAT4,
        OPTIFINE,
        "Inverse of gbufferModelView",
    ),
    frame(
        "gbufferPreviousModelView",
        T::MAT4,
        OPTIFINE,
        "gbufferModelView of the previous frame",
    ),
    frame(
        "gbufferProjection",
        T::MAT4,
        OPTIFINE,
        "Camera projection matrix (GL convention, NDC z in -1..1)",
    ),
    frame(
        "gbufferProjectionInverse",
        T::MAT4,
        OPTIFINE,
        "Inverse of gbufferProjection",
    ),
    frame(
        "gbufferPreviousProjection",
        T::MAT4,
        OPTIFINE,
        "gbufferProjection of the previous frame",
    ),
    frame(
        "shadowModelView",
        T::MAT4,
        OPTIFINE,
        "Shadow pass model-view matrix",
    ),
    frame(
        "shadowModelViewInverse",
        T::MAT4,
        OPTIFINE,
        "Inverse of shadowModelView",
    ),
    frame(
        "shadowProjection",
        T::MAT4,
        OPTIFINE,
        "Shadow pass projection matrix",
    ),
    frame(
        "shadowProjectionInverse",
        T::MAT4,
        OPTIFINE,
        "Inverse of shadowProjection",
    ),
    frame(
        "dhProjection",
        T::MAT4,
        DH,
        "Distant Horizons projection matrix",
    ),
    frame(
        "dhProjectionInverse",
        T::MAT4,
        DH,
        "Inverse of dhProjection",
    ),
    frame(
        "dhPreviousProjection",
        T::MAT4,
        DH,
        "dhProjection of the previous frame",
    ),
    frame(
        "dhNearPlane",
        T::FLOAT,
        DH,
        "Distant Horizons near plane distance",
    ),
    frame(
        "dhFarPlane",
        T::FLOAT,
        DH,
        "Distant Horizons far plane distance",
    ),
    frame(
        "dhRenderDistance",
        T::INT,
        DH,
        "Distant Horizons render distance in blocks",
    ),
    // ------------------------------------------------------------------ Voxy LOD mod
    // Names as the corpus packs declare them (e.g. glimmer's custom uniform
    // `combinedFar = vxRenderDistance`, Complementary's `uniform mat4 vxProjInv`).
    frame(
        "vxRenderDistance",
        T::INT,
        VOXY,
        "Voxy LOD render distance (Voxy sets it; 0 without Voxy)",
    ),
    frame("vxProj", T::MAT4, VOXY, "Voxy LOD projection matrix"),
    frame("vxProjInv", T::MAT4, VOXY, "Inverse of vxProj"),
    frame("vxProjPrev", T::MAT4, VOXY, "vxProj of the previous frame"),
    frame("vxModelView", T::MAT4, VOXY, "Voxy LOD model-view matrix"),
    frame("vxModelViewInv", T::MAT4, VOXY, "Inverse of vxModelView"),
    frame("vxModelViewPrev", T::MAT4, VOXY, "vxModelView of the previous frame"),
    // ---------------------------------------------------------------- core-profile names
    draw(
        "modelViewMatrix",
        T::MAT4,
        CORE,
        "Model-view matrix of the current draw",
    ),
    draw(
        "modelViewMatrixInverse",
        T::MAT4,
        CORE,
        "Inverse of modelViewMatrix",
    ),
    draw(
        "projectionMatrix",
        T::MAT4,
        CORE,
        "Projection matrix of the current draw",
    ),
    draw(
        "projectionMatrixInverse",
        T::MAT4,
        CORE,
        "Inverse of projectionMatrix",
    ),
    draw(
        "normalMatrix",
        T::MAT3,
        CORE,
        "Normal matrix of the current draw",
    ),
    draw(
        "textureMatrix",
        T::MAT4,
        CORE,
        "Texture matrix of the current draw (identity by default)",
    ),
    draw(
        "colorModulator",
        T::VEC4,
        CORE,
        "Color modulator of the current draw",
    ),
    draw(
        "chunkOffset",
        T::VEC3,
        CORE,
        "Chunk offset added to vaPosition (pre-1.21.2 name of modelOffset)",
    ),
    draw(
        "modelOffset",
        T::VEC3,
        CORE,
        "Model offset added to vaPosition (terrain chunks, clouds, DH)",
    ),
    // ---------------------------------------------------------------- ShaderBridge (gl_Fog)
    frame(
        "sb_FogColor",
        T::VEC4,
        SHADERBRIDGE,
        "gl_Fog.color replacement: fogColor with alpha",
    ),
    frame(
        "fogScale",
        T::FLOAT,
        SHADERBRIDGE,
        "gl_Fog.scale replacement: 1 / (fogEnd - fogStart)",
    ),
];

static BY_NAME: LazyLock<HashMap<&'static str, &'static BuiltinUniform>> =
    LazyLock::new(|| BUILTINS.iter().map(|b| (b.name, b)).collect());

/// Builtins that change per draw or describe fixed-function state, which OptiFine and
/// Iris do not offer as custom-uniform inputs although they are `sb_Frame` members.
const NOT_CUSTOM_INPUTS: &[&str] = &[
    "fogColor",
    "fogMode",
    "fogShape",
    "fogDensity",
    "fogStart",
    "fogEnd",
    "textureReloadCount",
    "sb_FogColor",
    "fogScale",
];

/// Every builtin uniform, in a stable order (grouped by topic).
pub fn all() -> &'static [BuiltinUniform] {
    BUILTINS
}

/// Look up a builtin uniform by its exact GLSL name.
pub fn get(name: &str) -> Option<&'static BuiltinUniform> {
    BY_NAME.get(name).copied()
}

/// Whether `name` is a builtin uniform.
pub fn is_builtin(name: &str) -> bool {
    BY_NAME.contains_key(name)
}

/// The block a uniform named `name` belongs to: the registry frequency for builtins,
/// [`Frequency::Frame`] for every other name (custom uniforms and unknown names, which
/// are [`UniformSource::Unset`]).
pub fn frequency_of(name: &str) -> Frequency {
    get(name).map_or(Frequency::Frame, |b| b.frequency)
}

/// Default source of a loose uniform named `name`: [`UniformSource::Builtin`] for
/// registry names, [`UniformSource::Unset`] otherwise. Custom uniforms are the caller's
/// business (they take precedence over builtins of the same name).
pub fn source_of(name: &str) -> UniformSource {
    if is_builtin(name) {
        UniformSource::Builtin(name.to_string())
    } else {
        UniformSource::Unset
    }
}

/// Type of a builtin usable as an input of custom-uniform expressions
/// (`uniform.<type>.<name>=<expr>` in `shaders.properties`), or `None`.
///
/// Every per-frame builtin is an input, including the OptiFine-only boolean parameters
/// (`is_alive`, `is_wet`, ...). Per-draw values (`entityId`, `entityColor`,
/// `blockEntityId`, ...) and the fog state (`fogColor`, `fogMode`, ...) are not, as in
/// OptiFine and Iris ("dynamic uniforms can not be used as parameters").
pub fn custom_uniform_input_type(name: &str) -> Option<GlslType> {
    let b = get(name)?;
    (b.frequency == Frequency::Frame && !NOT_CUSTOM_INPUTS.contains(&name)).then_some(b.ty)
}

/// Whether a pack declaration of type `declared` can receive the builtin value of type
/// `builtin`.
///
/// Iris disables a uniform whose declared GL type does not match the provided type
/// (`ProgramUniforms`: "Wrong uniform type ... Disabling that uniform"), except that GL
/// `bool` and `int` scalars are both set through `glUniform1i`. So the types must be equal,
/// or both non-array `int`/`bool` scalars.
pub fn is_type_compatible(builtin: GlslType, declared: GlslType) -> bool {
    if builtin == declared {
        return true;
    }
    let int_like = |t: GlslType| {
        t.is_scalar() && t.array.is_none() && matches!(t.scalar, ScalarKind::Int | ScalarKind::Bool)
    };
    int_like(builtin) && int_like(declared)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn is_identifier(s: &str) -> bool {
        let mut c = s.chars();
        matches!(c.next(), Some(f) if f.is_ascii_alphabetic() || f == '_')
            && c.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    }

    /// Cross-crate item 6: the Voxy LOD uniforms (as packs declare them) are per-frame
    /// builtins and usable in custom-uniform expressions.
    #[test]
    fn voxy_uniforms() {
        let rd = get("vxRenderDistance").unwrap();
        assert_eq!((rd.ty, rd.frequency, rd.source), (GlslType::INT, Frequency::Frame, sources::VOXY));
        for name in ["vxProj", "vxProjInv", "vxProjPrev", "vxModelView", "vxModelViewInv", "vxModelViewPrev"] {
            let b = get(name).unwrap();
            assert_eq!((b.ty, b.frequency, b.source), (GlslType::MAT4, Frequency::Frame, sources::VOXY), "{name}");
        }
        assert_eq!(custom_uniform_input_type("vxRenderDistance"), Some(GlslType::INT));
        assert_eq!(source_of("vxProjInv"), UniformSource::Builtin("vxProjInv".into()));
        // Samplers are resources, not loose uniforms.
        assert!(get("vxDepthTexOpaque").is_none());
    }

    #[test]
    fn no_duplicates_and_well_formed() {
        let mut seen = HashSet::new();
        for b in all() {
            assert!(seen.insert(b.name), "duplicate builtin {}", b.name);
            assert!(is_identifier(b.name), "{} is not an identifier", b.name);
            assert!(!b.name.starts_with("gl_"), "{}", b.name);
            assert!(!b.description.is_empty(), "{} has no description", b.name);
            assert!(
                sources::ALL.contains(&b.source),
                "{} has source {}",
                b.name,
                b.source
            );
            assert!(
                (1..=4).contains(&b.ty.rows) && (1..=4).contains(&b.ty.cols),
                "{}",
                b.name
            );
            assert!(b.ty.array.is_none(), "{} is an array", b.name);
            assert_eq!(get(b.name), Some(b));
            if b.name.starts_with(sb_core::RESERVED_PREFIX) {
                assert_eq!(b.source, sources::SHADERBRIDGE, "{}", b.name);
            }
        }
        assert!(
            all().len() > 150,
            "registry has only {} entries",
            all().len()
        );
    }

    #[test]
    fn lookup() {
        assert_eq!(get("frameTimeCounter").map(|b| b.ty), Some(GlslType::FLOAT));
        assert_eq!(get("eyeBrightness").map(|b| b.ty), Some(GlslType::IVEC2));
        assert_eq!(get("hideGUI").map(|b| b.ty), Some(GlslType::BOOL));
        assert_eq!(get("normalMatrix").map(|b| b.ty), Some(GlslType::MAT3));
        assert!(get("nope").is_none());
        assert!(get("").is_none());
        assert!(
            get("FrameTimeCounter").is_none(),
            "lookup is case-sensitive"
        );
        assert!(is_builtin("pi"));
        assert!(!is_builtin("iris_FogColor"));
    }

    #[test]
    fn draw_frequency_set() {
        let draw: HashSet<&str> = all()
            .iter()
            .filter(|b| b.frequency == Frequency::Draw)
            .map(|b| b.name)
            .collect();
        let expected: HashSet<&str> = [
            "entityId",
            "blockEntityId",
            "currentRenderedItemId",
            "entityColor",
            "alphaTestRef",
            "blendFunc",
            "renderStage",
            "modelViewMatrix",
            "modelViewMatrixInverse",
            "projectionMatrix",
            "projectionMatrixInverse",
            "normalMatrix",
            "textureMatrix",
            "colorModulator",
            "chunkOffset",
            "modelOffset",
            "atlasSize",
            "gtextureSize",
            "gtextureId",
            // OptiFine-only per-draw values.
            "spriteBounds",
            "instanceId",
        ]
        .into_iter()
        .collect();
        assert_eq!(draw, expected);
    }

    #[test]
    fn frequencies_and_sources() {
        assert_eq!(frequency_of("entityId"), Frequency::Draw);
        assert_eq!(frequency_of("alphaTestRef"), Frequency::Draw);
        assert_eq!(frequency_of("worldTime"), Frequency::Frame);
        assert_eq!(frequency_of("myCustomThing"), Frequency::Frame);
        assert_eq!(
            source_of("worldTime"),
            UniformSource::Builtin("worldTime".into())
        );
        assert_eq!(source_of("myCustomThing"), UniformSource::Unset);
        assert_eq!(Frequency::Frame.block_name(), "sb_Frame");
        assert_eq!(Frequency::Draw.block_name(), "sb_Draw");
        assert_eq!(Frequency::Frame.binding(), 0);
        assert_eq!(Frequency::Draw.binding(), 1);
    }

    #[test]
    fn custom_uniform_inputs() {
        assert_eq!(custom_uniform_input_type("is_alive"), Some(GlslType::BOOL));
        assert_eq!(custom_uniform_input_type("is_wet"), Some(GlslType::BOOL));
        assert_eq!(custom_uniform_input_type("biome"), Some(GlslType::INT));
        assert_eq!(custom_uniform_input_type("rainfall"), Some(GlslType::FLOAT));
        assert_eq!(custom_uniform_input_type("worldTime"), Some(GlslType::INT));
        assert_eq!(
            custom_uniform_input_type("gbufferModelView"),
            Some(GlslType::MAT4)
        );
        assert_eq!(
            custom_uniform_input_type("dhRenderDistance"),
            Some(GlslType::INT)
        );
        assert_eq!(
            custom_uniform_input_type("sunPosition"),
            Some(GlslType::VEC3)
        );
        for dynamic in [
            "entityId",
            "entityColor",
            "blockEntityId",
            "fogMode",
            "fogColor",
            "chunkOffset",
            "alphaTestRef",
        ] {
            assert_eq!(custom_uniform_input_type(dynamic), None, "{dynamic}");
        }
        assert_eq!(custom_uniform_input_type("unknownThing"), None);
        // Every input type is representable by the expression language (scalars,
        // vectors and mat4).
        for b in all() {
            if let Some(t) = custom_uniform_input_type(b.name) {
                assert!(t.cols == 1 || t == GlslType::MAT4, "{} {t}", b.name);
            }
        }
    }

    #[test]
    fn type_compatibility() {
        assert!(is_type_compatible(GlslType::INT, GlslType::INT));
        assert!(is_type_compatible(GlslType::BOOL, GlslType::INT));
        assert!(is_type_compatible(GlslType::INT, GlslType::BOOL));
        assert!(!is_type_compatible(GlslType::INT, GlslType::FLOAT));
        assert!(!is_type_compatible(GlslType::INT, GlslType::UINT));
        assert!(!is_type_compatible(GlslType::IVEC2, GlslType::VEC2));
        assert!(!is_type_compatible(GlslType::IVEC2, GlslType::BVEC2));
        assert!(!is_type_compatible(
            GlslType::INT,
            GlslType::INT.with_array(1)
        ));
        assert!(!is_type_compatible(GlslType::MAT4, GlslType::MAT3));
    }
}
