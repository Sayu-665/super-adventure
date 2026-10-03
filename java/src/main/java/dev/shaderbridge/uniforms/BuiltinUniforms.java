package dev.shaderbridge.uniforms;

import dev.shaderbridge.model.GlslType;
import dev.shaderbridge.model.ScalarKind;
import java.time.LocalDateTime;
import java.util.Collection;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Optional;
import org.joml.Vector3dc;

/**
 * Providers for every builtin uniform of the sb-uniforms registry
 * ({@code crates/sb-uniforms/src/registry.rs}), computing each value from {@link FrameState} and
 * {@link DrawState} with Iris' definitions.
 */
public final class BuiltinUniforms {
    private static final GlslType FLOAT = GlslType.FLOAT;
    private static final GlslType INT = GlslType.INT;
    private static final GlslType BOOL = GlslType.BOOL;
    private static final GlslType VEC3 = GlslType.VEC3;
    private static final GlslType VEC4 = GlslType.VEC4;
    private static final GlslType IVEC2 = GlslType.vector(ScalarKind.INT, 2);
    private static final GlslType IVEC3 = GlslType.vector(ScalarKind.INT, 3);
    private static final GlslType IVEC4 = GlslType.vector(ScalarKind.INT, 4);
    private static final GlslType MAT3 = GlslType.matrix(3, 3);
    private static final GlslType MAT4 = GlslType.MAT4;
    /** {@code GL_LINEAR}, the fog mode of linear fog. */
    private static final int GL_LINEAR = 9729;
    /** {@code GL_EXP2}, the fog mode of exponential (water) fog. */
    private static final int GL_EXP2 = 2049;
    /** The near plane Minecraft uses. */
    private static final float NEAR_PLANE = 0.05f;

    private static final Map<String, BuiltinUniform> ALL = build();

    private BuiltinUniforms() {
    }

    /**
     * @param name a uniform name
     * @return the builtin of that name, if the registry has it
     */
    public static Optional<BuiltinUniform> get(String name) {
        return Optional.ofNullable(ALL.get(name));
    }

    /** @return every builtin, in registry order */
    public static Collection<BuiltinUniform> all() {
        return ALL.values();
    }

    private static Map<String, BuiltinUniform> build() {
        Registry r = new Registry();
        camera(r);
        player(r);
        system(r);
        ids(r);
        world(r);
        rendering(r);
        matrices(r);
        coreProfile(r);
        shaderBridge(r);
        return Collections.unmodifiableMap(r.entries);
    }

    private static void camera(Registry r) {
        r.frame("cameraPosition", VEC3, (f, d, o) -> o.putVec3(f.camera().current()));
        r.frame("previousCameraPosition", VEC3, (f, d, o) -> o.putVec3(f.camera().previous()));
        r.frame("eyeAltitude", FLOAT, (f, d, o) -> o.putFloat((float) f.camera().current().y()));
        r.frame("cameraPositionInt", IVEC3, (f, d, o) -> putFloor(o, f.camera().currentUnshifted()));
        r.frame("cameraPositionFract", VEC3, (f, d, o) -> putFract(o, f.camera().currentUnshifted()));
        r.frame("previousCameraPositionInt", IVEC3, (f, d, o) -> putFloor(o, f.camera().previousUnshifted()));
        r.frame("previousCameraPositionFract", VEC3, (f, d, o) -> putFract(o, f.camera().previousUnshifted()));
        r.frame("eyePosition", VEC3, (f, d, o) -> o.putVec3(f.eyePosition));
        r.frame("relativeEyePosition", VEC3, (f, d, o) -> putDifference(o, f.camera().currentUnshifted(), f.eyePosition));
        r.frame("playerLookVector", VEC3, (f, d, o) -> o.putVec3(f.playerLookVector));
        r.frame("playerBodyVector", VEC3, (f, d, o) -> o.putVec3(f.playerBodyVector));
        r.frame("vehicleLookVector", VEC3, (f, d, o) -> {
            if (f.hasVehicle) {
                o.putVec3(f.vehicleLookVector);
            }
        });
        r.frame("relativeVehiclePosition", VEC3, (f, d, o) -> {
            if (f.hasVehicle) {
                putDifference(o, f.camera().currentUnshifted(), f.vehiclePosition);
            }
        });
        r.frame("upPosition", VEC3, (f, d, o) -> o.putVec3(f.upPosition));
        r.frame("eyeBrightness", IVEC2, (f, d, o) -> o.putIvec2(f.eyeBlockLight * 16, f.eyeSkyLight * 16));
        r.frame("eyeBrightnessSmooth", IVEC2, (f, d, o) -> o.putIvec2((int) f.eyeBrightnessSmoothX(), (int) f.eyeBrightnessSmoothY()));
        r.frame("centerDepthSmooth", FLOAT, (f, d, o) -> o.putFloat(f.centerDepthSmooth()));
        r.frame("firstPersonCamera", BOOL, (f, d, o) -> o.putBool(f.firstPersonCamera));
    }

    private static void player(Registry r) {
        r.frame("isEyeInWater", INT, (f, d, o) -> o.putInt(f.isEyeInWater));
        r.frame("isSpectator", BOOL, (f, d, o) -> o.putBool(f.isSpectator));
        r.frame("isRightHanded", BOOL, (f, d, o) -> o.putBool(f.isRightHanded));
        r.frame("blindness", FLOAT, (f, d, o) -> o.putFloat(f.blindness));
        r.frame("darknessFactor", FLOAT, (f, d, o) -> o.putFloat(f.darknessFactor));
        r.frame("darknessLightFactor", FLOAT, (f, d, o) -> o.putFloat(f.darknessLightFactor));
        r.frame("nightVision", FLOAT, (f, d, o) -> o.putFloat(f.nightVision));
        r.frame("playerMood", FLOAT, (f, d, o) -> o.putFloat(f.playerMood));
        r.frame("constantMood", FLOAT, (f, d, o) -> o.putFloat(f.constantMood));
        r.frame("currentPlayerHealth", FLOAT, (f, d, o) -> o.putFloat(f.currentPlayerHealth));
        r.frame("maxPlayerHealth", FLOAT, (f, d, o) -> o.putFloat(f.maxPlayerHealth));
        r.frame("currentPlayerHunger", FLOAT, (f, d, o) -> o.putFloat(f.currentPlayerHunger));
        r.frame("maxPlayerHunger", FLOAT, (f, d, o) -> o.putFloat(20));
        r.frame("currentPlayerArmor", FLOAT, (f, d, o) -> o.putFloat(f.currentPlayerArmor));
        r.frame("maxPlayerArmor", FLOAT, (f, d, o) -> o.putFloat(50));
        r.frame("currentPlayerAir", FLOAT, (f, d, o) -> o.putFloat(f.currentPlayerAir));
        r.frame("maxPlayerAir", FLOAT, (f, d, o) -> o.putFloat(f.maxPlayerAir));
        r.frame("isRiding", BOOL, (f, d, o) -> o.putBool(f.isRiding));
        r.frame("vehicleInWater", BOOL, (f, d, o) -> o.putBool(f.vehicleInWater));
        r.frame("inSwimmingAnimation", BOOL, (f, d, o) -> o.putBool(f.inSwimmingAnimation));
        r.frame("feetInWater", BOOL, (f, d, o) -> o.putBool(f.feetInWater));
        r.frame("isElytraFlying", BOOL, (f, d, o) -> o.putBool(f.isElytraFlying));
        r.frame("heavyFog", BOOL, (f, d, o) -> o.putBool(f.heavyFog));
        r.frame("is_sneaking", BOOL, (f, d, o) -> o.putBool(f.isSneaking));
        r.frame("is_sprinting", BOOL, (f, d, o) -> o.putBool(f.isSprinting));
        r.frame("is_hurt", BOOL, (f, d, o) -> o.putBool(f.isHurt));
        r.frame("is_invisible", BOOL, (f, d, o) -> o.putBool(f.isInvisible));
        r.frame("is_burning", BOOL, (f, d, o) -> o.putBool(f.isBurning));
        r.frame("is_on_ground", BOOL, (f, d, o) -> o.putBool(f.isOnGround));
        r.frame("is_alive", BOOL, (f, d, o) -> o.putBool(f.isAlive));
        r.frame("is_child", BOOL, (f, d, o) -> o.putBool(f.isChild));
        r.frame("is_glowing", BOOL, (f, d, o) -> o.putBool(f.isGlowing));
        r.frame("is_in_lava", BOOL, (f, d, o) -> o.putBool(f.isInLava));
        r.frame("is_in_water", BOOL, (f, d, o) -> o.putBool(f.isInWater));
        r.frame("is_ridden", BOOL, (f, d, o) -> o.putBool(f.isRidden));
        r.frame("is_riding", BOOL, (f, d, o) -> o.putBool(f.isRiding));
        r.frame("is_wet", BOOL, (f, d, o) -> o.putBool(f.isWet));
        r.frame("hideGUI", BOOL, (f, d, o) -> o.putBool(f.hideGui));
    }

    private static void system(Registry r) {
        r.frame("viewWidth", FLOAT, (f, d, o) -> o.putFloat(f.viewWidth));
        r.frame("viewHeight", FLOAT, (f, d, o) -> o.putFloat(f.viewHeight));
        r.frame("aspectRatio", FLOAT, (f, d, o) -> o.putFloat((float) f.viewWidth / Math.max(1, f.viewHeight)));
        r.frame("screenBrightness", FLOAT, (f, d, o) -> o.putFloat(f.screenBrightness));
        r.frame("frameCounter", INT, (f, d, o) -> o.putInt(f.timer().frameCounter()));
        r.frame("frameTime", FLOAT, (f, d, o) -> o.putFloat(f.timer().frameTime()));
        r.frame("frameTimeCounter", FLOAT, (f, d, o) -> o.putFloat(f.timer().frameTimeCounter()));
        // ShaderBridge outputs sRGB, Iris' ColorSpace.SRGB (ordinal 0).
        r.frame("currentColorSpace", INT, (f, d, o) -> o.putInt(0));
        r.frame("currentDate", IVEC3, (f, d, o) -> o.putIvec3(f.localTime.getYear(), f.localTime.getMonthValue(), f.localTime.getDayOfMonth()));
        r.frame("currentTime", IVEC3, (f, d, o) -> o.putIvec3(f.localTime.getHour(), f.localTime.getMinute(), f.localTime.getSecond()));
        r.frame("currentYearTime", IVEC2, (f, d, o) -> {
            LocalDateTime t = f.localTime;
            int elapsed = (t.getDayOfYear() - 1) * 86400 + t.getHour() * 3600 + t.getMinute() * 60 + t.getSecond();
            o.putIvec2(elapsed, t.toLocalDate().lengthOfYear() * 86400 - elapsed);
        });
        r.frame("textureFilteringMode", INT, (f, d, o) -> o.putInt(f.textureFilteringMode));
        r.frame("anisotropicFiltering", INT, (f, d, o) -> o.putInt(f.anisotropicFiltering));
    }

    private static void ids(Registry r) {
        r.frame("heldItemId", INT, (f, d, o) -> o.putInt(f.heldItemId));
        r.frame("heldItemId2", INT, (f, d, o) -> o.putInt(f.heldItemId2));
        r.frame("heldBlockLightValue", INT, (f, d, o) -> o.putInt(f.heldBlockLightValue));
        r.frame("heldBlockLightValue2", INT, (f, d, o) -> o.putInt(f.heldBlockLightValue2));
        r.frame("heldBlockLightColor", VEC3, (f, d, o) -> o.putVec3(f.heldBlockLightColor));
        r.frame("heldBlockLightColor2", VEC3, (f, d, o) -> o.putVec3(f.heldBlockLightColor2));
        r.draw("entityId", INT, (f, d, o) -> o.putInt(d.entityId));
        r.draw("blockEntityId", INT, (f, d, o) -> o.putInt(d.blockEntityId));
        r.draw("currentRenderedItemId", INT, (f, d, o) -> o.putInt(d.currentRenderedItemId));
        r.frame("vehicleId", INT, (f, d, o) -> o.putInt(f.vehicleId));
        r.frame("currentSelectedBlockId", INT, (f, d, o) -> o.putInt(f.currentSelectedBlockId));
        r.frame("currentSelectedBlockPos", VEC3, (f, d, o) -> o.putVec3(f.currentSelectedBlockPos));
    }

    private static void world(Registry r) {
        r.frame("sunPosition", VEC3, (f, d, o) -> o.putVec3(f.sunPosition));
        r.frame("moonPosition", VEC3, (f, d, o) -> o.putVec3(f.moonPosition));
        r.frame("shadowLightPosition", VEC3, (f, d, o) -> o.putVec3(f.shadowLightPosition));
        r.frame("sunAngle", FLOAT, (f, d, o) -> o.putFloat(f.sunAngle));
        r.frame("shadowAngle", FLOAT, (f, d, o) -> o.putFloat(f.shadowAngle));
        r.frame("moonPhase", INT, (f, d, o) -> o.putInt(f.moonPhase));
        r.frame("worldTime", INT, (f, d, o) -> o.putInt(f.worldTime));
        r.frame("worldDay", INT, (f, d, o) -> o.putInt(f.worldDay));
        r.frame("rainStrength", FLOAT, (f, d, o) -> o.putFloat(f.rainStrength));
        r.frame("wetness", FLOAT, (f, d, o) -> o.putFloat(f.wetness()));
        r.frame("thunderStrength", FLOAT, (f, d, o) -> o.putFloat(f.thunderStrength));
        r.frame("lightningBoltPosition", VEC4, (f, d, o) -> o.putVec4(f.lightningBoltPosition));
        r.frame("endFlashPosition", VEC3, (f, d, o) -> o.putVec3(f.endFlashPosition));
        r.frame("endFlashIntensity", FLOAT, (f, d, o) -> o.putFloat(f.hasEndFlash ? f.endFlashIntensity : 0));
        r.frame("previousEndFlashIntensity", FLOAT, (f, d, o) -> o.putFloat(f.previousEndFlashIntensity()));
        r.frame("cloudTime", FLOAT, (f, d, o) -> o.putFloat(f.cloudTime));
        r.frame("cloudHeight", FLOAT, (f, d, o) -> o.putFloat(f.cloudHeight));
        r.frame("ambientLight", FLOAT, (f, d, o) -> o.putFloat(f.ambientLight));
        r.frame("bedrockLevel", INT, (f, d, o) -> o.putInt(f.bedrockLevel));
        r.frame("heightLimit", INT, (f, d, o) -> o.putInt(f.heightLimit));
        r.frame("logicalHeightLimit", INT, (f, d, o) -> o.putInt(f.logicalHeightLimit));
        r.frame("seaLevel", INT, (f, d, o) -> o.putInt(f.seaLevel));
        r.frame("hasCeiling", BOOL, (f, d, o) -> o.putBool(f.hasCeiling));
        r.frame("hasSkylight", BOOL, (f, d, o) -> o.putBool(f.hasSkylight));
        r.frame("bossBattle", INT, (f, d, o) -> o.putInt(f.bossBattle));
        r.frame("biome", INT, (f, d, o) -> o.putInt(f.biome));
        r.frame("biome_category", INT, (f, d, o) -> o.putInt(f.biomeCategory));
        r.frame("biome_precipitation", INT, (f, d, o) -> o.putInt(f.biomePrecipitation));
        r.frame("rainfall", FLOAT, (f, d, o) -> o.putFloat(f.rainfall));
        r.frame("temperature", FLOAT, (f, d, o) -> o.putFloat(f.temperature));
    }

    private static void rendering(Registry r) {
        r.frame("near", FLOAT, (f, d, o) -> o.putFloat(NEAR_PLANE));
        r.frame("far", FLOAT, (f, d, o) -> o.putFloat(f.far()));
        r.frame("fogColor", VEC3, (f, d, o) -> o.putVec3(f.fogColor));
        r.frame("skyColor", VEC3, (f, d, o) -> o.putVec3(f.skyColor));
        r.frame("fogMode", INT, (f, d, o) -> o.putInt(f.fogDensity < 0 ? GL_LINEAR : GL_EXP2));
        // Iris reports a stable "cylindrical" fog shape (1), whatever Minecraft uses internally.
        r.frame("fogShape", INT, (f, d, o) -> o.putInt(1));
        r.frame("fogDensity", FLOAT, (f, d, o) -> o.putFloat(Math.max(0, f.fogDensity)));
        r.frame("fogStart", FLOAT, (f, d, o) -> o.putFloat(f.fogStart));
        r.frame("fogEnd", FLOAT, (f, d, o) -> o.putFloat(f.fogEnd));
        r.draw("alphaTestRef", FLOAT, (f, d, o) -> o.putFloat(d.alphaTestRef));
        r.draw("entityColor", VEC4, (f, d, o) -> o.putVec4(d.entityColor));
        r.draw("blendFunc", IVEC4, (f, d, o) -> o.putIvec4(d.blendFunc[0], d.blendFunc[1], d.blendFunc[2], d.blendFunc[3]));
        r.draw("atlasSize", IVEC2, (f, d, o) -> o.putIvec2(d.atlasWidth, d.atlasHeight));
        r.draw("gtextureSize", IVEC2, (f, d, o) -> o.putIvec2(d.gtextureWidth, d.gtextureHeight));
        r.draw("gtextureId", INT, (f, d, o) -> o.putInt(d.gtextureId));
        r.frame("textureReloadCount", INT, (f, d, o) -> o.putInt(f.textureReloadCount));
        r.draw("renderStage", INT, (f, d, o) -> o.putInt(d.renderStage));
        r.frame("chunkFadeTimeInv", FLOAT, (f, d, o) -> o.putFloat(f.chunkFadeTimeInv));
        r.frame("pi", FLOAT, (f, d, o) -> o.putFloat((float) Math.PI));
        r.draw("spriteBounds", VEC4, (f, d, o) -> o.putVec4(d.spriteBounds));
        r.draw("instanceId", INT, (f, d, o) -> o.putInt(d.instanceId));
        // OptiFine documents both as unused; packs only test them for zero.
        r.frame("terrainTextureSize", IVEC2, (f, d, o) -> o.putIvec2(0, 0));
        r.frame("terrainIconSize", INT, (f, d, o) -> o.putInt(0));
    }

    private static void matrices(Registry r) {
        r.frame("gbufferModelView", MAT4, (f, d, o) -> o.putMat4(f.gbufferModelView));
        r.frame("gbufferModelViewInverse", MAT4, (f, d, o) -> o.putMat4(f.gbufferModelViewInverse));
        r.frame("gbufferPreviousModelView", MAT4, (f, d, o) -> o.putMat4(f.gbufferPreviousModelView));
        r.frame("gbufferProjection", MAT4, (f, d, o) -> o.putMat4(f.gbufferProjection));
        r.frame("gbufferProjectionInverse", MAT4, (f, d, o) -> o.putMat4(f.gbufferProjectionInverse));
        r.frame("gbufferPreviousProjection", MAT4, (f, d, o) -> o.putMat4(f.gbufferPreviousProjection));
        r.frame("shadowModelView", MAT4, (f, d, o) -> o.putMat4(f.shadowModelView));
        r.frame("shadowModelViewInverse", MAT4, (f, d, o) -> o.putMat4(f.shadowModelViewInverse));
        r.frame("shadowProjection", MAT4, (f, d, o) -> o.putMat4(f.shadowProjection));
        r.frame("shadowProjectionInverse", MAT4, (f, d, o) -> o.putMat4(f.shadowProjectionInverse));
        r.frame("dhProjection", MAT4, (f, d, o) -> o.putMat4(f.dhProjection));
        r.frame("dhProjectionInverse", MAT4, (f, d, o) -> o.putMat4(f.dhProjectionInverse));
        r.frame("dhPreviousProjection", MAT4, (f, d, o) -> o.putMat4(f.dhPreviousProjection));
        r.frame("dhNearPlane", FLOAT, (f, d, o) -> o.putFloat(f.dhNearPlane));
        r.frame("dhFarPlane", FLOAT, (f, d, o) -> o.putFloat(f.dhFarPlane));
        r.frame("dhRenderDistance", INT, (f, d, o) -> o.putInt(f.dhRenderDistance));
    }

    private static void coreProfile(Registry r) {
        r.draw("modelViewMatrix", MAT4, (f, d, o) -> o.putMat4(d.modelViewMatrix));
        r.draw("modelViewMatrixInverse", MAT4, (f, d, o) -> o.putMat4(d.modelViewMatrixInverse));
        r.draw("projectionMatrix", MAT4, (f, d, o) -> o.putMat4(d.projectionMatrix));
        r.draw("projectionMatrixInverse", MAT4, (f, d, o) -> o.putMat4(d.projectionMatrixInverse));
        r.draw("normalMatrix", MAT3, (f, d, o) -> o.putMat3(d.normalMatrix));
        r.draw("textureMatrix", MAT4, (f, d, o) -> o.putMat4(d.textureMatrix));
        r.draw("colorModulator", VEC4, (f, d, o) -> o.putVec4(d.colorModulator));
        r.draw("chunkOffset", VEC3, (f, d, o) -> o.putVec3(d.modelOffset));
        r.draw("modelOffset", VEC3, (f, d, o) -> o.putVec3(d.modelOffset));
    }

    /** ShaderBridge's replacements of fixed-function state ({@code gl_Fog}). */
    private static void shaderBridge(Registry r) {
        r.frame("sb_FogColor", VEC4, (f, d, o) -> o.putVec4(f.fogColor.x, f.fogColor.y, f.fogColor.z, f.fogAlpha));
        r.frame("fogScale", FLOAT, (f, d, o) -> o.putFloat(f.fogEnd != f.fogStart ? 1.0f / (f.fogEnd - f.fogStart) : 0.0f));
    }

    private static void putFloor(UniformWriter out, Vector3dc v) {
        out.putIvec3((int) Math.floor(v.x()), (int) Math.floor(v.y()), (int) Math.floor(v.z()));
    }

    private static void putFract(UniformWriter out, Vector3dc v) {
        out.putVec3(v.x() - Math.floor(v.x()), v.y() - Math.floor(v.y()), v.z() - Math.floor(v.z()));
    }

    private static void putDifference(UniformWriter out, Vector3dc a, Vector3dc b) {
        out.putVec3(a.x() - b.x(), a.y() - b.y(), a.z() - b.z());
    }

    /** Collects the entries in registration order and rejects duplicates. */
    private static final class Registry {
        final Map<String, BuiltinUniform> entries = new LinkedHashMap<>();

        void frame(String name, GlslType type, BuiltinUniform.Provider provider) {
            add(new BuiltinUniform(name, type, false, provider));
        }

        void draw(String name, GlslType type, BuiltinUniform.Provider provider) {
            add(new BuiltinUniform(name, type, true, provider));
        }

        private void add(BuiltinUniform uniform) {
            if (entries.put(uniform.name(), uniform) != null) {
                throw new IllegalStateException("duplicate builtin " + uniform.name());
            }
        }
    }
}
