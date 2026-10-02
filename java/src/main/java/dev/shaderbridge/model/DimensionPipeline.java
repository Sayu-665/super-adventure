package dev.shaderbridge.model;

import java.util.List;
import java.util.Map;
import java.util.Optional;

/**
 * The complete pipeline of one world folder.
 *
 * @param folder              world folder ({@code world0}, {@code world-1}, ...) or {@code ""} for the pack root
 * @param dimensionIds        dimension ids this pipeline applies to ({@code *} = wildcard)
 * @param targets             render target configuration
 * @param settings            functional {@code shaders.properties} keys and global consts
 * @param uniforms            layouts of {@code sb_Frame} and {@code sb_Draw}
 * @param customUniforms      custom uniforms and variables, evaluated natively
 * @param bindings            pack-global binding table
 * @param programs            all programs; other fields refer to them by index
 * @param geometry            geometry program to program index, after fallback resolution
 * @param passes              frame steps in execution order
 * @param gbufferAttachments  colortex indices of the shared gbuffers render pass (empty if it does not fit)
 * @param shadowAttachments   shadowcolor indices of the shared shadow render pass
 * @param endOfFrameCopies    colortex buffers copied alt to main at the end of the frame
 * @param distantHorizons     Distant Horizons strategy
 */
public record DimensionPipeline(
    String folder,
    List<String> dimensionIds,
    RenderTargets targets,
    PackSettings settings,
    UniformLayout uniforms,
    List<CustomUniform> customUniforms,
    BindingTable bindings,
    List<Program> programs,
    Map<GeometryProgram, GeometrySlot> geometry,
    List<Pass> passes,
    List<Integer> gbufferAttachments,
    List<Integer> shadowAttachments,
    List<Integer> endOfFrameCopies,
    DhPipeline distantHorizons
) {
    public DimensionPipeline {
        Copies.required(folder, "folder");
        dimensionIds = Copies.list(dimensionIds);
        Copies.required(targets, "targets");
        Copies.required(settings, "settings");
        Copies.required(uniforms, "uniforms");
        customUniforms = Copies.list(customUniforms);
        Copies.required(bindings, "bindings");
        programs = Copies.list(programs);
        geometry = Copies.map(geometry);
        passes = Copies.list(passes);
        gbufferAttachments = Copies.list(gbufferAttachments);
        shadowAttachments = Copies.list(shadowAttachments);
        endOfFrameCopies = Copies.list(endOfFrameCopies);
        Copies.required(distantHorizons, "distant_horizons");
    }

    /**
     * @param program a geometry program
     * @return the program that renders it after fallback resolution, if any
     */
    public Optional<Program> programFor(GeometryProgram program) {
        GeometrySlot slot = geometry.get(program);
        return slot == null ? Optional.empty() : Optional.of(programs.get(slot.program()));
    }
}
