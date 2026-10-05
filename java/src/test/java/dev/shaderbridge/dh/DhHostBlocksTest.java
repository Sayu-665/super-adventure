package dev.shaderbridge.dh;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;

import dev.shaderbridge.render.pipeline.DrawProfiles;
import dev.shaderbridge.uniforms.Std140Writer;
import java.io.IOException;
import java.io.InputStream;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import org.joml.Matrix4f;
import org.junit.jupiter.api.Test;

/**
 * {@link DhHostBlocks}: the host blocks ShaderBridge fills match the {@code dh_terrain} profile's
 * declarations (the packaged {@code crates/sb-transform/profiles/dh_terrain.toml}) and std140,
 * and {@code uModelOffset} keeps its precision far from the world origin.
 */
class DhHostBlocksTest {
    private static final Pattern BLOCK = Pattern.compile("\\[\\[blocks]]\\s*name = \"(\\w+)\"\\s*instance = \"\\w+\"\\s*members = \"([^\"]*)\"");

    /** The profile's blocks: name to {@code "type name"} member declarations in order. */
    private static Map<String, List<String>> profileBlocks() throws IOException {
        String toml;
        try (InputStream in = DhHostBlocksTest.class.getResourceAsStream(DrawProfiles.RESOURCE_DIR + "dh_terrain.toml")) {
            assertNotNull(in, "the build packages the dh_terrain profile");
            toml = new String(in.readAllBytes(), StandardCharsets.UTF_8);
        }
        Map<String, List<String>> blocks = new LinkedHashMap<>();
        Matcher m = BLOCK.matcher(toml);
        while (m.find()) {
            List<String> members = new ArrayList<>();
            for (String declaration : m.group(2).split(";")) {
                if (!declaration.isBlank()) {
                    members.add(declaration.trim().replaceAll("\\s+", " "));
                }
            }
            blocks.put(m.group(1), members);
        }
        return blocks;
    }

    private static List<String> declarations(List<DhHostBlocks.Member> members) {
        return members.stream().map(m -> m.type().glslName() + " " + m.name()).toList();
    }

    @Test
    void membersMatchTheDhTerrainProfile() throws IOException {
        Map<String, List<String>> blocks = profileBlocks();
        assertEquals(List.of(DhHostBlocks.UNIQUE_BLOCK, DhHostBlocks.SHARED_BLOCK), List.copyOf(blocks.keySet()),
            "ShaderBridge fills every host block of the profile");
        assertEquals(blocks.get(DhHostBlocks.UNIQUE_BLOCK), declarations(DhHostBlocks.UNIQUE));
        assertEquals(blocks.get(DhHostBlocks.SHARED_BLOCK), declarations(DhHostBlocks.SHARED));
    }

    @Test
    void offsetsFollowStd140() {
        assertEquals(List.of(0), DhHostBlocks.UNIQUE.stream().map(DhHostBlocks.Member::offset).toList());
        assertEquals(16, DhHostBlocks.UNIQUE_SIZE);
        // bool and six floats packed, vec3 aligned to 16, mat4 columns of 16.
        assertEquals(List.of(0, 4, 8, 12, 16, 20, 24, 32, 48), DhHostBlocks.SHARED.stream().map(DhHostBlocks.Member::offset).toList());
        assertEquals(112, DhHostBlocks.SHARED_SIZE);
    }

    @Test
    void sharedBlockBytes() {
        ByteBuffer bytes = ByteBuffer.allocate(256).order(ByteOrder.LITTLE_ENDIAN);
        for (int i = 0; i < bytes.capacity(); i++) {
            bytes.put(i, (byte) 0x7f);
        }
        Matrix4f combined = new Matrix4f().perspective(1.2f, 1.5f, 0.05f, 1000f).translate(1, 2, 3);
        int base = 64;
        DhHostBlocks.writeShared(new Std140Writer(bytes), base, new DhHostBlocks.Shared(-64, 63710, 5, 1920, 1080, combined));
        assertEquals(0, bytes.getInt(base), "uIsWhiteWorld");
        assertEquals(-64f, bytes.getFloat(base + 4), "uWorldYOffset");
        assertEquals(0.01f, bytes.getFloat(base + 8), "uMircoOffset");
        assertEquals(63710f, bytes.getFloat(base + 12), "uEarthRadius");
        assertEquals(5f, bytes.getFloat(base + 16), "uFrameMod8");
        assertEquals(1920f, bytes.getFloat(base + 20), "uViewWidth");
        assertEquals(1080f, bytes.getFloat(base + 24), "uViewHeight");
        for (int c = 0; c < 3; c++) {
            assertEquals(0f, bytes.getFloat(base + 32 + c * 4), "uCameraPos is zero (camera-relative convention)");
        }
        for (int c = 0; c < 4; c++) {
            for (int r = 0; r < 4; r++) {
                assertEquals(combined.get(c, r), bytes.getFloat(base + 48 + c * 16 + r * 4), "uCombinedMatrix column " + c + " row " + r);
            }
        }
        assertEquals((byte) 0x7f, bytes.get(base - 1), "nothing written before the block");
        assertEquals((byte) 0x7f, bytes.get(base + DhHostBlocks.SHARED_SIZE), "nothing written after the block");
    }

    @Test
    void modelOffsetIsSubtractedInDoublePrecision() {
        // 30 million blocks out a float has a spacing of 2 blocks: subtracting floats loses the offset.
        int minX = 29_999_872;
        double cameraX = 29_999_871.75;
        assertEquals(0f, (float) minX - (float) cameraX, "the naive float result");
        float[] offset = DhHostBlocks.modelOffset(minX, -64, -29_999_616, cameraX, 70.5, -29_999_700.125);
        assertArrayEquals(new float[] {0.25f, -134.5f, 84.125f}, offset);
    }

    @Test
    void uniqueBlockBytes() {
        ByteBuffer bytes = ByteBuffer.allocate(32).order(ByteOrder.LITTLE_ENDIAN);
        DhHostBlocks.writeUnique(new Std140Writer(bytes), 16, new float[] {0.25f, -134.5f, 84.125f});
        assertEquals(0.25f, bytes.getFloat(16));
        assertEquals(-134.5f, bytes.getFloat(20));
        assertEquals(84.125f, bytes.getFloat(24));
        assertEquals(0, bytes.getInt(28), "padding untouched");
    }

    @Test
    void earthRadiusFollowsDistantHorizons() {
        assertEquals(0f, DhHostBlocks.earthRadius(0));
        assertEquals(0f, DhHostBlocks.earthRadius(1));
        assertEquals(0f, DhHostBlocks.earthRadius(-1));
        assertEquals(63_710f, DhHostBlocks.earthRadius(100));
        assertEquals(-127_420f, DhHostBlocks.earthRadius(-50));
    }
}
