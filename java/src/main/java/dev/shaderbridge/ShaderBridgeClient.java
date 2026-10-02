package dev.shaderbridge;

import com.mojang.renderpearl.api.device.GpuDevice;
import com.seibel.distanthorizons.api.DhApi;
import net.fabricmc.api.ClientModInitializer;

public final class ShaderBridgeClient implements ClientModInitializer {
    @Override
    public void onInitializeClient() {
        Class<?> device = GpuDevice.class;
        Class<?> dh = DhApi.class;
    }
}
