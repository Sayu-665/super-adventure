package dev.shaderbridge.pack;

import dev.shaderbridge.model.BlobId;
import dev.shaderbridge.model.Blobs;
import dev.shaderbridge.model.CompiledPack;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.StageModule;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Comparator;
import java.util.stream.Stream;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Writes the translated GLSL of a compiled pack to disk for debugging ({@code debugDumpGlsl}):
 * {@code <dir>/<program>.<ext>.renderpearl.glsl} and {@code .vulkan.glsl}, where the program name
 * keeps its world folder as a subdirectory.
 */
final class GlslDumper {
    private static final Logger LOGGER = LoggerFactory.getLogger("ShaderBridge");

    private GlslDumper() {
    }

    static void dump(CompiledPack pack, Blobs blobs, Path dir) {
        try {
            clear(dir);
            int files = 0;
            for (DimensionPipeline dimension : pack.dimensions()) {
                for (Program program : dimension.programs()) {
                    for (StageModule stage : program.stages()) {
                        String base = safeName(program.name()) + "." + stage.stage().packExtension();
                        files += write(blobs, stage.glslRenderpearl(), dir.resolve(base + ".renderpearl.glsl"));
                        files += write(blobs, stage.glslVulkan(), dir.resolve(base + ".vulkan.glsl"));
                    }
                }
            }
            LOGGER.info("Dumped {} translated shaders to {}", files, dir);
        } catch (IOException | IllegalArgumentException e) {
            LOGGER.warn("Cannot dump translated shaders to {}: {}", dir, e.getMessage());
        }
    }

    private static int write(Blobs blobs, BlobId blob, Path file) throws IOException {
        if (blob == null) {
            return 0;
        }
        Files.createDirectories(file.getParent());
        Files.writeString(file, blobs.glsl(blob), StandardCharsets.UTF_8);
        return 1;
    }

    /** Keeps {@code /} as a directory separator; replaces characters that are unsafe in file names. */
    static String safeName(String programName) {
        StringBuilder out = new StringBuilder(programName.length());
        for (String segment : programName.split("/")) {
            if (segment.isEmpty() || segment.equals(".") || segment.equals("..")) {
                continue;
            }
            if (!out.isEmpty()) {
                out.append('/');
            }
            out.append(segment.replaceAll("[^A-Za-z0-9_.-]", "_"));
        }
        return out.isEmpty() ? "_" : out.toString();
    }

    private static void clear(Path dir) throws IOException {
        if (!Files.isDirectory(dir)) {
            return;
        }
        try (Stream<Path> paths = Files.walk(dir)) {
            for (Path path : paths.sorted(Comparator.reverseOrder()).toList()) {
                if (!path.equals(dir)) {
                    Files.delete(path);
                }
            }
        }
    }
}
