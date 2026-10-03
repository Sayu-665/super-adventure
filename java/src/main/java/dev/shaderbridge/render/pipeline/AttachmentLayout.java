package dev.shaderbridge.render.pipeline;

import com.mojang.renderpearl.api.GpuFormat;
import dev.shaderbridge.model.ColorTarget;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.PassGroup;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.ProgramKind;
import dev.shaderbridge.model.TextureFormat;
import java.util.List;

/**
 * The color attachments of the render pass a pipeline draws in. Mojang's render passes fix their
 * attachments at creation and pipelines must list a color target state per attachment with the
 * same format, so the layout is part of a pipeline's identity.
 *
 * @param id          short name of the layout kind, part of the pipeline location
 * @param attachments the attachments, in slot order
 * @param shared      the attachments are the dimension's shared gbuffers or shadow list: logical
 *                    output {@code i} goes to slot {@code program.outputSlots()[i]}; otherwise
 *                    output {@code i} goes to slot {@code i}
 */
public record AttachmentLayout(String id, List<Attachment> attachments, boolean shared) {
    /** Format of colortex/shadowcolor targets a pack does not configure (Iris' default). */
    static final TextureFormat DEFAULT_FORMAT = TextureFormat.RGBA8;

    public AttachmentLayout {
        attachments = List.copyOf(attachments);
    }

    /**
     * One attachment.
     *
     * @param target colortex (or shadowcolor) index, -1 for a target outside the pack
     * @param format texture format of the attachment
     */
    public record Attachment(int target, GpuFormat format) {
    }

    /**
     * The pass world geometry is drawn in: the shared {@code gbuffer_attachments} (or
     * {@code shadow_attachments}) when the pack has them, else the program's own draw buffers.
     *
     * @param dim     the dimension pipeline
     * @param program a geometry program
     * @param shadow  the shadow pass
     * @return the layout
     */
    public static AttachmentLayout geometry(DimensionPipeline dim, Program program, boolean shadow) {
        List<Integer> sharedList = shadow ? dim.shadowAttachments() : dim.gbufferAttachments();
        List<ColorTarget> targets = shadow ? dim.targets().shadowcolor() : dim.targets().colortex();
        String kind = shadow ? "shadow" : "gbuffers";
        if (!sharedList.isEmpty()) {
            return new AttachmentLayout(kind, attachments(sharedList, targets), true);
        }
        return new AttachmentLayout(kind + "_own", attachments(program.drawBuffers(), targets), false);
    }

    /**
     * The pass of a fullscreen program: its draw buffers (shadowcolor targets for
     * {@code shadowcomp}).
     *
     * @param dim     the dimension pipeline
     * @param program a composite-style program
     * @return the layout
     */
    public static AttachmentLayout fullscreen(DimensionPipeline dim, Program program) {
        boolean shadow = program.kind() instanceof ProgramKind.Composite c && c.group() == PassGroup.SHADOW_COMP;
        return new AttachmentLayout("fullscreen", attachments(program.drawBuffers(), shadow ? dim.targets().shadowcolor() : dim.targets().colortex()),
            false);
    }

    /**
     * A single attachment, e.g. Minecraft's main color target for the {@code final} pass.
     *
     * @param id     layout name
     * @param target the pack target the attachment stands for
     * @param format its format
     * @return the layout
     */
    public static AttachmentLayout single(String id, int target, GpuFormat format) {
        return new AttachmentLayout(id, List.of(new Attachment(target, format)), false);
    }

    /**
     * @param target a target index
     * @return its slot, or -1 if the layout does not contain it
     */
    public int slotOf(int target) {
        for (int i = 0; i < attachments.size(); i++) {
            if (attachments.get(i).target() == target) {
                return i;
            }
        }
        return -1;
    }

    private static List<Attachment> attachments(List<Integer> indices, List<ColorTarget> targets) {
        return indices.stream().map(i -> new Attachment(i, TextureFormats.renderable(formatOf(targets, i)))).toList();
    }

    private static TextureFormat formatOf(List<ColorTarget> targets, int index) {
        return targets.stream().filter(t -> t.index() == index).map(ColorTarget::format).findFirst().orElse(DEFAULT_FORMAT);
    }
}
