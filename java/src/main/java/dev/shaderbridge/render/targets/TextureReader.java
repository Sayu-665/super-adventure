package dev.shaderbridge.render.targets;

import com.mojang.blaze3d.platform.NativeImage;
import java.io.IOException;
import java.io.InputStream;
import java.util.Optional;
import net.minecraft.resources.Identifier;
import net.minecraft.server.packs.resources.Resource;
import net.minecraft.server.packs.resources.ResourceProvider;

/**
 * Reads the files custom textures come from: the pack's own files and resource-pack (or vanilla)
 * textures. Images are PNGs decoded to RGBA by {@link NativeImage#read(byte[])}.
 */
public interface TextureReader {
    /**
     * @param path a file path relative to the pack's {@code shaders/} root
     * @return its bytes, or empty if it does not exist
     * @throws IOException if it cannot be read
     */
    Optional<byte[]> packFile(String path) throws IOException;

    /**
     * @param location a resource location ({@code minecraft:textures/...})
     * @return the decoded image, or empty if no resource pack provides it
     * @throws IOException if it cannot be read or decoded
     */
    Optional<NativeImage> resource(String location) throws IOException;

    /**
     * @param path a PNG relative to the pack's {@code shaders/} root
     * @return the decoded image, or empty if it does not exist
     * @throws IOException if it cannot be read or decoded
     */
    default Optional<NativeImage> packImage(String path) throws IOException {
        Optional<byte[]> bytes = packFile(path);
        return bytes.isEmpty() ? Optional.empty() : Optional.of(NativeImage.read(bytes.get()));
    }

    /**
     * @param files     the pack's files
     * @param resources Minecraft's resources ({@code Minecraft.getInstance().getResourceManager()})
     * @return a reader over both
     */
    static TextureReader of(PackFiles files, ResourceProvider resources) {
        return new TextureReader() {
            @Override
            public Optional<byte[]> packFile(String path) throws IOException {
                return files.read(path);
            }

            @Override
            public Optional<NativeImage> resource(String location) throws IOException {
                Identifier id = Identifier.tryParse(location);
                Optional<Resource> resource = id == null ? Optional.empty() : resources.getResource(id);
                if (resource.isEmpty()) {
                    return Optional.empty();
                }
                try (InputStream in = resource.get().open()) {
                    return Optional.of(NativeImage.read(in));
                }
            }
        };
    }
}
