package dev.shaderbridge.pack;

import java.util.Locale;

/**
 * Maps device and platform names to the suffixes of the OptiFine standard macros
 * {@code MC_OS_*}, {@code MC_GL_VENDOR_*} and {@code MC_GL_RENDERER_*}.
 *
 * <p>On Vulkan, Minecraft reports the vendor by PCI vendor id ({@code NVIDIA}, {@code AMD},
 * {@code INTEL}, ...) and the device name as the renderer; on OpenGL it reports
 * {@code GL_VENDOR} and {@code GL_RENDERER}. Both spellings map to the same macros.
 */
public final class GpuIdentity {
    private GpuIdentity() {
    }

    /**
     * @param platform name of Minecraft's {@code Util.OS} constant ({@code WINDOWS}, {@code OSX}, ...)
     * @return {@code WINDOWS}, {@code MAC}, {@code LINUX} or {@code UNKNOWN}
     */
    public static String os(String platform) {
        return switch (platform) {
            case "WINDOWS" -> "WINDOWS";
            case "OSX" -> "MAC";
            case "LINUX" -> "LINUX";
            default -> "UNKNOWN";
        };
    }

    /**
     * @param vendorName {@code DeviceInfo.vendorName()}
     * @return {@code ATI}, {@code NVIDIA}, {@code AMD}, {@code INTEL}, {@code XORG} or {@code OTHER}
     */
    public static String vendor(String vendorName) {
        String v = vendorName == null ? "" : vendorName.toLowerCase(Locale.ROOT);
        if (v.startsWith("ati")) {
            return "ATI";
        } else if (v.contains("nvidia")) {
            return "NVIDIA";
        } else if (v.startsWith("amd") || v.contains("advanced micro devices")) {
            return "AMD";
        } else if (v.contains("intel")) {
            return "INTEL";
        } else if (v.startsWith("x.org")) {
            return "XORG";
        }
        return "OTHER";
    }

    /**
     * @param deviceName {@code DeviceInfo.name()} (Vulkan device name or {@code GL_RENDERER})
     * @return {@code QUADRO}, {@code GEFORCE}, {@code RADEON}, {@code GALLIUM}, {@code INTEL},
     *     {@code MESA}, {@code APPLE} or {@code OTHER}
     */
    public static String renderer(String deviceName) {
        String r = deviceName == null ? "" : deviceName.toLowerCase(Locale.ROOT);
        if (r.contains("quadro") || r.startsWith("nvs")) {
            return "QUADRO";
        } else if (r.contains("geforce") || r.startsWith("nvidia")) {
            return "GEFORCE";
        } else if (r.startsWith("amd") || r.startsWith("ati") || r.contains("radeon")) {
            return "RADEON";
        } else if (r.startsWith("gallium") || r.startsWith("llvmpipe") || r.startsWith("softpipe")) {
            return "GALLIUM";
        } else if (r.startsWith("intel")) {
            return "INTEL";
        } else if (r.startsWith("mesa")) {
            return "MESA";
        } else if (r.startsWith("apple")) {
            return "APPLE";
        }
        return "OTHER";
    }
}
