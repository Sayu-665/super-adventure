package dev.shaderbridge.dh;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotSame;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.mojang.renderpearl.api.GpuFormat;
import com.mojang.renderpearl.api.commands.CommandEncoder;
import com.mojang.renderpearl.api.device.GpuDevice;
import com.mojang.renderpearl.api.textures.GpuTexture;
import com.mojang.renderpearl.api.textures.GpuTextureView;
import java.lang.reflect.Proxy;
import java.util.ArrayList;
import java.util.List;
import org.junit.jupiter.api.Test;

/** {@link DhDepthTargets}: the LOD depth attachment and the {@code dhDepthTex0/1} copies, without a GPU. */
class DhDepthTargetsTest {
    /** A created texture: label, usage, format and size. */
    private record Created(String label, int usage, GpuFormat format, int width, int height) {
    }

    private final List<Created> created = new ArrayList<>();
    private final List<String> commands = new ArrayList<>();
    private final List<GpuTexture> textures = new ArrayList<>();

    private final GpuDevice device = (GpuDevice) Proxy.newProxyInstance(GpuDevice.class.getClassLoader(), new Class<?>[] {GpuDevice.class},
        (proxy, method, args) -> switch (method.getName()) {
            case "createTexture" -> {
                Created c = new Created((String) args[0], (int) args[1], (GpuFormat) args[2], (int) args[3], (int) args[4]);
                created.add(c);
                GpuTexture texture = texture(c);
                textures.add(texture);
                yield texture;
            }
            case "createTextureView" -> Proxy.newProxyInstance(GpuTextureView.class.getClassLoader(), new Class<?>[] {GpuTextureView.class},
                (p, m, a) -> m.getName().equals("texture") ? args[0] : null);
            default -> throw new UnsupportedOperationException(method.getName());
        });

    private final CommandEncoder encoder = (CommandEncoder) Proxy.newProxyInstance(CommandEncoder.class.getClassLoader(),
        new Class<?>[] {CommandEncoder.class}, (proxy, method, args) -> {
            switch (method.getName()) {
                case "clearDepthTexture" -> commands.add("clear " + label(args[0]) + " " + args[1]);
                case "copyTextureToTexture" -> commands.add("copy " + label(args[0]) + " -> " + label(args[1]) + " " + args[7] + "x" + args[8]);
                default -> throw new UnsupportedOperationException(method.getName());
            }
            return null;
        });

    private static GpuTexture texture(Created c) {
        return (GpuTexture) Proxy.newProxyInstance(GpuTexture.class.getClassLoader(), new Class<?>[] {GpuTexture.class},
            (proxy, method, args) -> switch (method.getName()) {
                case "getWidth" -> c.width();
                case "getHeight" -> c.height();
                case "getFormat" -> c.format();
                case "getLabel" -> c.label();
                case "toString" -> c.label();
                default -> null;
            });
    }

    private static String label(Object texture) {
        return ((GpuTexture) texture).getLabel().replace("ShaderBridge ", "");
    }

    @Test
    void depthTexturesAreScreenSizedD32AndClearedWhenCreated() {
        DhDepthTargets targets = new DhDepthTargets(device);
        targets.beginFrame(encoder, 1920, 1080, 0.0);
        assertEquals(3, created.size());
        for (Created c : created) {
            assertEquals(GpuFormat.D32_FLOAT, c.format(), "Mojang's Vulkan pipelines expect D32 depth");
            assertEquals(1920, c.width());
            assertEquals(1080, c.height());
            assertTrue((c.usage() & GpuTexture.USAGE_RENDER_ATTACHMENT) != 0 && (c.usage() & GpuTexture.USAGE_TEXTURE_BINDING) != 0);
        }
        assertEquals(List.of("clear DH depth 0.0", "clear dhDepthTex0 0.0", "clear dhDepthTex1 0.0"), commands);
    }

    @Test
    void laterFramesClearTheAttachmentOnlyAndCopiesFollowTheLodPasses() {
        DhDepthTargets targets = new DhDepthTargets(device);
        targets.beginFrame(encoder, 800, 600, 1.0);
        GpuTextureView attachment = targets.attachment();
        commands.clear();
        targets.beginFrame(encoder, 800, 600, 1.0);
        assertSame(attachment, targets.attachment());
        targets.copy(encoder, 1);
        targets.copy(encoder, 0);
        assertEquals(List.of("clear DH depth 1.0", "copy DH depth -> dhDepthTex1 800x600", "copy DH depth -> dhDepthTex0 800x600"), commands,
            "the copies keep last frame's LODs until the passes copy again, as in Iris");
    }

    @Test
    void resizeRecreatesTheTextures() {
        DhDepthTargets targets = new DhDepthTargets(device);
        targets.beginFrame(encoder, 800, 600, 0.0);
        GpuTextureView before = targets.sampled(0);
        targets.beginFrame(encoder, 1024, 768, 0.0);
        assertNotSame(before, targets.sampled(0));
        assertEquals(6, created.size());
        assertEquals(1024, created.getLast().width());
    }
}
