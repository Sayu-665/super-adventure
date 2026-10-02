package dev.shaderbridge.model;

import java.util.List;
import java.util.Optional;

/**
 * A fully compiled shader pack ({@code sb_core::model::CompiledPack}, ARCHITECTURE §7).
 *
 * @param formatVersion version of the JSON format ({@link #FORMAT_VERSION})
 * @param info          pack identity and the environment it was compiled for
 * @param options       options model for the GUI
 * @param idMaps        block, item and entity id maps
 * @param dimensions    one pipeline per world folder ({@code ""} is the pack root)
 * @param diagnostics   pack-wide and per-program diagnostics
 * @param blobs         index of the concatenated blob buffer
 */
public record CompiledPack(
    int formatVersion,
    PackInfo info,
    OptionsModel options,
    IdMaps idMaps,
    List<DimensionPipeline> dimensions,
    List<Diagnostic> diagnostics,
    List<BlobInfo> blobs
) {
    /** The {@code MODEL_FORMAT_VERSION} this mod understands. */
    public static final int FORMAT_VERSION = 1;

    public CompiledPack {
        Copies.required(info, "info");
        Copies.required(options, "options");
        Copies.required(idMaps, "id_maps");
        dimensions = Copies.list(dimensions);
        diagnostics = Copies.list(diagnostics);
        blobs = Copies.list(blobs);
    }

    /**
     * @param folder world folder name, e.g. {@code world0}, or {@code ""} for the pack root
     * @return the pipeline of that folder, if the pack has one
     */
    public Optional<DimensionPipeline> dimension(String folder) {
        return dimensions.stream().filter(d -> d.folder().equals(folder)).findFirst();
    }
}
