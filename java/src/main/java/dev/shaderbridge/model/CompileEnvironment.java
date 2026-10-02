package dev.shaderbridge.model;

import java.util.List;
import java.util.Map;

/**
 * Host environment that affects compilation (standard macros and feature availability).
 *
 * @param minecraftVersion Minecraft version string, e.g. {@code 26.3}
 * @param os               {@code MC_OS_*} suffix: {@code WINDOWS}, {@code MAC}, {@code LINUX} or {@code UNKNOWN}
 * @param vendor           {@code MC_GL_VENDOR_*} suffix, e.g. {@code NVIDIA}
 * @param renderer         {@code MC_GL_RENDERER_*} suffix, e.g. {@code GEFORCE}
 * @param distantHorizons  Distant Horizons is loaded and rendering
 * @param extraMacros      extra macros: name to optional value (null value = defined without value)
 * @param targets          output targets to generate
 * @param depthMode        depth convention of the host
 * @param device           device capabilities relevant to translation
 */
public record CompileEnvironment(
    String minecraftVersion,
    String os,
    String vendor,
    String renderer,
    boolean distantHorizons,
    Map<String, String> extraMacros,
    List<OutputTarget> targets,
    DepthMode depthMode,
    DeviceCaps device
) {
    public CompileEnvironment {
        Copies.required(minecraftVersion, "minecraft_version");
        Copies.required(os, "os");
        Copies.required(vendor, "vendor");
        Copies.required(renderer, "renderer");
        extraMacros = Copies.map(extraMacros);
        targets = Copies.list(targets);
        Copies.required(depthMode, "depth_mode");
        Copies.required(device, "device");
    }
}
