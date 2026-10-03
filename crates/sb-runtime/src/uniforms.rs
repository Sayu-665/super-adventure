//! Uniform values: the per-frame state derived from the scene, a provider for every
//! builtin uniform of the sb-uniforms registry (`sb_Frame` and `sb_Draw` members), the
//! draw-profile host blocks (`Globals`, `TerrainUniform`, `DynamicTransforms`, DH blocks),
//! and std140 block filling (including `uniform T x = init;` defaults).

use crate::math::{self, Mat4, Vec3, celestial, shadow};
use crate::scene::SceneParams;
use sb_core::model::{BlockLayout, PackSettings, ShadowSettings, UniformSource};
use sb_core::{GlslType, ScalarKind};
use sb_expr::{Value, write_value};

/// Near plane of `gbufferProjection` (Minecraft's 0.05).
pub(crate) const NEAR: f64 = 0.05;
/// Minecraft's default cloud range (128 chunks) in blocks: a lower bound of the level
/// projection's far plane.
pub(crate) const DEFAULT_CLOUD_RANGE_BLOCKS: f64 = 128.0 * 16.0;
/// Near plane of `dhProjection`.
pub(crate) const DH_NEAR: f64 = 0.1;
/// `frameCounter` wraps at this value (as in Iris).
const FRAME_COUNTER_WRAP: u32 = 720_720;
/// The world's sea level as Minecraft reports it.
const SEA_LEVEL: i32 = 63;

/// Vanilla biomes in registry order (data-driven registries are sorted by key), used for
/// the `BIOME_*` expression constants and the `biome` uniform.
pub(crate) const BIOMES: [&str; 65] = [
    "badlands", "bamboo_jungle", "basalt_deltas", "beach", "birch_forest", "cherry_grove", "cold_ocean", "crimson_forest", "dark_forest",
    "deep_cold_ocean", "deep_dark", "deep_frozen_ocean", "deep_lukewarm_ocean", "deep_ocean", "desert", "dripstone_caves", "end_barrens",
    "end_highlands", "end_midlands", "eroded_badlands", "flower_forest", "forest", "frozen_ocean", "frozen_peaks", "frozen_river", "grove",
    "ice_spikes", "jagged_peaks", "jungle", "lukewarm_ocean", "lush_caves", "mangrove_swamp", "meadow", "mushroom_fields", "nether_wastes",
    "ocean", "old_growth_birch_forest", "old_growth_pine_taiga", "old_growth_spruce_taiga", "pale_garden", "plains", "river", "savanna",
    "savanna_plateau", "small_end_islands", "snowy_beach", "snowy_plains", "snowy_slopes", "snowy_taiga", "soul_sand_valley", "sparse_jungle",
    "stony_peaks", "stony_shore", "sunflower_plains", "swamp", "taiga", "the_end", "the_void", "warm_ocean", "warped_forest", "windswept_forest",
    "windswept_gravelly_hills", "windswept_hills", "windswept_savanna", "wooded_badlands",
];

/// Inputs that determine a frame's uniform values.
#[derive(Debug, Clone)]
pub(crate) struct FrameInputs<'a> {
    pub scene: &'a SceneParams,
    pub camera: Vec3,
    pub width: u32,
    pub height: u32,
    pub frame: u32,
    pub settings: &'a PackSettings,
    pub shadow: &'a ShadowSettings,
    /// DH LODs are rendered.
    pub dh: bool,
    /// One projection for vanilla terrain and LODs (synthesized DH).
    pub unified_projection: bool,
    /// GL depth at the screen centre measured in the previous frame.
    pub center_depth: f32,
}

/// Everything the host computes once per frame.
#[derive(Debug, Clone)]
pub(crate) struct FrameState {
    pub frame_counter: u32,
    pub frame_time: f32,
    pub frame_time_counter: f32,
    pub world_time: i64,
    pub world_day: i64,
    pub width: f32,
    pub height: f32,
    pub camera: Vec3,
    pub look: Vec3,
    pub model_view: Mat4,
    pub projection: Mat4,
    pub shadow_model_view: Mat4,
    pub shadow_projection: Mat4,
    pub dh_projection: Mat4,
    pub near: f64,
    pub far: f64,
    pub dh_near: f64,
    pub dh_far: f64,
    pub dh_render_distance: i32,
    pub sky_angle: f64,
    pub sun_angle: f64,
    pub shadow_angle: f64,
    pub sun_position: Vec3,
    pub moon_position: Vec3,
    pub shadow_light_position: Vec3,
    pub up_position: Vec3,
    pub fog_color: [f32; 3],
    pub sky_color: [f32; 3],
    /// Environmental fog (`FogData.environmentalStart/End`): Iris' `fogStart` / `fogEnd`
    /// and `gl_Fog.start` / `.end`. See [`Fog`].
    pub fog_start: f32,
    pub fog_end: f32,
    /// Render-distance fog (`FogData.renderDistanceStart/End`).
    pub render_fog_start: f32,
    pub render_fog_end: f32,
    /// `FogData.skyEnd` / `cloudEnd`.
    pub sky_fog_end: f32,
    pub cloud_fog_end: f32,
    pub rain: f32,
    pub thunder: f32,
    pub moon_phase: i32,
    pub center_depth: f32,
    pub sun_path_rotation: f64,
}

/// Minecraft 26.3's fog distances (`FogRenderer.setupFog` with the overworld's
/// `AtmosphericFogEnvironment`) for a camera in the open.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Fog {
    pub environmental: (f32, f32),
    pub render_distance: (f32, f32),
    pub sky_end: f32,
    pub cloud_end: f32,
}

/// What Distant Horizons writes into `FogData` when it disables vanilla fog
/// (`MixinFogRenderer`: `A_REALLY_REALLY_BIG_VALUE` / `A_EVEN_LARGER_VALUE`).
pub(crate) const DH_NO_FOG: (f32, f32) = (420_694_206_942_069.0, 42_069_420_694_206_942_069.0);

impl Fog {
    /// * environmental fog: the `visual/fog_start_distance` / `fog_end_distance`
    ///   attribute defaults (0 and 1024 blocks; the overworld does not override them),
    ///   pulled in by rain (`-160` / `-256` blocks at full rain, the end never below 96);
    /// * render-distance fog: the last `clamp(rd / 10, 4, 64)` blocks of the render
    ///   distance;
    /// * sky / cloud fog ends: `min(rd, 512)` and the 2048-block default cloud range.
    ///
    /// Distant Horizons (`fog.enableVanillaFog = false`, its default) replaces the
    /// environmental and render-distance distances with huge values while it renders, so
    /// packs see no vanilla fog in front of the LODs (Iris reads the same `FogData`).
    pub fn new(render_distance_blocks: f32, rain: f32, distant_horizons: bool) -> Self {
        let rain = if rain.is_finite() { rain.clamp(0.0, 1.0) } else { 0.0 };
        let span = (render_distance_blocks / 10.0).clamp(4.0, 64.0);
        let (environmental, render_distance) = if distant_horizons {
            (DH_NO_FOG, DH_NO_FOG)
        } else {
            ((-160.0 * rain, (1024.0 - 256.0 * rain).max(96.0)), (render_distance_blocks - span, render_distance_blocks))
        };
        Self { environmental, render_distance, sky_end: render_distance_blocks.min(512.0), cloud_end: 2048.0 }
    }
}

fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

impl FrameState {
    /// Compute the frame state. The camera is static, so the previous-frame values equal
    /// the current ones.
    pub fn new(inp: &FrameInputs<'_>) -> Self {
        let s = inp.scene;
        let width = inp.width.max(1) as f64;
        let height = inp.height.max(1) as f64;
        let rd_blocks = f64::from(s.clamped_render_distance() * 16);
        let dh_chunks = f64::from(s.clamped_dh_distance());
        let dh_far = (dh_chunks * 16.0 + 512.0) * std::f64::consts::SQRT_2;
        let unified = inp.dh && inp.unified_projection;
        // `far` is the render distance in blocks (Iris `CameraUniforms`), while the level
        // projection reaches Minecraft's `Camera.depthFar`; the unified DH projection uses
        // the DH far plane for both (ARCHITECTURE §9).
        let far = if unified { dh_far } else { rd_blocks };
        let projection_far = if unified { dh_far } else { mc_depth_far(rd_blocks) };
        let fov = if s.fov.is_finite() { s.fov.clamp(10.0, 170.0) } else { 70.0 };
        let yaw = if s.yaw.is_finite() { s.yaw } else { 0.0 };
        let pitch = if s.pitch.is_finite() { s.pitch.clamp(-90.0, 90.0) } else { 0.0 };
        let model_view = math::mc_view_rotation(yaw, pitch);
        let aspect = width / height;
        let projection = math::perspective_gl(fov, aspect, NEAR, projection_far);
        let (dh_near, dh_projection) = if unified { (NEAR, projection) } else { (DH_NEAR, math::perspective_gl(fov, aspect, DH_NEAR, dh_far)) };

        let world_time = s.world_time.rem_euclid(24000);
        let world_day = s.world_time.div_euclid(24000);
        let sky_angle = celestial::sky_angle(world_time);
        let sun_angle = celestial::sun_angle(sky_angle);
        let shadow_angle = celestial::shadow_angle(sun_angle);
        let spr = f64::from(inp.settings.sun_path_rotation);
        let sun_position = celestial::position(&model_view, sky_angle, spr, 100.0);
        let moon_position = celestial::position(&model_view, sky_angle, spr, -100.0);
        let shadow_light_position = if celestial::is_day(sun_angle) { sun_position } else { moon_position };
        let up_position = celestial::up_position(&model_view);

        let sh = inp.shadow;
        let shadow_model_view = shadow::model_view(shadow_angle, spr, f64::from(sh.interval_size), inp.camera);
        let shadow_projection = match sh.fov {
            Some(fov) if fov.is_finite() && fov > 0.0 => shadow::perspective(f64::from(fov)),
            _ => {
                let (n, f) = shadow_planes(sh, shadow_plane_distance(inp.dh, dh_chunks, rd_blocks));
                shadow::ortho(f64::from(sh.distance), n, f)
            }
        };

        // Daylight: 1 at noon, 0 at midnight (vanilla sky brightness curve).
        let daylight = ((sky_angle * std::f64::consts::TAU).cos() as f32 * 2.0 + 0.5).clamp(0.0, 1.0);
        let rain = s.rain.clamp(0.0, 1.0);
        let thunder = s.thunder.clamp(0.0, 1.0);
        let dim = 1.0 - rain * 0.5;
        let sky_color = mix3([0.0, 0.0, 0.02], [0.47, 0.65, 1.0], daylight).map(|c| c * dim);
        let fog_color = mix3([0.02, 0.02, 0.05], [0.75, 0.85, 1.0], daylight).map(|c| c * dim);
        let frame_time = if s.frame_time.is_finite() && s.frame_time > 0.0 { s.frame_time.min(1.0) } else { 1.0 / 60.0 };
        let fog = Fog::new(rd_blocks as f32, rain, inp.dh);
        Self {
            frame_counter: inp.frame % FRAME_COUNTER_WRAP,
            frame_time,
            frame_time_counter: (inp.frame as f32 * frame_time) % 3600.0,
            world_time,
            world_day,
            width: width as f32,
            height: height as f32,
            camera: inp.camera,
            look: math::mc_look_vector(yaw, pitch),
            model_view,
            projection,
            shadow_model_view,
            shadow_projection,
            dh_projection,
            near: NEAR,
            far,
            dh_near,
            dh_far: if unified { far } else { dh_far },
            dh_render_distance: (dh_chunks * 16.0) as i32,
            sky_angle,
            sun_angle,
            shadow_angle,
            sun_position,
            moon_position,
            shadow_light_position,
            up_position,
            fog_color,
            sky_color,
            fog_start: fog.environmental.0,
            fog_end: fog.environmental.1,
            render_fog_start: fog.render_distance.0,
            render_fog_end: fog.render_distance.1,
            sky_fog_end: fog.sky_end,
            cloud_fog_end: fog.cloud_end,
            rain,
            thunder,
            moon_phase: world_day.rem_euclid(8) as i32,
            center_depth: if inp.center_depth.is_finite() { inp.center_depth.clamp(0.0, 1.0) } else { 1.0 },
            sun_path_rotation: spr,
        }
    }

    /// `gbufferModelViewInverse`.
    pub fn model_view_inverse(&self) -> Mat4 {
        self.model_view.inverse_or_identity()
    }
}

/// Minecraft 26.3's level projection far plane (`Camera.depthFar`): four times the
/// render distance, at least the cloud range (default 128 chunks).
pub(crate) fn mc_depth_far(render_distance_blocks: f64) -> f64 {
    (render_distance_blocks * 4.0).max(DEFAULT_CLOUD_RANGE_BLOCKS)
}

/// The distance a `-1` shadow plane stands for, as Iris 26.3's `ShadowRenderer` and
/// `MatrixUniforms` compute it: `DHCompat.getRenderDistance() * 16`, where
/// `getRenderDistance()` is the vanilla render distance in *chunks* without DH but the DH
/// distance already in *blocks* while DH renders, so Iris scales the DH distance by 16
/// twice. Packs tuned on Iris see that range; it is reproduced here.
pub(crate) fn shadow_plane_distance(dh: bool, dh_chunks: f64, render_distance_blocks: f64) -> f64 {
    if dh { dh_chunks * 16.0 * 16.0 } else { render_distance_blocks }
}

/// The shadow ortho near/far planes: Iris 26.3 defaults (`-100.05` / `156`) when the
/// model carries the documented OptiFine defaults `0.05` / `256` (which assumed the old
/// light placement 100 blocks away), `-1` = `-distance` / `+distance` (see
/// [`shadow_plane_distance`]).
pub(crate) fn shadow_planes(sh: &ShadowSettings, distance: f64) -> (f64, f64) {
    let (n, f) = (f64::from(sh.near_plane), f64::from(sh.far_plane));
    if (n - 0.05).abs() < 1e-6 && (f - 256.0).abs() < 1e-6 {
        return (shadow::NEAR, shadow::FAR);
    }
    shadow::planes_for_distance(n, f, distance)
}

/// Per-draw state.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DrawState {
    pub model_view: Mat4,
    pub projection: Mat4,
    pub texture_matrix: Mat4,
    pub color_modulator: [f32; 4],
    /// `DynamicTransforms.ModelOffset` / `modelOffset` / `chunkOffset`.
    pub model_offset: [f32; 3],
    pub entity_id: i32,
    pub block_entity_id: i32,
    pub item_id: i32,
    pub entity_color: [f32; 4],
    pub alpha_test_ref: f32,
    pub blend_func: [i32; 4],
    pub texture_size: [i32; 2],
    pub texture_id: i32,
    pub render_stage: i32,
    /// `vertUniqueUniformBlock.uModelOffset` (DH buffer minimum corner).
    pub dh_model_offset: [f32; 3],
    /// Sodium `u_RegionOffset`: the region's minimum corner relative to the camera.
    pub region_offset: [f32; 3],
    /// Sodium `u_RegionID`.
    pub region_id: u32,
    /// Shadow pass draw.
    pub shadow: bool,
}

impl DrawState {
    /// A draw with the camera matrices of `f`.
    pub fn new(f: &FrameState) -> Self {
        Self {
            model_view: f.model_view,
            projection: f.projection,
            texture_matrix: Mat4::IDENTITY,
            color_modulator: [1.0; 4],
            model_offset: [0.0; 3],
            entity_id: -1,
            block_entity_id: -1,
            item_id: -1,
            entity_color: [0.0; 4],
            alpha_test_ref: 0.0,
            blend_func: [0; 4],
            texture_size: [0; 2],
            texture_id: 0,
            render_stage: 0,
            dh_model_offset: [0.0; 3],
            region_offset: [0.0; 3],
            region_id: 0,
            shadow: false,
        }
    }

    /// The same draw in the shadow pass (shadow matrices).
    pub fn for_shadow(mut self, f: &FrameState) -> Self {
        self.model_view = f.shadow_model_view;
        self.projection = f.shadow_projection;
        self.shadow = true;
        self
    }
}

/// Iris `WorldRenderingPhase` ordinals (`MC_RENDER_STAGE_*`).
pub(crate) mod stage {
    pub const SKY: i32 = 1;
    pub const SUN: i32 = 4;
    pub const MOON: i32 = 5;
    pub const TERRAIN_SOLID: i32 = 8;
    pub const TERRAIN_CUTOUT: i32 = 10;
    pub const ENTITIES: i32 = 11;
    pub const TERRAIN_TRANSLUCENT: i32 = 17;
}

fn v3(v: Vec3) -> Value {
    Value::Vec3(math::vec::to_f32(v))
}

fn m4(m: &Mat4) -> Value {
    Value::Mat4(m.to_cols_f32())
}

fn b(v: bool) -> Value {
    Value::Bool(v)
}

fn fl(v: f64) -> Value {
    Value::Float(v as f32)
}

/// The value of a builtin uniform (sb-uniforms registry name) for this frame and draw.
/// `None` only for names that are not builtins.
pub(crate) fn builtin_value(name: &str, f: &FrameState, d: &DrawState) -> Option<Value> {
    let floor = f.camera.map(f64::floor);
    let fract = [f.camera[0] - floor[0], f.camera[1] - floor[1], f.camera[2] - floor[2]];
    let normal_matrix = d.model_view.inverse_or_identity().transpose();
    let horizontal = math::vec::normalize([f.look[0], 0.0, f.look[2]]);
    Some(match name {
        // camera / player
        "cameraPosition" | "previousCameraPosition" | "eyePosition" => v3(f.camera),
        "eyeAltitude" => fl(f.camera[1]),
        "cameraPositionInt" | "previousCameraPositionInt" => v3(floor),
        "cameraPositionFract" | "previousCameraPositionFract" => v3(fract),
        "relativeEyePosition" | "vehicleLookVector" | "relativeVehiclePosition" | "currentSelectedBlockPos" | "endFlashPosition" => v3([0.0; 3]),
        "playerLookVector" => v3(f.look),
        "playerBodyVector" => v3(horizontal),
        "upPosition" => v3(f.up_position),
        "eyeBrightness" | "eyeBrightnessSmooth" => Value::Vec2([0.0, 240.0]),
        "centerDepthSmooth" => Value::Float(f.center_depth),
        "firstPersonCamera" | "isRightHanded" | "is_alive" | "is_on_ground" | "hasSkylight" | "hideGUI" => b(true),
        "isEyeInWater" => Value::Int(i32::from(f.camera[1] < f64::from(SEA_LEVEL) - 0.125)),
        "isSpectator" | "isRiding" | "vehicleInWater" | "inSwimmingAnimation" | "feetInWater" | "isElytraFlying" | "heavyFog" | "is_sneaking"
        | "is_sprinting" | "is_hurt" | "is_invisible" | "is_burning" | "is_child" | "is_glowing" | "is_in_lava" | "is_in_water" | "is_ridden"
        | "is_riding" | "is_wet" | "hasCeiling" => b(false),
        "blindness" | "darknessFactor" | "darknessLightFactor" | "nightVision" | "playerMood" | "constantMood" | "currentPlayerArmor"
        | "endFlashIntensity" | "previousEndFlashIntensity" | "ambientLight" => Value::Float(0.0),
        "currentPlayerHealth" | "maxPlayerHealth" | "currentPlayerHunger" | "maxPlayerHunger" => Value::Float(20.0),
        "maxPlayerArmor" => Value::Float(50.0),
        "currentPlayerAir" | "maxPlayerAir" => Value::Float(300.0),
        // screen / time
        "viewWidth" => Value::Float(f.width),
        "viewHeight" => Value::Float(f.height),
        "aspectRatio" => Value::Float(f.width / f.height),
        "screenBrightness" => Value::Float(0.5),
        "frameCounter" => Value::Int(f.frame_counter as i32),
        "frameTime" => Value::Float(f.frame_time),
        "frameTimeCounter" => Value::Float(f.frame_time_counter),
        "currentColorSpace" | "textureFilteringMode" | "anisotropicFiltering" | "textureReloadCount" | "bossBattle" => Value::Int(0),
        // A fixed date keeps renders reproducible.
        "currentDate" => Value::Vec3([2026.0, 10.0, 2.0]),
        "currentTime" => Value::Vec3([12.0, 0.0, 0.0]),
        "currentYearTime" => Value::Vec2([23_716_800.0, 7_819_200.0]),
        // ids
        "heldItemId" | "heldItemId2" | "vehicleId" | "currentSelectedBlockId" => Value::Int(-1),
        "heldBlockLightValue" | "heldBlockLightValue2" => Value::Int(0),
        "heldBlockLightColor" | "heldBlockLightColor2" => v3([0.0; 3]),
        "entityId" => Value::Int(d.entity_id),
        "blockEntityId" => Value::Int(d.block_entity_id),
        "currentRenderedItemId" => Value::Int(d.item_id),
        // celestial / weather
        "sunPosition" => v3(f.sun_position),
        "moonPosition" => v3(f.moon_position),
        "shadowLightPosition" => v3(f.shadow_light_position),
        "sunAngle" => fl(f.sun_angle),
        "shadowAngle" => fl(f.shadow_angle),
        "moonPhase" => Value::Int(f.moon_phase),
        "worldTime" => Value::Int(f.world_time as i32),
        "worldDay" => Value::Int(f.world_day.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32),
        "rainStrength" | "wetness" => Value::Float(f.rain),
        "thunderStrength" => Value::Float(f.thunder),
        "lightningBoltPosition" => Value::Vec4([0.0; 4]),
        "cloudTime" => Value::Float((f.world_time as f32 + f.frame_time_counter * 20.0) % 24000.0),
        "cloudHeight" => Value::Float(192.33),
        // dimension / biome
        "bedrockLevel" => Value::Int(-64),
        "heightLimit" | "logicalHeightLimit" => Value::Int(384),
        "seaLevel" => Value::Int(SEA_LEVEL),
        "biome" => Value::Int(BIOMES.iter().position(|b| *b == "plains").unwrap_or(0) as i32),
        "biome_category" => Value::Int(5),      // CAT_PLAINS
        "biome_precipitation" => Value::Int(1), // PPT_RAIN
        "rainfall" => Value::Float(0.4),
        "temperature" => Value::Float(0.8),
        // rendering
        "near" => fl(f.near),
        "far" => fl(f.far),
        "fogColor" => Value::Vec3(f.fog_color),
        "skyColor" => Value::Vec3(f.sky_color),
        "sb_FogColor" => Value::Vec4([f.fog_color[0], f.fog_color[1], f.fog_color[2], 1.0]),
        "fogMode" => Value::Int(9729), // GL_LINEAR
        "fogShape" => Value::Int(1),   // cylinder (terrain)
        "fogDensity" => Value::Float(0.0),
        "fogStart" => Value::Float(f.fog_start),
        "fogEnd" => Value::Float(f.fog_end),
        "fogScale" => Value::Float(1.0 / (f.fog_end - f.fog_start).max(1e-3)),
        "alphaTestRef" => Value::Float(d.alpha_test_ref),
        "entityColor" => Value::Vec4(d.entity_color),
        "blendFunc" => Value::Vec4(d.blend_func.map(|v| v as f32)),
        "atlasSize" | "gtextureSize" => Value::Vec2(d.texture_size.map(|v| v as f32)),
        "gtextureId" => Value::Int(d.texture_id),
        "renderStage" => Value::Int(d.render_stage),
        "chunkFadeTimeInv" => Value::Float(1.0 / 0.75),
        "pi" => Value::Float(std::f32::consts::PI),
        "spriteBounds" => Value::Vec4([0.0, 0.0, 1.0, 1.0]),
        "instanceId" => Value::Int(0),
        "terrainTextureSize" => Value::Vec2([64.0, 64.0]),
        "terrainIconSize" => Value::Int(16),
        // matrices
        "gbufferModelView" | "gbufferPreviousModelView" => m4(&f.model_view),
        "gbufferModelViewInverse" => m4(&f.model_view_inverse()),
        "gbufferProjection" | "gbufferPreviousProjection" => m4(&f.projection),
        "gbufferProjectionInverse" => m4(&f.projection.inverse_or_identity()),
        "shadowModelView" => m4(&f.shadow_model_view),
        "shadowModelViewInverse" => m4(&f.shadow_model_view.inverse_or_identity()),
        "shadowProjection" => m4(&f.shadow_projection),
        "shadowProjectionInverse" => m4(&f.shadow_projection.inverse_or_identity()),
        "dhProjection" | "dhPreviousProjection" => m4(&f.dh_projection),
        "dhProjectionInverse" => m4(&f.dh_projection.inverse_or_identity()),
        "dhNearPlane" => fl(f.dh_near),
        "dhFarPlane" => fl(f.dh_far),
        "dhRenderDistance" => Value::Int(f.dh_render_distance),
        // Voxy is not part of the synthetic scene: no LOD distance, and the LOD matrices
        // equal the level's (what a pack falls back to when it combines both).
        "vxRenderDistance" => Value::Int(0),
        "vxProj" | "vxProjPrev" => m4(&f.projection),
        "vxProjInv" => m4(&f.projection.inverse_or_identity()),
        "vxModelView" | "vxModelViewPrev" => m4(&f.model_view),
        "vxModelViewInv" => m4(&f.model_view_inverse()),
        "modelViewMatrix" => m4(&d.model_view),
        "modelViewMatrixInverse" => m4(&d.model_view.inverse_or_identity()),
        "projectionMatrix" => m4(&d.projection),
        "projectionMatrixInverse" => m4(&d.projection.inverse_or_identity()),
        "normalMatrix" => m4(&normal_matrix),
        "textureMatrix" => m4(&d.texture_matrix),
        "colorModulator" => Value::Vec4(d.color_modulator),
        "chunkOffset" | "modelOffset" => Value::Vec3(d.model_offset),
        _ => return None,
    })
}

/// The value of a member of a draw-profile host block (Mojang's `Globals`,
/// `TerrainUniform`, `DynamicTransforms`, `Projection`, `Fog`; DH's
/// `vertUniqueUniformBlock`, `vertSharedUniformBlock`, `fragUniformBlock`,
/// `vertUniformBlock`). `None` = unknown (zero-filled).
pub(crate) fn host_member_value(block: &str, member: &str, f: &FrameState, d: &DrawState) -> Option<Value> {
    let floor = f.camera.map(f64::floor);
    let dh_mvp = |m: &Mat4| if d.shadow { m4(&f.shadow_projection.mul(&f.shadow_model_view)) } else { m4(&f.dh_projection.mul(m)) };
    Some(match (block, member) {
        ("Globals", "CameraBlockPos") => v3(floor),
        ("Globals", "CameraOffset") => v3([floor[0] - f.camera[0], floor[1] - f.camera[1], floor[2] - f.camera[2]]),
        ("Globals", "GlintAlpha") => Value::Float(1.0),
        ("Globals", "GameTime") => Value::Float(f.world_time as f32 / 24000.0),
        ("Globals", "ScreenSize") => Value::Vec2([f.width, f.height]),
        ("Globals", "MenuBlurRadius" | "UseRgss") => Value::Int(0),
        ("TerrainUniform" | "DynamicTransforms", "ModelViewMat") => m4(&d.model_view),
        ("TerrainUniform", "TextureSize") => Value::Vec2(d.texture_size.map(|v| v as f32)),
        ("DynamicTransforms", "TextureMat") => m4(&d.texture_matrix),
        ("DynamicTransforms", "ColorModulator") => Value::Vec4(d.color_modulator),
        ("DynamicTransforms", "ModelOffset") => Value::Vec3(d.model_offset),
        ("Projection", "ProjMat") => m4(&d.projection),
        ("Fog", "FogColor") => Value::Vec4([f.fog_color[0], f.fog_color[1], f.fog_color[2], 1.0]),
        ("Fog", "FogEnvironmentalStart") => Value::Float(f.fog_start),
        ("Fog", "FogEnvironmentalEnd") => Value::Float(f.fog_end),
        ("Fog", "FogRenderDistanceStart") => Value::Float(f.render_fog_start),
        ("Fog", "FogRenderDistanceEnd") => Value::Float(f.render_fog_end),
        ("Fog", "FogSkyEnd") => Value::Float(f.sky_fog_end),
        ("Fog", "FogCloudsEnd") => Value::Float(f.cloud_fog_end),
        ("vertUniqueUniformBlock", "uModelOffset") => Value::Vec3(d.dh_model_offset),
        ("vertSharedUniformBlock", "uIsWhiteWorld") => b(false),
        ("vertSharedUniformBlock", "uWorldYOffset") => Value::Float(crate::scene::dh::REGION_MIN_Y as f32),
        ("vertSharedUniformBlock", "uMircoOffset") => Value::Float(0.01),
        ("vertSharedUniformBlock", "uEarthRadius") => Value::Float(0.0),
        ("vertSharedUniformBlock", "uFrameMod8") => Value::Float((f.frame_counter % 8) as f32),
        ("vertSharedUniformBlock", "uViewWidth") => Value::Float(f.width),
        ("vertSharedUniformBlock", "uViewHeight") => Value::Float(f.height),
        ("vertSharedUniformBlock", "uCameraPos") => v3(f.camera),
        ("vertSharedUniformBlock", "uCombinedMatrix") => dh_mvp(&f.model_view),
        ("fragUniformBlock", "uClipDistance") => fl(f.dh_near),
        ("fragUniformBlock", "uNoiseIntensity") => Value::Float(0.0),
        ("fragUniformBlock", "uNoiseSteps" | "uNoiseDropoff") => Value::Int(0),
        ("fragUniformBlock", "uDitherDhRendering" | "uNoiseEnabled") => b(false),
        ("vertUniformBlock", "uProjectionMvm") => dh_mvp(&f.model_view),
        ("vertUniformBlock", "uSkyLight") => Value::Int(15),
        ("vertUniformBlock", "uBlockLight") => Value::Int(0),
        ("vertUniformBlock", "uCameraPosChunk") => v3(f.camera.map(|c| (c / 16.0).floor())),
        ("vertUniformBlock", "uCameraPosSubChunk") => v3(f.camera.map(|c| c - (c / 16.0).floor() * 16.0)),
        ("vertUniformBlock", "uOffsetChunk" | "uOffsetSubChunk") => v3([0.0; 3]),
        ("vertUniformBlock", "uNorthShading" | "uSouthShading" | "uEastShading" | "uWestShading" | "uTopShading" | "uBottomShading") => Value::Float(1.0),
        // Sodium's terrain UBO (`sodium_terrain` profile).
        ("u_Globals", "u_ProjectionMatrix") => m4(&d.projection),
        ("u_Globals", "u_ModelViewMatrix") => m4(&d.model_view),
        ("u_Globals", "u_FogColor") => Value::Vec4([f.fog_color[0], f.fog_color[1], f.fog_color[2], 1.0]),
        ("u_Globals", "u_EnvironmentFog") => Value::Vec2([f.fog_start, f.fog_end]),
        ("u_Globals", "u_RenderFog") => Value::Vec2([f.render_fog_start, f.render_fog_end]),
        ("u_Globals", "u_TexelSize") => Value::Vec2(d.texture_size.map(|s| 1.0 / s.max(1) as f32)),
        // A 1/64-texel nudge of each corner towards its quad centre (against bleeding).
        ("u_Globals", "u_TexCoordShrink") => Value::Vec2(d.texture_size.map(|s| 1.0 / (64.0 * s.max(1) as f32))),
        ("u_Globals", "u_FadePeriodInv") => Value::Float(1.0 / 0.75),
        ("u_Globals", "u_UseRGSS") => b(false),
        _ => return None,
    })
}

/// The value of a push-constant member (Sodium's per-region `u_RegionOffset`,
/// `u_CurrentTime`, `u_RegionID`). `None` = unknown (zero-filled).
pub(crate) fn push_member_value(member: &str, f: &FrameState, d: &DrawState) -> Option<Value> {
    Some(match member {
        "u_RegionOffset" => Value::Vec3(d.region_offset),
        "u_CurrentTime" => Value::Int(f.frame_counter as i32),
        "u_RegionID" => Value::Int(d.region_id as i32),
        _ => return None,
    })
}

/// Push-constant bytes (`size` long) for reflected `members`: known members get their
/// values, everything else stays zero.
pub(crate) fn push_constant_bytes(members: &[sb_compile::BufferMember], size: u32, f: &FrameState, d: &DrawState) -> Vec<u8> {
    let mut bytes = vec![0u8; size as usize];
    for m in members {
        let (Some(ty), Some(dst)) = (m.glsl_type, bytes.get_mut(m.offset as usize..)) else { continue };
        if ty.array.is_some() {
            continue;
        }
        if let Some(v) = push_member_value(&m.name, f, d) {
            let _ = write_value(ty, v, dst);
        }
    }
    bytes
}

/// Names of the host blocks [`host_member_value`] knows.
pub(crate) const HOST_BLOCKS: [&str; 10] = [
    "Globals",
    "TerrainUniform",
    "DynamicTransforms",
    "Projection",
    "Fog",
    "vertUniqueUniformBlock",
    "vertSharedUniformBlock",
    "fragUniformBlock",
    "vertUniformBlock",
    "u_Globals",
];

/// Write `values` (GLSL constructor order) into std140 memory of type `ty`.
pub(crate) fn write_default(ty: GlslType, values: &[f32], out: &mut [u8]) {
    let scalar = |kind: ScalarKind, v: f32, out: &mut [u8]| {
        let bytes = match kind {
            ScalarKind::Float => v.to_le_bytes(),
            ScalarKind::Int => (v as i32).to_le_bytes(),
            ScalarKind::Uint => (v.max(0.0) as u32).to_le_bytes(),
            ScalarKind::Bool => u32::from(v != 0.0).to_le_bytes(),
            ScalarKind::Double => {
                if let Some(o) = out.get_mut(..8) {
                    o.copy_from_slice(&f64::from(v).to_le_bytes());
                }
                return;
            }
        };
        if let Some(o) = out.get_mut(..4) {
            o.copy_from_slice(&bytes);
        }
    };
    let ssize = if ty.scalar == ScalarKind::Double { 8 } else { 4 };
    let elem = ty.element();
    let per_elem = usize::from(elem.rows) * usize::from(elem.cols);
    let count = ty.array.unwrap_or(1) as usize;
    let stride = if ty.array.is_some() { ty.std140_array_stride() as usize } else { 0 };
    let col_stride = if elem.cols > 1 { if ty.scalar == ScalarKind::Double { 32 } else { 16 } } else { 0 };
    for e in 0..count {
        for c in 0..usize::from(elem.cols) {
            for r in 0..usize::from(elem.rows) {
                let Some(&v) = values.get(e * per_elem + c * usize::from(elem.rows) + r) else { return };
                let off = e * stride + c * col_stride + r * ssize;
                if let Some(o) = out.get_mut(off..) {
                    scalar(elem.scalar, v, o);
                }
            }
        }
    }
}

/// Fill a pack block (`sb_Frame` / `sb_Draw`) from its layout: builtins from the
/// providers, `Unset` members from their declared defaults (zero otherwise). `Custom`
/// members are zeroed here and written by `CustomUniforms::evaluate_into_block`.
pub(crate) fn fill_pack_block(layout: &BlockLayout, out: &mut [u8], f: &FrameState, d: &DrawState) {
    for m in &layout.members {
        let Some(dst) = out.get_mut(m.offset as usize..) else { continue };
        let size = m.ty.std140_size() as usize;
        if let Some(zero) = dst.get_mut(..size.min(dst.len())) {
            zero.fill(0);
        }
        match &m.source {
            UniformSource::Builtin(name) => match builtin_value(name, f, d) {
                Some(v) if m.ty.array.is_none() => {
                    let _ = write_value(m.ty, v, dst);
                }
                _ => {
                    if let Some(def) = &m.default {
                        write_default(m.ty, def, dst);
                    }
                }
            },
            UniformSource::Unset => {
                if let Some(def) = &m.default {
                    write_default(m.ty, def, dst);
                }
            }
            UniformSource::Custom(_) => {}
        }
    }
}

/// Fill a reflected host block: every member with a known value is written, the rest
/// stays zero.
pub(crate) fn fill_host_block(block: &str, members: &[sb_compile::BufferMember], out: &mut [u8], f: &FrameState, d: &DrawState) {
    for m in members {
        let (Some(ty), Some(dst)) = (m.glsl_type, out.get_mut(m.offset as usize..)) else { continue };
        if ty.array.is_some() {
            continue;
        }
        if let Some(v) = host_member_value(block, &m.name, f, d) {
            let _ = write_value(ty, v, dst);
        }
    }
}

/// Expression constants for custom uniforms: `CAT_*`, `PPT_*` and `BIOME_*`.
pub(crate) fn expression_constants() -> indexmap::IndexMap<String, Value> {
    let mut c = sb_expr::standard_constants();
    for (i, name) in BIOMES.iter().enumerate() {
        c.insert(format!("BIOME_{}", name.to_ascii_uppercase()), Value::Int(i as i32));
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use sb_core::model::{BlockMember, UniformSource};
    use sb_expr::read_value;
    use sb_uniforms::Frequency;

    fn state() -> FrameState {
        state_with_dh(true)
    }

    fn state_with_dh(dh: bool) -> FrameState {
        let scene = SceneParams::default();
        let settings = PackSettings::default();
        let shadow = ShadowSettings::default();
        FrameState::new(&FrameInputs {
            scene: &scene,
            camera: scene.camera_position(),
            width: 640,
            height: 360,
            frame: 3,
            settings: &settings,
            shadow: &shadow,
            dh,
            unified_projection: false,
            center_depth: 0.5,
        })
    }

    #[test]
    fn every_registry_builtin_has_a_provider() {
        let f = state();
        let d = DrawState::new(&f);
        for u in sb_uniforms::all() {
            let v = builtin_value(u.name, &f, &d).unwrap_or_else(|| panic!("no provider for builtin `{}`", u.name));
            // The value must be writable as the registry type.
            let mut buf = vec![0u8; u.ty.std140_size() as usize];
            write_value(u.ty, v, &mut buf).unwrap_or_else(|e| panic!("`{}`: {e}", u.name));
            assert!(u.frequency == Frequency::Frame || u.frequency == Frequency::Draw);
        }
        assert!(builtin_value("notABuiltin", &f, &d).is_none());
    }

    #[test]
    fn frame_values_follow_gl_conventions() {
        let f = state();
        assert!((f.projection.project_point([0.0, 0.0, -NEAR])[2] + 1.0).abs() < 1e-9);
        assert!((f.far - 64.0).abs() < 1e-9);
        let expected_dh_far = (16.0 * 16.0 + 512.0) * std::f64::consts::SQRT_2;
        assert!((f.dh_far - expected_dh_far).abs() < 1e-9);
        assert!((f.dh_projection.project_point([0.0, 0.0, -expected_dh_far])[2] - 1.0).abs() < 1e-9);
        let d = DrawState::new(&f);
        let Some(Value::Mat4(inv)) = builtin_value("gbufferProjectionInverse", &f, &d) else { panic!() };
        let inv = Mat4 { cols: [0, 1, 2, 3].map(|c| [0, 1, 2, 3].map(|r| f64::from(inv[c * 4 + r]))) };
        assert!(inv.mul(&f.projection).approx_eq(&Mat4::IDENTITY, 1e-4));
        assert_eq!(builtin_value("frameCounter", &f, &d), Some(Value::Int(3)));
        assert_eq!(builtin_value("viewWidth", &f, &d), Some(Value::Float(640.0)));
        let Some(Value::Vec3(sun)) = builtin_value("sunPosition", &f, &d) else { panic!() };
        assert!((sun.iter().map(|v| v * v).sum::<f32>().sqrt() - 100.0).abs() < 1e-3);
        // Unified projection: far is the DH far plane and dhProjection == gbufferProjection.
        let scene = SceneParams::default();
        let settings = PackSettings::default();
        let shadow = ShadowSettings::default();
        let u = FrameState::new(&FrameInputs {
            scene: &scene,
            camera: [0.0, 80.0, 0.0],
            width: 100,
            height: 100,
            frame: 0,
            settings: &settings,
            shadow: &shadow,
            dh: true,
            unified_projection: true,
            center_depth: 1.0,
        });
        assert!((u.far - expected_dh_far).abs() < 1e-9);
        assert_eq!(u.dh_projection, u.projection);
    }

    #[test]
    fn shadow_planes_use_iris_defaults() {
        let s = ShadowSettings::default();
        assert_eq!(shadow_planes(&s, 256.0), (shadow::NEAR, shadow::FAR));
        let s = ShadowSettings { near_plane: -1.0, far_plane: -1.0, ..Default::default() };
        // Without DH, -1 is the vanilla render distance in blocks (Iris: chunks * 16)...
        assert_eq!(shadow_planes(&s, shadow_plane_distance(false, 16.0, 64.0)), (-64.0, 64.0));
        // ...while DH renders, Iris scales the DH distance in blocks by 16 again.
        assert_eq!(shadow_planes(&s, shadow_plane_distance(true, 16.0, 64.0)), (-4096.0, 4096.0));
        let s = ShadowSettings { near_plane: -1.0, far_plane: 300.0, ..Default::default() };
        assert_eq!(shadow_planes(&s, 64.0), (-64.0, 300.0));
    }

    /// The `-1` shadow planes of a frame without Distant Horizons follow the vanilla
    /// render distance (the planes used to collapse to 0 and fall back to the defaults).
    #[test]
    fn shadow_projection_without_dh_uses_render_distance() {
        let scene = SceneParams { render_distance: 4, dh_render_distance: 0, ..Default::default() };
        let settings = PackSettings::default();
        let shadow = ShadowSettings { near_plane: -1.0, far_plane: -1.0, distance: 32.0, ..Default::default() };
        let f = FrameState::new(&FrameInputs {
            scene: &scene,
            camera: [0.0, 80.0, 0.0],
            width: 64,
            height: 64,
            frame: 0,
            settings: &settings,
            shadow: &shadow,
            dh: false,
            unified_projection: false,
            center_depth: 1.0,
        });
        assert!(f.shadow_projection.approx_eq(&shadow::ortho(32.0, -64.0, 64.0), 1e-12), "{:?}", f.shadow_projection);
    }

    /// `gbufferProjection` is Minecraft's level projection: near 0.05, far `depthFar`
    /// (4x the render distance, at least the 2048-block default cloud range), while
    /// `far` reports the render distance; the unified DH projection uses the DH far
    /// plane for both.
    #[test]
    fn projection_far_plane_is_minecraft_depth_far() {
        assert_eq!(mc_depth_far(64.0), 2048.0);
        assert_eq!(mc_depth_far(32.0 * 16.0), 2048.0);
        assert_eq!(mc_depth_far(48.0 * 16.0), 3072.0);
        let f = state();
        assert!((f.projection.project_point([0.0, 0.0, -2048.0])[2] - 1.0).abs() < 1e-9);
        assert!((f.projection.project_point([0.0, 0.0, -NEAR])[2] + 1.0).abs() < 1e-9);
        let d = DrawState::new(&f);
        assert_eq!(builtin_value("far", &f, &d), Some(Value::Float(64.0)));
    }

    /// Iris' `fogStart` / `fogEnd` are Minecraft's environmental fog (0 / 1024 blocks in
    /// the open overworld), not the render-distance fog; Distant Horizons pushes both out
    /// of reach while it renders LODs.
    #[test]
    fn fog_follows_minecraft_and_distant_horizons() {
        let f = state_with_dh(false);
        let d = DrawState::new(&f);
        assert_eq!(builtin_value("fogStart", &f, &d), Some(Value::Float(0.0)));
        assert_eq!(builtin_value("fogEnd", &f, &d), Some(Value::Float(1024.0)));
        assert_eq!(host_member_value("Fog", "FogRenderDistanceStart", &f, &d), Some(Value::Float(57.6)));
        assert_eq!(host_member_value("Fog", "FogRenderDistanceEnd", &f, &d), Some(Value::Float(64.0)));
        assert_eq!(host_member_value("Fog", "FogSkyEnd", &f, &d), Some(Value::Float(64.0)));
        assert_eq!(host_member_value("Fog", "FogCloudsEnd", &f, &d), Some(Value::Float(2048.0)));
        let rain = Fog::new(64.0, 1.0, false);
        assert_eq!(rain.environmental, (-160.0, 768.0));
        assert_eq!(Fog::new(512.0, 0.0, false).render_distance, (460.8, 512.0));
        assert_eq!(Fog::new(2048.0, 0.0, false).render_distance, (1984.0, 2048.0));
        let dh = Fog::new(64.0, 0.5, true);
        assert_eq!((dh.environmental, dh.render_distance), (DH_NO_FOG, DH_NO_FOG));
        let f = state();
        assert_eq!(builtin_value("fogStart", &f, &DrawState::new(&f)), Some(Value::Float(DH_NO_FOG.0)));
        assert!(Fog::new(64.0, f32::NAN, false).environmental.1 == 1024.0);
    }

    #[test]
    fn pack_block_filling() {
        let f = state();
        let d = DrawState { entity_id: 7, ..DrawState::new(&f) };
        let mut layout = BlockLayout { name: "sb_Frame".into(), set: 0, binding: 0, size: 160, members: Vec::new() };
        let mut add = |name: &str, ty: GlslType, offset: u32, source: UniformSource, default: Option<Vec<f32>>| {
            layout.members.push(BlockMember { name: name.into(), ty, offset, source, default });
        };
        add("frameTimeCounter", GlslType::FLOAT, 0, UniformSource::Builtin("frameTimeCounter".into()), None);
        add("worldTime__float", GlslType::FLOAT, 4, UniformSource::Builtin("worldTime".into()), None);
        add("myConst", GlslType::VEC3, 16, UniformSource::Unset, Some(vec![1.0, 2.0, 3.0]));
        add("myArr", GlslType::FLOAT.with_array(2), 32, UniformSource::Unset, Some(vec![5.0, 6.0]));
        add("myMat", GlslType::MAT2, 64, UniformSource::Unset, Some(vec![1.0, 2.0, 3.0, 4.0]));
        add("entityId", GlslType::INT, 96, UniformSource::Builtin("entityId".into()), None);
        add("hideGUI", GlslType::INT, 100, UniformSource::Builtin("hideGUI".into()), None);
        add("custom", GlslType::FLOAT, 104, UniformSource::Custom("custom".into()), None);
        let mut buf = vec![0xAAu8; 160];
        fill_pack_block(&layout, &mut buf, &f, &d);
        let rf = |o: usize| f32::from_le_bytes(buf[o..o + 4].try_into().unwrap());
        let ri = |o: usize| i32::from_le_bytes(buf[o..o + 4].try_into().unwrap());
        assert_eq!(rf(0), f.frame_time_counter);
        assert_eq!(rf(4), f.world_time as f32);
        assert_eq!((rf(16), rf(20), rf(24)), (1.0, 2.0, 3.0));
        assert_eq!((rf(32), rf(48)), (5.0, 6.0));
        assert_eq!((rf(64), rf(68), rf(80), rf(84)), (1.0, 2.0, 3.0, 4.0));
        assert_eq!(ri(96), 7);
        assert_eq!(ri(100), 1);
        // Custom members are zeroed; the expression evaluator fills them afterwards.
        assert_eq!(&buf[104..108], &[0; 4]);
    }

    #[test]
    fn host_blocks() {
        let f = state();
        let d = DrawState { dh_model_offset: [128.0, -64.0, 256.0], ..DrawState::new(&f) };
        assert_eq!(host_member_value("vertUniqueUniformBlock", "uModelOffset", &f, &d), Some(Value::Vec3([128.0, -64.0, 256.0])));
        let Some(Value::Vec3(off)) = host_member_value("Globals", "CameraOffset", &f, &d) else { panic!() };
        assert!(off.iter().all(|v| *v <= 0.0 && *v > -1.0));
        assert!(host_member_value("Globals", "Nope", &f, &d).is_none());
        // Reflected member list: the TerrainUniform layout of the profile.
        let members = vec![
            sb_compile::BufferMember { name: "ModelViewMat".into(), offset: 0, size: 64, type_name: "mat4".into(), glsl_type: Some(GlslType::MAT4), array_stride: None, matrix_stride: Some(16), row_major: false },
            sb_compile::BufferMember { name: "TextureSize".into(), offset: 64, size: 8, type_name: "ivec2".into(), glsl_type: Some(GlslType::IVEC2), array_stride: None, matrix_stride: None, row_major: false },
        ];
        let d = DrawState { texture_size: [64, 64], ..DrawState::new(&f) };
        let mut buf = vec![0u8; 80];
        fill_host_block("TerrainUniform", &members, &mut buf, &f, &d);
        assert_eq!(read_value(GlslType::IVEC2, &buf[64..]).unwrap(), Value::Vec2([64.0, 64.0]));
        assert_eq!(read_value(GlslType::MAT4, &buf).unwrap(), Value::Mat4(f.model_view.to_cols_f32()));
    }

    #[test]
    fn biome_constants_match_biome_uniform() {
        let c = expression_constants();
        let f = state();
        assert_eq!(c.get("BIOME_PLAINS").copied(), builtin_value("biome", &f, &DrawState::new(&f)));
        assert_eq!(c.get("CAT_PLAINS").copied(), Some(Value::Int(5)));
    }
}
