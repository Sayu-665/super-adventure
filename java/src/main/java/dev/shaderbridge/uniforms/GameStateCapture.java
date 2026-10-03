package dev.shaderbridge.uniforms;

import com.mojang.blaze3d.systems.RenderSystem;
import java.time.LocalDateTime;
import java.util.Map;
import net.fabricmc.loader.api.FabricLoader;
import net.minecraft.client.Camera;
import net.minecraft.client.Minecraft;
import net.minecraft.client.Options;
import net.minecraft.client.gui.components.LerpingBossEvent;
import net.minecraft.client.multiplayer.ClientLevel;
import net.minecraft.client.player.LocalPlayer;
import net.minecraft.client.renderer.EndFlashState;
import net.minecraft.client.renderer.CloudRenderer;
import net.minecraft.client.renderer.GameRenderer;
import net.minecraft.client.renderer.fog.FogData;
import net.minecraft.client.renderer.state.level.CameraRenderState;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.component.DataComponents;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.network.chat.contents.TranslatableContents;
import net.minecraft.resources.Identifier;
import net.minecraft.tags.TagKey;
import net.minecraft.util.RandomSource;
import net.minecraft.world.InteractionHand;
import net.minecraft.world.attribute.AmbientMoodSettings;
import net.minecraft.world.attribute.EnvironmentAttributes;
import net.minecraft.world.effect.MobEffectInstance;
import net.minecraft.world.effect.MobEffects;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.HumanoidArm;
import net.minecraft.world.entity.LightningBolt;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.item.BlockItem;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.component.BlockItemStateProperties;
import net.minecraft.world.level.GameType;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.LightLayer;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.Property;
import net.minecraft.world.level.dimension.DimensionType;
import net.minecraft.world.level.material.FogType;
import net.minecraft.world.phys.BlockHitResult;
import net.minecraft.world.phys.HitResult;
import net.minecraft.world.phys.Vec3;
import org.joml.Matrix4fc;
import org.joml.Vector3d;
import org.joml.Vector3f;
import org.joml.Vector3fc;

/**
 * Reads the game into a {@link FrameState} once per frame, with Iris' definitions of every
 * value. The render integration calls {@link #capture} at the start of level rendering with the
 * camera state and the projection Minecraft renders the level with, then
 * {@link FrameState#update()}. {@link #tick} runs once per client tick for tick-driven values.
 */
public final class GameStateCapture {
    /** Iris' frame counter for the cloud animation: ticks per texture column. */
    private static final int CLOUD_TICKS_PER_CELL = 400;
    /** Width of the vanilla cloud texture, used before clouds are loaded. */
    private static final int DEFAULT_CLOUD_TEXTURE_WIDTH = 256;

    private IdMapLookup ids = IdMapLookup.EMPTY;
    private Map<String, Integer> biomeIds = Map.of();
    private final MoodTracker constantMood = new MoodTracker();
    private final RandomSource random = RandomSource.create();
    private final boolean distantHorizonsLoaded = FabricLoader.getInstance().isModLoaded("distanthorizons");
    private int textureReloadCount;

    /**
     * Switches to the id maps of a newly activated pack.
     *
     * @param ids      id maps of the active pack
     * @param biomeIds {@code BIOME_*} macro ids the pack was compiled with ({@link BiomeIds#fromMacros})
     */
    public void usePack(IdMapLookup ids, Map<String, Integer> biomeIds) {
        this.ids = ids;
        this.biomeIds = biomeIds;
    }

    /** Counts a resource reload ({@code textureReloadCount}). */
    public void onResourceReload() {
        textureReloadCount++;
    }

    /**
     * Per-tick work: samples the cave mood near the player as the vanilla ambient sound handler
     * does, without its reset ({@code constantMood}).
     *
     * @param minecraft the client
     */
    public void tick(Minecraft minecraft) {
        LocalPlayer player = minecraft.player;
        if (player == null || minecraft.level == null) {
            constantMood.reset();
            return;
        }
        if (minecraft.isPaused()) {
            return;
        }
        Level level = player.level();
        level.environmentAttributes().getValue(EnvironmentAttributes.AMBIENT_SOUNDS, player.position()).mood().ifPresent(mood -> sampleMood(level, player, mood));
    }

    private void sampleMood(Level level, LocalPlayer player, AmbientMoodSettings mood) {
        int extent = mood.blockSearchExtent();
        int span = extent * 2 + 1;
        BlockPos pos = BlockPos.containing(
            player.getX() + random.nextInt(span) - extent,
            player.getEyeY() + random.nextInt(span) - extent,
            player.getZ() + random.nextInt(span) - extent);
        constantMood.tick(level.getBrightness(LightLayer.SKY, pos), level.getBrightness(LightLayer.BLOCK, pos), mood.tickDelay());
    }

    /**
     * Captures the frame's inputs.
     *
     * @param state       destination
     * @param camera      the level's camera render state
     * @param projection  the projection the level is rendered with (Minecraft's reversed-Z form, view bobbing applied)
     * @param partialTick the frame's partial tick
     */
    public void capture(FrameState state, CameraRenderState camera, Matrix4fc projection, float partialTick) {
        Minecraft mc = Minecraft.getInstance();
        state.frameStartNanos = System.nanoTime();
        state.partialTick = partialTick;
        state.localTime = LocalDateTime.now();
        state.textureReloadCount = textureReloadCount;
        captureScreen(state, mc);
        state.cameraPosition.set(camera.pos.x, camera.pos.y, camera.pos.z);
        state.viewRotation.set(camera.viewRotationMatrix);
        state.projection.set(projection);
        state.zeroToOne = RenderSystem.getDevice().getDeviceInfo().isZZeroToOne();
        state.firstPersonCamera = camera.isFirstPerson;
        state.renderDistanceBlocks = mc.options.getEffectiveRenderDistance() * 16.0f;
        captureFog(state, camera.fogData, camera.fogType, mc);
        // Without Distant Horizons, Iris reports 0.01 for both planes and the vanilla distance.
        state.dhActive = false;
        state.dhNearPlane = 0.01f;
        state.dhFarPlane = 0.01f;
        state.dhRenderDistance = (int) state.renderDistanceBlocks;
        if (distantHorizonsLoaded) {
            DistantHorizonsInfo.capture(state, partialTick);
        }
        ClientLevel level = mc.level;
        Entity entity = mc.getCameraEntity();
        if (level == null || entity == null) {
            return;
        }
        Camera mainCamera = mc.gameRenderer.mainCamera();
        captureEntity(state, mc, entity, partialTick);
        capturePlayer(state, mc, camera.fogType, partialTick);
        captureWorld(state, mc, level, mainCamera, partialTick);
        captureBiome(state, mc);
        captureIds(state, mc, level);
    }

    private static void captureScreen(FrameState s, Minecraft mc) {
        Options options = mc.options;
        s.viewWidth = mc.gameRenderer.mainRenderTarget().width;
        s.viewHeight = mc.gameRenderer.mainRenderTarget().height;
        s.screenBrightness = options.gamma().get().floatValue();
        s.textureFilteringMode = switch (options.textureFiltering().get()) {
            case NONE -> 0;
            case RGSS -> 1;
            case ANISOTROPIC -> 2;
        };
        s.anisotropicFiltering = s.textureFilteringMode == 2 ? options.maxAnisotropyValue() : 0;
        double fadeSeconds = options.chunkSectionFadeInTime().get();
        // Iris' formula; a disabled fade reports 0 instead of infinity.
        s.chunkFadeTimeInv = fadeSeconds > 0 ? (float) (1.0 / (fadeSeconds * 1000.0)) : 0.0f;
        s.hideGui = mc.gui.hud.isHidden();
        s.isRightHanded = options.mainHand().get() == HumanoidArm.RIGHT;
    }

    private static void captureFog(FrameState s, FogData fog, FogType fogType, Minecraft mc) {
        s.fogColor.set(fog.color.x, fog.color.y, fog.color.z);
        s.fogAlpha = fog.color.w;
        s.fogStart = fog.environmentalStart;
        s.fogEnd = fog.environmentalEnd;
        if (fogType == FogType.WATER) {
            float vision = mc.player != null ? mc.player.getWaterVision() : 0;
            s.fogDensity = 0.05f - vision * vision * 0.03f;
        } else {
            s.fogDensity = -1;
        }
    }

    private static void captureEntity(FrameState s, Minecraft mc, Entity entity, float pt) {
        set(s.eyePosition, entity.getEyePosition(pt));
        if (entity instanceof LivingEntity) {
            set(s.playerLookVector, entity.getViewVector(pt));
        } else {
            s.playerLookVector.zero();
        }
        set(s.playerBodyVector, entity.getForward());
        Entity vehicle = mc.player != null ? mc.player.getVehicle() : null;
        s.hasVehicle = vehicle != null;
        if (vehicle != null) {
            set(s.vehicleLookVector, vehicle.getForward());
            set(s.vehiclePosition, vehicle.getPosition(pt));
        }
        BlockPos eyes = BlockPos.containing(entity.position().x, entity.getEyeY(), entity.position().z);
        s.eyeBlockLight = entity.level().getBrightness(LightLayer.BLOCK, eyes);
        s.eyeSkyLight = entity.level().getBrightness(LightLayer.SKY, eyes);
        s.blindness = blindness(entity);
        s.darknessFactor = entity instanceof LivingEntity living && living.getEffect(MobEffects.DARKNESS) != null
            ? living.getEffect(MobEffects.DARKNESS).getBlendFactor(living, pt) : 0;
        s.nightVision = nightVision(mc, entity, pt);
        s.playerMood = entity instanceof LocalPlayer player ? Math.clamp(player.getCurrentMood(), 0, 1) : 0;
        s.isChild = entity instanceof LivingEntity living && living.isBaby();
    }

    private static float blindness(Entity entity) {
        if (entity instanceof LivingEntity living) {
            MobEffectInstance effect = living.getEffect(MobEffects.BLINDNESS);
            if (effect != null) {
                return effect.isInfiniteDuration() ? 1.0f : Math.clamp(effect.getDuration() / 20.0f, 0.0f, 1.0f);
            }
        }
        return 0;
    }

    private static float nightVision(Minecraft mc, Entity entity, float pt) {
        if (entity instanceof LivingEntity living && living.hasEffect(MobEffects.NIGHT_VISION)) {
            float strength = GameRenderer.nightVisionScale(living, pt);
            if (strength > 0) {
                return Math.clamp(strength, 0.0f, 1.0f);
            }
        }
        // Conduit power acts as night vision under water, as in the lightmap.
        if (mc.player != null && mc.player.hasEffect(MobEffects.CONDUIT_POWER)) {
            return Math.clamp(mc.player.getWaterVision(), 0.0f, 1.0f);
        }
        return 0;
    }

    private void capturePlayer(FrameState s, Minecraft mc, FogType fogType, float pt) {
        LocalPlayer player = mc.player;
        s.constantMood = constantMood.value();
        if (player == null) {
            return;
        }
        s.isSpectator = mc.gameMode != null && mc.gameMode.getPlayerMode() == GameType.SPECTATOR;
        s.isEyeInWater = switch (fogType) {
            case WATER -> 1;
            case LAVA -> player.isSpectator() ? 0 : 2;
            case POWDER_SNOW -> 3;
            default -> 0;
        };
        float darknessScale = mc.options.darknessEffectScale().get().floatValue();
        float darknessGamma = player.getEffectBlendFactor(MobEffects.DARKNESS, pt) * darknessScale;
        s.darknessLightFactor = Math.max(0.0f, (float) Math.cos((player.tickCount - pt) * Math.PI * 0.025) * 0.45f * darknessGamma) * darknessScale;
        boolean survival = mc.gameMode != null && mc.gameMode.getPlayerMode().isSurvival();
        s.currentPlayerHealth = survival ? player.getHealth() / player.getMaxHealth() : -1;
        s.maxPlayerHealth = survival ? player.getMaxHealth() : -1;
        s.currentPlayerHunger = survival ? player.getFoodData().getFoodLevel() / 20.0f : -1;
        s.currentPlayerArmor = survival ? player.getArmorValue() / 50.0f : -1;
        s.currentPlayerAir = survival ? (float) player.getAirSupply() / player.getMaxAirSupply() : -1;
        s.maxPlayerAir = survival ? player.getMaxAirSupply() : -1;
        s.isRiding = player.isPassenger();
        s.vehicleInWater = player.getVehicle() != null && player.getVehicle().isInShallowWater();
        s.inSwimmingAnimation = player.isSwimming();
        s.feetInWater = player.isInShallowWater();
        s.isElytraFlying = player.isFallFlying();
        s.heavyFog = mc.gui.hud.getBossOverlay().shouldCreateWorldFog();
        s.isSneaking = player.isCrouching();
        s.isSprinting = player.isSprinting();
        s.isHurt = player.hurtTime > 0;
        s.isInvisible = player.isInvisible();
        s.isBurning = player.isOnFire();
        s.isOnGround = player.onGround();
        s.isAlive = player.isAlive();
        s.isGlowing = player.isCurrentlyGlowing();
        s.isInLava = player.isInLava();
        s.isInWater = player.isInWater();
        s.isRidden = player.isVehicle();
        s.isWet = player.isInWaterOrRain();
        s.bossBattle = bossBattle(mc);
    }

    /** OptiFine's {@code bossBattle}: 1 custom, 2 ender dragon, 3 wither, 4 raid, 0 without a boss bar. */
    private static int bossBattle(Minecraft mc) {
        for (LerpingBossEvent event : mc.gui.hud.getBossOverlay().events.values()) {
            if (event.getName().getContents() instanceof TranslatableContents name) {
                String key = name.getKey();
                if (key.equals("entity.minecraft.ender_dragon")) {
                    return 2;
                } else if (key.equals("entity.minecraft.wither")) {
                    return 3;
                } else if (key.startsWith("event.minecraft.raid")) {
                    return 4;
                }
            }
            return 1;
        }
        return 0;
    }

    private static void captureWorld(FrameState s, Minecraft mc, ClientLevel level, Camera camera, float pt) {
        s.sunAngleAttribute = camera.attributeProbe().getValue(EnvironmentAttributes.SUN_ANGLE, pt);
        s.moonAngleAttribute = camera.attributeProbe().getValue(EnvironmentAttributes.MOON_ANGLE, pt);
        s.moonPhase = camera.attributeProbe().getValue(EnvironmentAttributes.MOON_PHASE, pt).index();
        s.cloudHeight = camera.attributeProbe().getValue(EnvironmentAttributes.CLOUD_HEIGHT, pt);
        set(s.skyColor, camera.attributeProbe().getValue(EnvironmentAttributes.SKY_COLOR, pt));

        DimensionType dimension = level.dimensionType();
        long time = level.getDefaultClockTime();
        boolean keepsRealTime = level.dimension() == Level.END || level.dimension() == Level.NETHER;
        s.worldTime = (int) (keepsRealTime || !dimension.hasFixedTime() ? time % 24000L : 0L);
        s.worldDay = (int) (time / 24000L);
        s.rainStrength = Math.clamp(level.getRainLevel(pt), 0.0f, 1.0f);
        s.thunderStrength = Math.clamp(level.getThunderLevel(pt), 0.0f, 1.0f);
        s.bedrockLevel = dimension.minY();
        s.heightLimit = dimension.height();
        s.logicalHeightLimit = dimension.logicalHeight();
        s.hasCeiling = dimension.hasCeiling();
        s.hasSkylight = dimension.hasSkyLight();
        s.ambientLight = dimension.ambientLight();
        s.seaLevel = level.getSeaLevel();

        s.inEnd = level.dimension() == Level.END;
        EndFlashState flash = level.endFlashState();
        s.hasEndFlash = flash != null;
        if (flash != null) {
            s.endFlashXAngle = flash.getXAngle();
            s.endFlashYAngle = flash.getYAngle();
            s.endFlashIntensity = flash.getIntensity(pt);
        }

        s.lightningBoltPosition.zero();
        Vec3 cameraPos = camera.position();
        for (Entity entity : level.entitiesForRendering()) {
            if (entity instanceof LightningBolt bolt) {
                Vec3 pos = bolt.getPosition(pt);
                s.lightningBoltPosition.set((float) (pos.x - cameraPos.x), (float) (pos.y - cameraPos.y), (float) (pos.z - cameraPos.z), 1.0f);
                break;
            }
        }

        CloudRenderer.TextureData clouds = mc.levelRenderer.cloudRenderer().texture;
        int cloudWidth = clouds != null ? clouds.width() : DEFAULT_CLOUD_TEXTURE_WIDTH;
        s.cloudTime = (level.getGameTime() % ((long) cloudWidth * CLOUD_TICKS_PER_CELL) + pt) * 0.03f;
    }

    private void captureBiome(FrameState s, Minecraft mc) {
        LocalPlayer player = mc.player;
        if (player == null) {
            return;
        }
        BlockPos pos = player.blockPosition();
        Holder<Biome> holder = player.level().getBiome(pos);
        Biome biome = holder.value();
        s.biome = holder.unwrapKey().map(key -> biomeIds.getOrDefault(BiomeIds.macroName(key.identifier().toString()), 0)).orElse(0);
        s.biomeCategory = BiomeCategory.of(holder).ordinal();
        s.biomePrecipitation = switch (biome.getPrecipitationAt(pos, player.level().getSeaLevel())) {
            case NONE -> 0;
            case RAIN -> 1;
            case SNOW -> 2;
        };
        s.rainfall = biome.climateSettings.downfall();
        s.temperature = biome.getBaseTemperature();
    }

    private void captureIds(FrameState s, Minecraft mc, ClientLevel level) {
        LocalPlayer player = mc.player;
        if (player == null) {
            s.heldItemId = -1;
            s.heldItemId2 = -1;
            return;
        }
        ItemStack main = player.getItemInHand(InteractionHand.MAIN_HAND);
        ItemStack off = player.getItemInHand(InteractionHand.OFF_HAND);
        s.heldItemId = itemId(main);
        s.heldItemId2 = itemId(off);
        int mainLight = lightEmission(main);
        int offLight = lightEmission(off);
        // With oldHandLight the main hand reports the brighter of both hands. Light colors stay white.
        s.heldBlockLightValue = s.settings().oldHandLight() ? Math.max(mainLight, offLight) : mainLight;
        s.heldBlockLightValue2 = offLight;
        Entity vehicle = player.getVehicle();
        s.vehicleId = vehicle == null ? 0 : ids.entity(BuiltInRegistries.ENTITY_TYPE.getKey(vehicle.getType()).toString());

        s.currentSelectedBlockId = 0;
        s.currentSelectedBlockPos.set(-256);
        HitResult hit = mc.hitResult;
        if (!s.hideGui && hit != null && hit.getType() == HitResult.Type.BLOCK) {
            BlockPos pos = ((BlockHitResult) hit).getBlockPos();
            BlockState state = level.getBlockState(pos);
            if (!state.isAir() && level.getWorldBorder().isWithinBounds(pos)) {
                s.currentSelectedBlockId = blockId(state);
                Vec3 camera = mc.gameRenderer.mainCamera().position();
                s.currentSelectedBlockPos.set((float) (pos.getX() + 0.5 - camera.x), (float) (pos.getY() + 0.5 - camera.y), (float) (pos.getZ() + 0.5 - camera.z));
            }
        }
    }

    /**
     * Iris' held item id: the item model when the stack has one, else the item; an empty hand is
     * looked up as {@code minecraft:air}, so packs can map it.
     */
    private int itemId(ItemStack stack) {
        Identifier model = stack.isEmpty() ? null : stack.get(DataComponents.ITEM_MODEL);
        Identifier id = model != null ? model : BuiltInRegistries.ITEM.getKey(stack.getItem());
        return ids.item(id.toString());
    }

    /**
     * Light emitted by a held block item, with the stack's block state properties applied (a light
     * block item of level 7 emits 7), as Iris' default {@code IrisItemLightProvider} computes it.
     */
    private static int lightEmission(ItemStack stack) {
        if (!(stack.getItem() instanceof BlockItem block)) {
            return 0;
        }
        BlockState state = block.getBlock().defaultBlockState();
        BlockItemStateProperties properties = stack.get(DataComponents.BLOCK_STATE);
        if (properties != null) {
            state = properties.apply(state);
        }
        return state.getLightEmission();
    }

    private int blockId(BlockState state) {
        return ids.block(
            BuiltInRegistries.BLOCK.getKey(state.getBlock()).toString(),
            name -> propertyValue(state, name),
            tag -> state.typeHolder().is(TagKey.create(Registries.BLOCK, Identifier.parse(tag))));
    }

    private static String propertyValue(BlockState state, String name) {
        for (Property<?> property : state.getProperties()) {
            if (property.getName().equals(name)) {
                return valueName(state, property);
            }
        }
        return null;
    }

    private static <T extends Comparable<T>> String valueName(BlockState state, Property<T> property) {
        return property.getName(state.getValue(property));
    }

    private static void set(Vector3d dest, Vec3 v) {
        dest.set(v.x, v.y, v.z);
    }

    private static void set(Vector3f dest, Vector3fc v) {
        dest.set(v);
    }
}
