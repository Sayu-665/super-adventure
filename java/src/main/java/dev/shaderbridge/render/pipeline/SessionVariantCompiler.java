package dev.shaderbridge.render.pipeline;

import dev.shaderbridge.model.BlobInfo;
import dev.shaderbridge.model.Blobs;
import dev.shaderbridge.model.GeometryProgram;
import dev.shaderbridge.model.ModelJson;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.json.ModelParseException;
import dev.shaderbridge.natives.ShaderBridgeNative;
import dev.shaderbridge.pack.PackException;
import dev.shaderbridge.pack.PackSession;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.util.List;

/**
 * Compiles program variants with the native {@code compileVariant} of the session that compiled
 * the pack. The variant's blobs live in a per-session buffer replaced by the next variant, so the
 * three native calls run under the session's monitor, which every {@link PackSession} method also
 * holds: native calls stay serialized per session.
 */
public final class SessionVariantCompiler implements OnDemandVariants.VariantCompiler {
    private final PackSession session;

    /** JSON of {@code compileVariant}. */
    private record VariantJson(Program program, List<BlobInfo> blobs) {
    }

    /** @param session the session that compiled the active pack */
    public SessionVariantCompiler(PackSession session) {
        this.session = session;
    }

    @Override
    public ProgramVariant compile(String folder, GeometryProgram slot, String profile) throws PackException {
        synchronized (session) {
            long handle = session.handle();
            String json = ShaderBridgeNative.compileVariant(handle, folder, slot.wireName(), profile);
            if (json == null) {
                throw new PackException(lastError());
            }
            VariantJson variant;
            try {
                variant = ModelJson.parse(json, VariantJson.class);
            } catch (ModelParseException e) {
                throw new PackException("the native library returned an invalid variant: " + e.getMessage(), e);
            }
            long size = ShaderBridgeNative.variantBlobSize(handle);
            if (size < 0 || size > Integer.MAX_VALUE) {
                throw new PackException("invalid variant blob size " + size + ": " + lastError());
            }
            ByteBuffer buffer = ByteBuffer.allocateDirect((int) size).order(ByteOrder.LITTLE_ENDIAN);
            if (size > 0 && !ShaderBridgeNative.variantBlobData(handle, buffer)) {
                throw new PackException("cannot fetch the variant's shaders: " + lastError());
            }
            try {
                return new ProgramVariant(folder, variant.program(), Blobs.of(buffer, variant.blobs()));
            } catch (IllegalArgumentException e) {
                throw new PackException("inconsistent variant shaders: " + e.getMessage(), e);
            }
        }
    }

    private static String lastError() {
        String error = ShaderBridgeNative.lastError();
        return error == null ? "unknown error" : error;
    }
}
