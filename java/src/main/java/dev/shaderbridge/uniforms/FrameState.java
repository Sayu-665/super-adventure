package dev.shaderbridge.uniforms;

import java.time.LocalDateTime;
import org.joml.Matrix4f;
import org.joml.Matrix4fc;
import org.joml.Vector3d;
import org.joml.Vector3f;
import org.joml.Vector4f;

/**
 * Everything the per-frame builtin uniforms are computed from.
 *
 * <p>The public input fields are written once per frame by {@link GameStateCapture} (or by tests);
 * {@link #update()} then advances the trackers and derives the remaining values (matrices,
 * celestial positions, smoothed values). Defaults describe "no world loaded". Not thread-safe:
 * used on the render thread only.
 */
public final class FrameState {
    // ------------------------------------------------------------------ inputs: time and screen
    /** Monotonic start time of the frame ({@link System#nanoTime()}). */
    public long frameStartNanos;
    /** Partial tick of the frame. */
    public float partialTick;
    /** Local wall-clock time ({@code currentDate}, {@code currentTime}). */
    public LocalDateTime localTime = LocalDateTime.of(2000, 1, 1, 0, 0);
    /** Main render target width in pixels. */
    public int viewWidth = 1;
    /** Main render target height in pixels. */
    public int viewHeight = 1;
    /** Brightness (gamma) option, 0..1. */
    public float screenBrightness;
    /** Texture filtering option: 0 none, 1 RGSS, 2 anisotropic. */
    public int textureFilteringMode;
    /** Anisotropy level, 0 when anisotropic filtering is off. */
    public int anisotropicFiltering;
    /** {@code 1 / chunk fade-in time}. */
    public float chunkFadeTimeInv;
    /** The HUD is hidden (F1). */
    public boolean hideGui;
    /** Number of resource reloads since start. */
    public int textureReloadCount;

    // ------------------------------------------------------------------ inputs: camera
    /** Camera position in world space. */
    public final Vector3d cameraPosition = new Vector3d();
    /** Camera view rotation ({@code gbufferModelView}). */
    public final Matrix4f viewRotation = new Matrix4f();
    /** Minecraft's projection of the frame (reversed-Z, view bobbing applied). */
    public final Matrix4f projection = new Matrix4f();
    /** The device clips z to [0,1]. */
    public boolean zeroToOne = true;
    /** Render distance in blocks. */
    public float renderDistanceBlocks = 256;
    /** First-person camera. */
    public boolean firstPersonCamera = true;
    /** Eye position of the camera entity. */
    public final Vector3d eyePosition = new Vector3d();
    /** View direction of the camera entity (zero if it is not living). */
    public final Vector3d playerLookVector = new Vector3d();
    /** Body direction of the camera entity. */
    public final Vector3d playerBodyVector = new Vector3d();
    /** Forward direction of the ridden vehicle (zero without one). */
    public final Vector3d vehicleLookVector = new Vector3d();
    /** Position of the ridden vehicle. */
    public final Vector3d vehiclePosition = new Vector3d();
    /** The player rides a vehicle. */
    public boolean hasVehicle;

    // ------------------------------------------------------------------ inputs: player
    /** Block light at the eyes, 0..15. */
    public int eyeBlockLight;
    /** Sky light at the eyes, 0..15. */
    public int eyeSkyLight = 15;
    /** GL window depth at the screen centre (set by the render integration). */
    public float centerDepth = 1;
    /** 0 air, 1 water, 2 lava, 3 powder snow. */
    public int isEyeInWater;
    /** Spectator mode. */
    public boolean isSpectator;
    /** Main hand is the right hand. */
    public boolean isRightHanded = true;
    /** Blindness strength, 0..1. */
    public float blindness;
    /** Darkness strength, 0..1. */
    public float darknessFactor;
    /** Darkness lightmap pulse, 0..1. */
    public float darknessLightFactor;
    /** Night vision strength, 0..1. */
    public float nightVision;
    /** Cave mood, 0..1. */
    public float playerMood;
    /** Cave mood without resets, 0..1. */
    public float constantMood;
    /** Health / max health, or -1 outside survival. */
    public float currentPlayerHealth = -1;
    /** Max health, or -1 outside survival. */
    public float maxPlayerHealth = -1;
    /** Food level / 20, or -1 outside survival. */
    public float currentPlayerHunger = -1;
    /** Armor / 50, or -1 outside survival. */
    public float currentPlayerArmor = -1;
    /** Air supply / max air supply, or -1 outside survival. */
    public float currentPlayerAir = -1;
    /** Max air supply, or -1 outside survival. */
    public float maxPlayerAir = -1;
    /** Riding an entity. */
    public boolean isRiding;
    /** The vehicle is in water. */
    public boolean vehicleInWater;
    /** Swimming pose. */
    public boolean inSwimmingAnimation;
    /** Feet in (shallow) water. */
    public boolean feetInWater;
    /** Elytra gliding. */
    public boolean isElytraFlying;
    /** A boss bar requests world fog. */
    public boolean heavyFog;
    /** Sneaking. */
    public boolean isSneaking;
    /** Sprinting. */
    public boolean isSprinting;
    /** Recently hurt. */
    public boolean isHurt;
    /** Invisible. */
    public boolean isInvisible;
    /** Burning. */
    public boolean isBurning;
    /** On the ground. */
    public boolean isOnGround;
    /** Alive. */
    public boolean isAlive = true;
    /** The camera entity is a child. */
    public boolean isChild;
    /** Glowing. */
    public boolean isGlowing;
    /** In lava. */
    public boolean isInLava;
    /** In water. */
    public boolean isInWater;
    /** Ridden by another entity. */
    public boolean isRidden;
    /** In water or rain. */
    public boolean isWet;

    // ------------------------------------------------------------------ inputs: ids
    /** item.properties id of the main-hand item. */
    public int heldItemId = -1;
    /** item.properties id of the off-hand item. */
    public int heldItemId2 = -1;
    /** Light emitted by the main-hand item. */
    public int heldBlockLightValue;
    /** Light emitted by the off-hand item. */
    public int heldBlockLightValue2;
    /** Light color of the main-hand item. */
    public final Vector3f heldBlockLightColor = new Vector3f(1, 1, 1);
    /** Light color of the off-hand item. */
    public final Vector3f heldBlockLightColor2 = new Vector3f(1, 1, 1);
    /** entity.properties id of the vehicle (0 without one). */
    public int vehicleId;
    /** block.properties id of the targeted block (0 without one). */
    public int currentSelectedBlockId;
    /** Camera-relative centre of the targeted block ({@code -256} without one). */
    public final Vector3f currentSelectedBlockPos = new Vector3f(-256);

    // ------------------------------------------------------------------ inputs: world
    /** {@code SUN_ANGLE} attribute in degrees. */
    public float sunAngleAttribute;
    /** {@code MOON_ANGLE} attribute in degrees. */
    public float moonAngleAttribute = 180;
    /** Moon phase, 0..7. */
    public int moonPhase;
    /** Time of day in ticks, 0..23999. */
    public int worldTime;
    /** Day count. */
    public int worldDay;
    /** Rain strength, 0..1. */
    public float rainStrength;
    /** Thunder strength, 0..1. */
    public float thunderStrength;
    /** Camera-relative lightning position, w = 1 while a bolt exists. */
    public final Vector4f lightningBoltPosition = new Vector4f();
    /** The camera is in the End. */
    public boolean inEnd;
    /** An End flash is present. */
    public boolean hasEndFlash;
    /** End flash X angle in degrees. */
    public float endFlashXAngle;
    /** End flash Y angle in degrees. */
    public float endFlashYAngle;
    /** End flash intensity, 0..1. */
    public float endFlashIntensity;
    /** Vanilla cloud animation time. */
    public float cloudTime;
    /** Cloud height. */
    public float cloudHeight = 192;
    /** Ambient light of the dimension. */
    public float ambientLight;
    /** Lowest block Y. */
    public int bedrockLevel;
    /** Height of the dimension. */
    public int heightLimit = 256;
    /** Logical height of the dimension. */
    public int logicalHeightLimit = 256;
    /** Sea level. */
    public int seaLevel = 63;
    /** The dimension has a ceiling. */
    public boolean hasCeiling;
    /** The dimension has skylight. */
    public boolean hasSkylight = true;
    /** Boss bar kind: 0 none, 1 custom, 2 ender dragon, 3 wither, 4 raid. */
    public int bossBattle;
    /** Biome id ({@code BIOME_*}). */
    public int biome;
    /** Biome category ({@code CAT_*}). */
    public int biomeCategory;
    /** Precipitation: 0 none, 1 rain, 2 snow. */
    public int biomePrecipitation;
    /** Biome downfall. */
    public float rainfall;
    /** Biome base temperature. */
    public float temperature;

    // ------------------------------------------------------------------ inputs: fog and sky
    /** Fog color. */
    public final Vector3f fogColor = new Vector3f();
    /** Fog alpha. */
    public float fogAlpha = 1;
    /** Environmental fog start in blocks. */
    public float fogStart;
    /** Environmental fog end in blocks. */
    public float fogEnd = 1024;
    /** Exponential fog density under water, or -1 for linear fog. */
    public float fogDensity = -1;
    /** Sky color. */
    public final Vector3f skyColor = new Vector3f();

    // ------------------------------------------------------------------ inputs: Distant Horizons
    /** Distant Horizons is rendering (with the unified projection: and Minecraft's projection reaches the DH far plane). */
    public boolean dhActive;
    /** DH near plane ({@code dhProjection}'s, Minecraft's near plane with the unified projection). */
    public float dhNearPlane = 0.01f;
    /** DH far plane ({@code dhProjection}'s and, with the unified projection, {@code gbufferProjection}'s). */
    public float dhFarPlane = 0.01f;
    /** DH render distance in blocks (the vanilla render distance without DH). */
    public int dhRenderDistance = 256;

    // ------------------------------------------------------------------ derived (update)
    private final CameraTracker cameraTracker = new CameraTracker();
    private final FrameTimer timer = new FrameTimer();
    private UniformSettings settings = UniformSettings.DEFAULT;
    private SmoothedFloat wetness;
    private SmoothedFloat eyeBrightnessX;
    private SmoothedFloat eyeBrightnessY;
    private SmoothedFloat centerDepthSmooth;
    private boolean hasPreviousMatrices;
    private float previousEndFlashIntensity;
    private float currentEndFlashIntensity;

    final Matrix4f gbufferModelView = new Matrix4f();
    final Matrix4f gbufferModelViewInverse = new Matrix4f();
    final Matrix4f gbufferPreviousModelView = new Matrix4f();
    final Matrix4f gbufferProjection = new Matrix4f();
    final Matrix4f gbufferProjectionInverse = new Matrix4f();
    final Matrix4f gbufferPreviousProjection = new Matrix4f();
    final Matrix4f shadowModelView = new Matrix4f();
    final Matrix4f shadowModelViewInverse = new Matrix4f();
    final Matrix4f shadowProjection = new Matrix4f();
    final Matrix4f shadowProjectionInverse = new Matrix4f();
    final Matrix4f dhProjection = new Matrix4f();
    final Matrix4f dhProjectionInverse = new Matrix4f();
    final Matrix4f dhPreviousProjection = new Matrix4f();
    final Vector3f sunPosition = new Vector3f();
    final Vector3f moonPosition = new Vector3f();
    final Vector3f shadowLightPosition = new Vector3f();
    final Vector3f upPosition = new Vector3f();
    final Vector3f endFlashPosition = new Vector3f();
    float sunAngle;
    float shadowAngle;

    /** Creates the state with default settings; call {@link #configure} when a pack is activated. */
    public FrameState() {
        configure(UniformSettings.DEFAULT);
    }

    /**
     * Applies a pipeline's directives and restarts the smoothed values.
     *
     * @param settings directives of the active dimension pipeline
     */
    public void configure(UniformSettings settings) {
        this.settings = settings;
        this.wetness = new SmoothedFloat(settings.wetnessHalfLife(), settings.drynessHalfLife());
        this.eyeBrightnessX = new SmoothedFloat(settings.eyeBrightnessHalfLife(), settings.eyeBrightnessHalfLife());
        this.eyeBrightnessY = new SmoothedFloat(settings.eyeBrightnessHalfLife(), settings.eyeBrightnessHalfLife());
        this.centerDepthSmooth = new SmoothedFloat(settings.centerDepthHalfLife(), settings.centerDepthHalfLife());
    }

    /** @return the directives in use */
    public UniformSettings settings() {
        return settings;
    }

    /** Advances timers and trackers and derives the computed values from the inputs. */
    public void update() {
        timer.beginFrame(frameStartNanos);
        cameraTracker.update(cameraPosition);
        float dt = timer.frameTime();
        wetness.update(rainStrength, dt);
        eyeBrightnessX.update(eyeBlockLight * 16, dt);
        eyeBrightnessY.update(eyeSkyLight * 16, dt);
        centerDepthSmooth.update(centerDepth, dt);
        previousEndFlashIntensity = currentEndFlashIntensity;
        currentEndFlashIntensity = hasEndFlash ? endFlashIntensity : 0;

        if (hasPreviousMatrices) {
            gbufferPreviousModelView.set(gbufferModelView);
            gbufferPreviousProjection.set(gbufferProjection);
            dhPreviousProjection.set(dhProjection);
        }
        gbufferModelView.set(viewRotation);
        gbufferModelView.invert(gbufferModelViewInverse);
        MatrixConversions.reversedToGl(projection, zeroToOne, gbufferProjection);
        gbufferProjection.invert(gbufferProjectionInverse);
        if (dhActive && !settings.unifiedProjection()) {
            dhProjection.setPerspective(gbufferProjection.perspectiveFov(), gbufferProjection.m11() / gbufferProjection.m00(), dhNearPlane, dhFarPlane);
        } else {
            // Without Distant Horizons, and with the unified projection (whose far plane is already the DH far plane).
            dhProjection.set(gbufferProjection);
        }
        dhProjection.invert(dhProjectionInverse);
        if (!hasPreviousMatrices) {
            gbufferPreviousModelView.set(gbufferModelView);
            gbufferPreviousProjection.set(gbufferProjection);
            dhPreviousProjection.set(dhProjection);
            hasPreviousMatrices = true;
        }

        sunAngle = CelestialMath.sunAngle(sunAngleAttribute);
        shadowAngle = CelestialMath.shadowAngle(sunAngleAttribute, moonAngleAttribute);
        CelestialMath.celestialPosition(gbufferModelView, settings.sunPathRotation(), sunAngleAttribute, sunPosition);
        CelestialMath.celestialPosition(gbufferModelView, settings.sunPathRotation(), moonAngleAttribute, moonPosition);
        CelestialMath.upPosition(gbufferModelView, upPosition);
        if (inEnd && hasEndFlash) {
            CelestialMath.endFlashPosition(gbufferModelView, endFlashXAngle, endFlashYAngle, endFlashPosition);
        } else {
            endFlashPosition.zero();
        }
        boolean flashLight = inEnd && hasEndFlash && settings.endFlashShadows();
        shadowLightPosition.set(flashLight ? endFlashPosition : CelestialMath.isDay(sunAngleAttribute) ? sunPosition : moonPosition);
        updateShadowMatrices(flashLight);
    }

    private void updateShadowMatrices(boolean flashLight) {
        Vector3d camera = cameraPosition;
        if (flashLight) {
            ShadowMatrices.endFlashModelView(endFlashXAngle, endFlashYAngle, settings.shadowIntervalSize(), camera.x, camera.y, camera.z, shadowModelView);
        } else {
            ShadowMatrices.celestialModelView(shadowAngle, settings.sunPathRotation(), settings.shadowIntervalSize(), camera.x, camera.y, camera.z, shadowModelView);
        }
        shadowModelView.invert(shadowModelViewInverse);
        if (settings.shadowFov() != null) {
            ShadowMatrices.perspective(settings.shadowFov(), shadowProjection);
        } else {
            float distance = ShadowMatrices.minusOneDistance(dhActive, dhRenderDistance, renderDistanceBlocks);
            ShadowMatrices.Planes planes = ShadowMatrices.planes(settings.shadowNearPlane(), settings.shadowFarPlane(), distance);
            ShadowMatrices.orthographic(settings.shadowDistance(), planes, shadowProjection);
        }
        shadowProjection.invert(shadowProjectionInverse);
    }

    // ------------------------------------------------------------------ derived accessors

    /** @return the camera tracker ({@code cameraPosition} and friends) */
    public CameraTracker camera() {
        return cameraTracker;
    }

    /** @return the frame timer ({@code frameCounter}, {@code frameTime}, {@code frameTimeCounter}) */
    public FrameTimer timer() {
        return timer;
    }

    /** @return the smoothed rain strength ({@code wetness}) */
    public float wetness() {
        return wetness.get();
    }

    /** @return smoothed block light at the eyes, 0..240 */
    public float eyeBrightnessSmoothX() {
        return eyeBrightnessX.get();
    }

    /** @return smoothed sky light at the eyes, 0..240 */
    public float eyeBrightnessSmoothY() {
        return eyeBrightnessY.get();
    }

    /** @return the smoothed centre depth ({@code centerDepthSmooth}) */
    public float centerDepthSmooth() {
        return centerDepthSmooth.get();
    }

    /** @return {@code endFlashIntensity} of the previous frame */
    public float previousEndFlashIntensity() {
        return previousEndFlashIntensity;
    }

    /** @return {@code far}: the render distance, or the DH far plane with a unified projection */
    public float far() {
        return settings.unifiedProjection() && dhActive ? dhFarPlane : renderDistanceBlocks;
    }

    /** @return {@code gbufferModelView} */
    public Matrix4fc gbufferModelView() {
        return gbufferModelView;
    }

    /** @return {@code gbufferProjection} (GL convention) */
    public Matrix4fc gbufferProjection() {
        return gbufferProjection;
    }

    /** @return {@code dhProjection} (GL convention) */
    public Matrix4fc dhProjection() {
        return dhProjection;
    }

    /** @return {@code shadowModelView} */
    public Matrix4fc shadowModelView() {
        return shadowModelView;
    }

    /** @return {@code shadowProjection} (GL convention) */
    public Matrix4fc shadowProjection() {
        return shadowProjection;
    }
}
