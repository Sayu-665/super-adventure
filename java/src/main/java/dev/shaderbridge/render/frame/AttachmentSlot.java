package dev.shaderbridge.render.frame;

/**
 * What one color attachment slot of a ShaderBridge render pass holds. Mojang's render passes take
 * every attachment at creation, at one size, and need a texture in slot 0 (the pass size comes
 * from it); later slots may be left without a texture, which discards their writes.
 */
public sealed interface AttachmentSlot {
    /**
     * A pack target.
     *
     * @param target colortex (or, in shadow passes, shadowcolor) index
     * @param alt    its alternate texture
     */
    record Target(int target, boolean alt) implements AttachmentSlot {
    }

    /** Minecraft's main color target (the {@code final} pass). */
    record MainColor() implements AttachmentSlot {
    }

    /** A screen-sized throwaway texture standing in for a missing target in slot 0. */
    record Sink() implements AttachmentSlot {
    }

    /** No texture: the slot's writes are discarded. */
    record Unused() implements AttachmentSlot {
    }
}
