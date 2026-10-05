package dev.shaderbridge.render.raw;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.shaderbridge.render.RenderFixture;
import dev.shaderbridge.render.pipeline.SpirvReflector;
import java.nio.ByteBuffer;
import java.util.Map;
import org.junit.jupiter.api.Test;

class SpirvBlockSizesTest {
    private static final RenderFixture GLIMMER = RenderFixture.load(RenderFixture.GLIMMER);

    /** {@code buffer Data { vec3 a; float b; mat3 m; float arr[4]; vec4 tail[]; }} as glslang lays it out (std430). */
    private static ByteBuffer module(int storageClass, boolean bufferBlock) {
        SpirvAssembler m = new SpirvAssembler();
        int f32 = m.id();
        int u32 = m.id();
        int vec3 = m.id();
        int vec4 = m.id();
        int mat3 = m.id();
        int four = m.id();
        int array = m.id();
        int runtime = m.id();
        int block = m.id();
        m.op(SpirvAssembler.OP_TYPE_FLOAT, f32, 32);
        m.op(SpirvAssembler.OP_TYPE_INT, u32, 32, 0);
        m.op(SpirvAssembler.OP_TYPE_VECTOR, vec3, f32, 3);
        m.op(SpirvAssembler.OP_TYPE_VECTOR, vec4, f32, 4);
        m.op(SpirvAssembler.OP_TYPE_MATRIX, mat3, vec3, 3);
        m.op(SpirvAssembler.OP_CONSTANT, u32, four, 4);
        m.op(SpirvAssembler.OP_TYPE_ARRAY, array, f32, four);
        m.op(SpirvAssembler.OP_TYPE_RUNTIME_ARRAY, runtime, vec4);
        m.op(SpirvAssembler.OP_TYPE_STRUCT, block, vec3, f32, mat3, array, runtime);
        m.op(SpirvAssembler.OP_DECORATE, array, 6, 4);
        m.op(SpirvAssembler.OP_DECORATE, runtime, 6, 16);
        m.op(SpirvAssembler.OP_DECORATE, block, bufferBlock ? 3 : 2);
        m.op(SpirvAssembler.OP_MEMBER_DECORATE, block, 0, 35, 0);
        m.op(SpirvAssembler.OP_MEMBER_DECORATE, block, 1, 35, 12);
        m.op(SpirvAssembler.OP_MEMBER_DECORATE, block, 2, 35, 16);
        m.op(SpirvAssembler.OP_MEMBER_DECORATE, block, 2, 5);
        m.op(SpirvAssembler.OP_MEMBER_DECORATE, block, 2, 7, 16);
        m.op(SpirvAssembler.OP_MEMBER_DECORATE, block, 3, 35, 64);
        m.op(SpirvAssembler.OP_MEMBER_DECORATE, block, 4, 35, 80);
        int pointer = m.pointer(storageClass, block);
        int variable = m.variable(pointer, storageClass);
        m.binding(variable, 2, 7);
        return m.build();
    }

    @Test
    void blockSizeEndsAtTheLastStaticMember() {
        // vec3 @0, float @12, mat3 @16 (3 columns of stride 16 = 48), float[4] @64 (stride 4 = 16),
        // vec4[] @80 counts as empty: the block ends at 80.
        assertEquals(Map.of(new SpirvBlockSizes.Location(2, 7), 80L), SpirvBlockSizes.storageBlocks(module(SpirvAssembler.STORAGE_BUFFER, false)));
        assertEquals(Map.of(new SpirvBlockSizes.Location(2, 7), 80L), SpirvBlockSizes.storageBlocks(module(SpirvAssembler.UNIFORM, true)),
            "a BufferBlock in the Uniform storage class is a storage buffer too");
    }

    @Test
    void uniformBlocksAreNotStorageBuffers() {
        assertTrue(SpirvBlockSizes.storageBlocks(module(SpirvAssembler.UNIFORM, false)).isEmpty());
    }

    @Test
    void malformedModulesAreRejected() {
        assertThrows(SpirvReflector.InvalidSpirvException.class, () -> SpirvBlockSizes.storageBlocks(ByteBuffer.allocate(8)));
    }

    @Test
    void glimmersStorageBuffersAreAsLargeAsDeclared() {
        // environmentData { vec3 sunlightColor; vec3 skylightColor @16; float @28; uint @32 } = 36 bytes
        // (bufferObject.0 = 40); smoothedData { float } = 4 bytes (bufferObject.1 = 4).
        Map<Integer, Long> declared = StorageBlocks.declared(GLIMMER.dim(), GLIMMER.blobs());
        assertEquals(36L, declared.get(0));
        assertEquals(4L, declared.get(1));
        GLIMMER.dim().targets().buffers().forEach(b -> {
            if (declared.containsKey(b.index())) {
                assertTrue(declared.get(b.index()) <= b.size(), "glimmer declares no block larger than its buffer " + b.index());
            }
        });
    }
}
