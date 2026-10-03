package dev.shaderbridge.render.targets;

import com.mojang.renderpearl.api.GpuFormat;
import com.mojang.renderpearl.api.commands.CommandEncoder;
import com.mojang.renderpearl.api.device.GpuDevice;
import com.mojang.renderpearl.api.textures.GpuTexture;
import com.mojang.renderpearl.api.textures.GpuTextureView;
import java.lang.reflect.Proxy;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;

/** A device that creates texture objects without a GPU and an encoder that records its commands. */
final class FakeGpu {
    /** A texture. */
    static final class Texture implements GpuTexture {
        final String label;
        final int usage;
        final GpuFormat format;
        final int width;
        final int height;
        final int mips;
        boolean closed;

        Texture(String label, int usage, GpuFormat format, int width, int height, int mips) {
            this.label = label;
            this.usage = usage;
            this.format = format;
            this.width = width;
            this.height = height;
            this.mips = mips;
        }

        @Override
        public int getWidth(int mipLevel) {
            return Math.max(1, width >> mipLevel);
        }

        @Override
        public int getHeight(int mipLevel) {
            return Math.max(1, height >> mipLevel);
        }

        @Override
        public int getDepthOrLayers() {
            return 1;
        }

        @Override
        public int getMipLevels() {
            return mips;
        }

        @Override
        public GpuFormat getFormat() {
            return format;
        }

        @Override
        public int usage() {
            return usage;
        }

        @Override
        public String getLabel() {
            return label;
        }

        @Override
        public boolean isClosed() {
            return closed;
        }

        @Override
        public void close() {
            closed = true;
        }
    }

    /** A view. */
    record View(Texture texture, int baseMipLevel, int mipLevels, boolean[] closed) implements GpuTextureView {
        @Override
        public boolean isClosed() {
            return closed[0];
        }

        @Override
        public int getWidth(int mipLevel) {
            return texture.getWidth(baseMipLevel + mipLevel);
        }

        @Override
        public int getHeight(int mipLevel) {
            return texture.getHeight(baseMipLevel + mipLevel);
        }

        @Override
        public void close() {
            closed[0] = true;
        }
    }

    /** A recorded encoder command. */
    record Command(String name, List<Object> args) {
    }

    final List<Texture> textures = new ArrayList<>();
    final List<View> views = new ArrayList<>();
    final List<Command> commands = new ArrayList<>();

    GpuDevice device() {
        return (GpuDevice) Proxy.newProxyInstance(GpuDevice.class.getClassLoader(), new Class<?>[] {GpuDevice.class}, (proxy, method, args) -> {
            switch (method.getName()) {
                case "createTexture" -> {
                    String label = args[0] instanceof String s ? s : "?";
                    Texture t = new Texture(label, (int) args[1], (GpuFormat) args[2], (int) args[3], (int) args[4], (int) args[6]);
                    textures.add(t);
                    return t;
                }
                case "createTextureView" -> {
                    Texture t = (Texture) args[0];
                    View v = args.length == 1 ? new View(t, 0, t.mips, new boolean[1]) : new View(t, (int) args[1], (int) args[2], new boolean[1]);
                    views.add(v);
                    return v;
                }
                default -> throw new UnsupportedOperationException(method.getName());
            }
        });
    }

    CommandEncoder encoder() {
        return (CommandEncoder) Proxy.newProxyInstance(CommandEncoder.class.getClassLoader(), new Class<?>[] {CommandEncoder.class},
            (proxy, method, args) -> {
                commands.add(new Command(method.getName(), Arrays.asList(args)));
                return null;
            });
    }

    List<Command> commands(String name) {
        return commands.stream().filter(c -> c.name().equals(name)).toList();
    }
}
