package dev.shaderbridge.pack;

import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.renderpearl.api.device.DeviceInfo;
import dev.shaderbridge.config.DepthModeSetting;
import dev.shaderbridge.model.CompileEnvironment;
import dev.shaderbridge.model.DeviceCaps;
import dev.shaderbridge.model.OutputTarget;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import net.fabricmc.loader.api.FabricLoader;
import net.minecraft.util.Util;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/** Builds the {@link CompileEnvironment} of the running game. Call on the render thread. */
public final class CompileEnvironmentFactory {
    /** Mojang's per-pipeline descriptor limit (push descriptors). */
    private static final int MOJANG_MAX_DESCRIPTORS = 32;
    /** The Vulkan-guaranteed minimum of {@code maxPushConstantsSize}. */
    private static final int MIN_PUSH_CONSTANTS = 128;
    private static final Logger LOGGER = LoggerFactory.getLogger("ShaderBridge");

    private CompileEnvironmentFactory() {
    }

    /**
     * @param depthMode the user's depth mode setting
     * @param biomeIds  {@code BIOME_*} macro ids of the known biomes
     * @return the environment of the current device, platform and mod set
     */
    public static CompileEnvironment create(DepthModeSetting depthMode, Map<String, Integer> biomeIds) {
        DeviceInfo info = RenderSystem.getDevice().getDeviceInfo();
        boolean vulkan = "Vulkan".equalsIgnoreCase(info.backendName());
        if (depthMode == DepthModeSetting.AUTO && !info.isZZeroToOne()) {
            LOGGER.warn("The {} device has no [0,1] clip control; shaders keep GL depth and cannot share Minecraft's depth buffer", info.backendName());
        }
        return new CompileEnvironment(
            minecraftVersion(),
            GpuIdentity.os(Util.getPlatform().name()),
            GpuIdentity.vendor(info.vendorName()),
            GpuIdentity.renderer(info.name()),
            FabricLoader.getInstance().isModLoaded("distanthorizons"),
            macros(biomeIds),
            vulkan ? List.of(OutputTarget.VULKAN, OutputTarget.RENDERPEARL) : List.of(OutputTarget.RENDERPEARL),
            depthMode.resolve(info.isZZeroToOne()),
            deviceCaps(info));
    }

    /**
     * Pack pipelines run on Minecraft's device, so only features Mojang enables on it count:
     * geometry and tessellation shaders, format-less storage images, depth clip control and
     * comparison samplers are not available on 26.3.
     */
    private static DeviceCaps deviceCaps(DeviceInfo info) {
        return new DeviceCaps(false, false, false, false, false, MIN_PUSH_CONSTANTS, info.limits().maxColorAttachments(), false, MOJANG_MAX_DESCRIPTORS);
    }

    private static Map<String, String> macros(Map<String, Integer> biomeIds) {
        Map<String, String> macros = new LinkedHashMap<>();
        biomeIds.forEach((name, id) -> macros.put(name, Integer.toString(id)));
        return macros;
    }

    private static String minecraftVersion() {
        return FabricLoader.getInstance().getModContainer("minecraft")
            .map(mod -> mod.getMetadata().getVersion().getFriendlyString())
            .orElse("26.3");
    }
}
