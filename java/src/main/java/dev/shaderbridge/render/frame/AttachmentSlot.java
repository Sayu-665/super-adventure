package dev.shaderbridge.render.frame;

/**
 * What one color attachment slot of a ShaderBridge render pass holds. Mojang's render passes take
 * every attachment at creation, at one size. Every slot gets a texture: a slot without one would
 * need a pipeline format of {@code VK_FORMAT_UNDEFINED} there (Vulkan's
 * {@code dynamicRenderingUnusedAttachments} is not enabled), but the pipelines of a pass share
 * one attachment layout with a real format in every slot.
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

    /** A throwaway texture of the pass size standing in for a missing target: the slot's writes are discarded. */
    record Sink() implements AttachmentSlot {
    }
}
