package dev.shaderbridge.model;

import java.util.ArrayList;
import java.util.List;
import java.util.Map;

/**
 * Cross-reference checks of a parsed {@link CompiledPack}: every program index, blob id and blob
 * kind the model refers to must exist, so that the render integration can index into the model
 * without guarding every access. JSON parsing only checks the shape; this checks the references.
 */
public final class ModelValidation {
    /** At most this many problems are reported; one broken table usually yields many. */
    private static final int MAX_PROBLEMS = 20;

    private ModelValidation() {
    }

    /**
     * @param pack a parsed model whose {@code blobs} index is filled
     * @return human-readable problems, empty if the model is consistent
     */
    public static List<String> problems(CompiledPack pack) {
        List<String> out = new ArrayList<>();
        for (DimensionPipeline dimension : pack.dimensions()) {
            String where = "dimension '" + dimension.folder() + "'";
            int programs = dimension.programs().size();
            for (Map.Entry<GeometryProgram, GeometrySlot> slot : dimension.geometry().entrySet()) {
                String slotName = where + " geometry " + slot.getKey().wireName();
                index(out, slotName, slot.getValue().program(), programs);
                for (Map.Entry<String, Integer> variant : slot.getValue().variants().entrySet()) {
                    index(out, slotName + " variant " + variant.getKey(), variant.getValue(), programs);
                }
            }
            for (Pass pass : dimension.passes()) {
                String passName = where + " pass " + pass.group().wireName() + pass.index();
                if (pass.program() != null) {
                    index(out, passName + " program", pass.program(), programs);
                }
                for (int compute : pass.computes()) {
                    index(out, passName + " compute", compute, programs);
                }
            }
            for (ColorTarget target : dimension.targets().colortex()) {
                for (int program : target.mipmapPrograms()) {
                    index(out, where + " colortex" + target.index() + " mipmap program", program, programs);
                }
            }
            for (Program program : dimension.programs()) {
                for (StageModule stage : program.stages()) {
                    String stageName = "program '" + program.name() + "' " + stage.stage().wireName();
                    blob(out, pack, stageName + " spirv", stage.spirv(), BlobKind.SPIRV);
                    blob(out, pack, stageName + " glsl_vulkan", stage.glslVulkan(), BlobKind.GLSL);
                    blob(out, pack, stageName + " glsl_renderpearl", stage.glslRenderpearl(), BlobKind.GLSL);
                }
            }
        }
        return out.size() > MAX_PROBLEMS ? List.copyOf(out.subList(0, MAX_PROBLEMS)) : List.copyOf(out);
    }

    private static void index(List<String> out, String what, int index, int size) {
        if (index < 0 || index >= size) {
            out.add(what + " refers to program " + index + " of " + size);
        }
    }

    private static void blob(List<String> out, CompiledPack pack, String what, BlobId id, BlobKind kind) {
        if (id == null) {
            return;
        }
        if (id.index() >= pack.blobs().size()) {
            out.add(what + " refers to blob " + id.index() + " of " + pack.blobs().size());
        } else if (pack.blobs().get(id.index()).kind() != kind) {
            out.add(what + " refers to a " + pack.blobs().get(id.index()).kind().wireName() + " blob");
        }
    }
}
