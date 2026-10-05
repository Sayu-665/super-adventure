package dev.shaderbridge.render.raw;

import dev.shaderbridge.model.BindingEntry;
import dev.shaderbridge.model.Blobs;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.ResourceRef;
import dev.shaderbridge.model.StageModule;
import java.util.HashMap;
import java.util.Map;

/**
 * The largest block each shader storage buffer ({@code bufferObject.<n>}) is declared with by any
 * program of a dimension pipeline: storage buffers are created at least that large, since Vulkan
 * needs a bound range to cover the block (GL tolerates a smaller buffer).
 */
public final class StorageBlocks {
    private StorageBlocks() {
    }

    /**
     * @param dim   a dimension pipeline
     * @param blobs the SPIR-V of its programs
     * @return per storage buffer index, the largest declared block in bytes (absent if no program declares it)
     */
    public static Map<Integer, Long> declared(DimensionPipeline dim, Blobs blobs) {
        Map<SpirvBlockSizes.Location, Integer> buffers = new HashMap<>();
        for (BindingEntry e : dim.bindings().entries()) {
            if (e.resource() instanceof ResourceRef.Ssbo ssbo) {
                buffers.put(new SpirvBlockSizes.Location(e.set(), e.binding()), ssbo.index());
            }
        }
        Map<Integer, Long> declared = new HashMap<>();
        if (buffers.isEmpty()) {
            return declared;
        }
        for (Program program : dim.programs()) {
            for (StageModule module : program.stages()) {
                if (module.spirv() == null) {
                    continue;
                }
                Map<SpirvBlockSizes.Location, Long> blocks;
                try {
                    blocks = SpirvBlockSizes.storageBlocks(blobs.spirv(module.spirv()));
                } catch (IllegalArgumentException unreadable) {
                    continue; // the raw path rejects the program when it is offered
                }
                blocks.forEach((location, size) -> {
                    Integer index = buffers.get(location);
                    if (index != null) {
                        declared.merge(index, size, Math::max);
                    }
                });
            }
        }
        return declared;
    }
}
