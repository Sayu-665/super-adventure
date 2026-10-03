package dev.shaderbridge.render.pipeline;

import java.util.Locale;
import net.minecraft.resources.Identifier;

/**
 * Identity of a pack pipeline: a program compiled for a draw profile, drawn with a shape (the
 * replaced vanilla pipeline, or a ShaderBridge pass kind) into an attachment layout.
 *
 * @param folder  world folder of the dimension pipeline ({@code ""} for the pack root)
 * @param program program name ({@code world0/gbuffers_terrain})
 * @param profile draw profile the program was translated for
 * @param shape   {@link PipelineShape#id()}
 * @param layout  {@link AttachmentLayout#id()}
 */
public record PipelineKey(String folder, String program, String profile, String shape, String layout) {
    /** Namespace of every ShaderBridge pipeline location and shader id. */
    public static final String NAMESPACE = "shaderbridge";

    /**
     * @param packHash the pack's source hash (any length; 16 characters are used)
     * @return the pipeline location {@code shaderbridge:<hash>/<folder>/<program>/<profile>/<shape>/<layout>},
     *     with every character outside {@code [a-z0-9/._-]} replaced by {@code _}
     */
    public Identifier location(String packHash) {
        String hash = sanitize(packHash.length() > 16 ? packHash.substring(0, 16) : packHash);
        String path = String.join("/", hash, folder.isEmpty() ? "root" : sanitize(folder), sanitize(program), sanitize(profile), sanitize(shape),
            sanitize(layout));
        return Identifier.fromNamespaceAndPath(NAMESPACE, path);
    }

    /** Lower-cases and replaces characters identifiers do not allow; empty segments become {@code _}. */
    static String sanitize(String s) {
        StringBuilder out = new StringBuilder(s.length());
        for (char c : s.toLowerCase(Locale.ROOT).toCharArray()) {
            boolean ok = (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '/' || c == '.' || c == '_' || c == '-';
            out.append(ok ? c : '_');
        }
        String path = out.toString().replaceAll("/{2,}", "/").replaceAll("^/|/$", "");
        return path.isEmpty() ? "_" : path;
    }
}
