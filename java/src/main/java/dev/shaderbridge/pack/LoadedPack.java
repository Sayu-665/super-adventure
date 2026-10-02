package dev.shaderbridge.pack;

import dev.shaderbridge.model.Blobs;
import dev.shaderbridge.model.CompiledPack;
import dev.shaderbridge.model.Diagnostic;
import java.util.List;

/**
 * A compiled pack ready for rendering: the model, its binary payloads, its diagnostics and the
 * native session that produced it (still needed for custom-uniform evaluators and variants).
 * Closing it closes the session; the render integration must stop using it first.
 */
public final class LoadedPack implements AutoCloseable {
    private final String name;
    private final CompiledPack model;
    private final Blobs blobs;
    private final DiagnosticSummary summary;
    private final PackSession session;
    private volatile boolean closed;

    /**
     * @param name    pack file name
     * @param model   the compiled model
     * @param blobs   its binary payloads
     * @param session the session that compiled it; ownership passes to this object
     */
    public LoadedPack(String name, CompiledPack model, Blobs blobs, PackSession session) {
        this.name = name;
        this.model = model;
        this.blobs = blobs;
        this.session = session;
        this.summary = DiagnosticSummary.of(model.diagnostics());
    }

    /** @return the pack file name */
    public String name() {
        return name;
    }

    /** @return the compiled model */
    public CompiledPack model() {
        return model;
    }

    /** @return the SPIR-V and GLSL payloads */
    public Blobs blobs() {
        return blobs;
    }

    /** @return every diagnostic of the compile */
    public List<Diagnostic> diagnostics() {
        return model.diagnostics();
    }

    /** @return diagnostic counts */
    public DiagnosticSummary summary() {
        return summary;
    }

    /** @return the native session (for uniform evaluators); valid until {@link #close()} */
    public PackSession session() {
        return session;
    }

    /** @return true once {@link #close()} was called */
    public boolean isClosed() {
        return closed;
    }

    @Override
    public void close() {
        closed = true;
        session.close();
    }

    @Override
    public String toString() {
        return "LoadedPack[" + name + ", " + model.dimensions().size() + " dimensions, " + summary.describe() + "]";
    }
}
