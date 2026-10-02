package dev.shaderbridge.model;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.util.List;
import org.junit.jupiter.api.Test;

class BlobsTest {
    private static final String GLSL = "#version 450\n\nvoid main() { }\n";
    private static final List<BlobInfo> INDEX = List.of(
        new BlobInfo(BlobKind.SPIRV, 0, 20),
        new BlobInfo(BlobKind.GLSL, 24, GLSL.length()),
        new BlobInfo(BlobKind.BYTES, 56, 3));

    /** Builds the buffer the sample's blob index describes (8-byte aligned, like BlobTable::concat). */
    private static Blobs sampleBlobs() {
        ByteBuffer buffer = ByteBuffer.allocateDirect(59).order(ByteOrder.LITTLE_ENDIAN);
        buffer.putInt(0, 0x07230203).putInt(4, 0x00010500).putInt(8, 1).putInt(12, 2).putInt(16, 3);
        buffer.put(24, GLSL.getBytes(StandardCharsets.UTF_8));
        buffer.put(56, new byte[] {1, 2, 3});
        return Blobs.of(buffer, INDEX);
    }

    @Test
    void sampleIndexMatchesTheHandWrittenJson() throws Exception {
        assertEquals(30, GLSL.length());
        assertEquals(INDEX, ModelJson.parse(ModelJsonTest.resource("compiled_pack_sample.json"), CompiledPack.class).blobs());
    }

    @Test
    void spirvIsALittleEndianSlice() {
        ByteBuffer words = sampleBlobs().spirv(new BlobId(0));
        assertEquals(20, words.remaining());
        assertEquals(ByteOrder.LITTLE_ENDIAN, words.order());
        assertEquals(0x07230203, words.getInt(0));
        assertEquals(3, words.getInt(16));
        assertTrue(words.isReadOnly());
    }

    @Test
    void glslIsDecoded() {
        assertEquals(GLSL, sampleBlobs().glsl(new BlobId(1)));
    }

    @Test
    void rawBytes() {
        ByteBuffer bytes = sampleBlobs().bytes(new BlobId(2));
        assertEquals(3, bytes.remaining());
        assertEquals(3, bytes.get(2));
        assertEquals(59, sampleBlobs().byteSize());
        assertEquals(3, sampleBlobs().count());
    }

    @Test
    void kindAndRangeAreChecked() {
        Blobs blobs = sampleBlobs();
        assertThrows(IllegalArgumentException.class, () -> blobs.glsl(new BlobId(0)));
        assertThrows(IllegalArgumentException.class, () -> blobs.spirv(new BlobId(1)));
        assertThrows(IllegalArgumentException.class, () -> blobs.info(new BlobId(3)));
        ByteBuffer small = ByteBuffer.allocateDirect(10);
        assertThrows(IllegalArgumentException.class, () -> Blobs.of(small, INDEX));
        ByteBuffer odd = ByteBuffer.allocateDirect(8);
        Blobs oddBlobs = Blobs.of(odd, List.of(new BlobInfo(BlobKind.SPIRV, 0, 6)));
        assertThrows(IllegalArgumentException.class, () -> oddBlobs.spirv(new BlobId(0)));
        assertEquals(0, Blobs.empty().count());
    }

    @Test
    void invalidUtf8IsRejected() {
        ByteBuffer buffer = ByteBuffer.allocateDirect(2).put(0, (byte) 0xC3).put(1, (byte) 0x28);
        Blobs blobs = Blobs.of(buffer, List.of(new BlobInfo(BlobKind.GLSL, 0, 2)));
        assertThrows(IllegalArgumentException.class, () -> blobs.glsl(new BlobId(0)));
    }
}
