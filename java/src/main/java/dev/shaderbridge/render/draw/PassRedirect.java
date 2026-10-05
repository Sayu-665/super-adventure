package dev.shaderbridge.render.draw;

import com.mojang.renderpearl.api.commands.RenderPass;
import com.mojang.renderpearl.api.commands.RenderPassDescriptor;
import java.util.function.Function;

/**
 * Hands ShaderBridge's render passes to code that creates its own, for the duration of one call:
 * while {@linkplain #arm armed}, every render pass created through Mojang's command encoder is
 * created by the armed factory instead (ShaderBridge's {@code FrontendCommandEncoderMixin}). Used
 * to draw Distant Horizons' generic objects, whose renderer opens a pass on Distant Horizons' own
 * textures for each object group, into the pack's gbuffers attachments. The factory's own pass
 * creation is not redirected. Render thread only.
 */
public final class PassRedirect {
    private static Function<RenderPassDescriptor, RenderPass> factory;
    private static RenderPass last;

    private PassRedirect() {
    }

    /** Ends a redirection ({@link #arm}). */
    public interface Armed extends AutoCloseable {
        @Override
        void close();
    }

    /**
     * Redirects render pass creation until the returned handle is closed.
     *
     * @param passes creates the pass to use instead of the requested one (it should register the
     *               pass with {@link ActivePasses})
     * @return closes the redirection and forgets the last redirected pass
     * @throws IllegalStateException if a redirection is already armed
     */
    public static Armed arm(Function<RenderPassDescriptor, RenderPass> passes) {
        if (factory != null) {
            throw new IllegalStateException("a render pass redirection is already armed");
        }
        factory = passes;
        return () -> {
            factory = null;
            ActivePasses.close(last);
            last = null;
        };
    }

    /** @return whether a redirection is armed */
    public static boolean armed() {
        return factory != null;
    }

    /**
     * @param requested the pass the caller asked for
     * @return the pass to return instead, or null when nothing is redirected
     */
    public static RenderPass redirect(RenderPassDescriptor requested) {
        Function<RenderPassDescriptor, RenderPass> passes = factory;
        if (passes == null) {
            return null;
        }
        factory = null;
        try {
            ActivePasses.close(last);
            last = passes.apply(requested);
            return last;
        } finally {
            factory = passes;
        }
    }
}
