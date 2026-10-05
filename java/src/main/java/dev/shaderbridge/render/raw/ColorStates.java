package dev.shaderbridge.render.raw;

import com.mojang.renderpearl.api.GpuFormat;
import dev.shaderbridge.model.BlendFactor;
import dev.shaderbridge.model.BlendMode;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.render.pipeline.SpirvReflection.ScalarClass;
import dev.shaderbridge.render.pipeline.TextureFormats;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import org.lwjgl.vulkan.VK10;

/**
 * The color attachment states of a raw fullscreen pipeline, with the headless executor's rules
 * (as {@code AttachmentPlanner} applies them to renderpearl pipelines): output location
 * {@code i} writes slot {@code i} when the slot has an attachment and the fragment shader
 * declares the location with the attachment's numeric class; blending uses the per-buffer
 * override of the slot's target, else the program's blend, and is off for integer formats.
 */
public final class ColorStates {
    private ColorStates() {
    }

    /**
     * One color attachment slot.
     *
     * @param format the attachment's format, empty for a slot without attachment
     * @param write  the program writes the slot
     * @param blend  the blend mode, empty for none
     */
    public record Slot(Optional<GpuFormat> format, boolean write, Optional<BlendMode> blend) {
    }

    /**
     * @param program         a composite-style program
     * @param formats         the format of each slot's attachment, empty where there is none
     * @param fragmentOutputs fragment output locations and their numeric class (SPIR-V reflection)
     * @return the state of each slot
     */
    public static List<Slot> of(Program program, List<Optional<GpuFormat>> formats, Map<Integer, ScalarClass> fragmentOutputs) {
        List<Slot> slots = new ArrayList<>();
        for (int i = 0; i < formats.size(); i++) {
            Optional<GpuFormat> format = formats.get(i);
            boolean write = format.isPresent() && fragmentOutputs.get(i) == TextureFormats.numericClass(format.get());
            int target = i < program.drawBuffers().size() ? program.drawBuffers().get(i) : -1;
            Optional<BlendMode> blend = write && TextureFormats.blendable(format.get()) ? blendOf(program, target) : Optional.empty();
            slots.add(new Slot(format, write, blend));
        }
        return slots;
    }

    /**
     * Without {@code independentBlend} every attachment of a Vulkan pipeline must have the same
     * state; slots without attachment take that common state.
     *
     * @param slots            the slots
     * @param independentBlend the device feature is enabled
     * @return why the slots cannot be one pipeline on this device, empty if they can
     */
    public static Optional<String> problem(List<Slot> slots, boolean independentBlend) {
        if (independentBlend) {
            return Optional.empty();
        }
        Set<String> states = new HashSet<>();
        slots.stream().filter(s -> s.format().isPresent()).forEach(s -> states.add(s.write() + "/" + s.blend()));
        return states.size() > 1
            ? Optional.of("its attachments need different write masks or blend states, which requires the independentBlend device feature")
            : Optional.empty();
    }

    /**
     * @param factor a pack blend factor
     * @return its {@code VkBlendFactor}
     */
    public static int vkFactor(BlendFactor factor) {
        return switch (factor) {
            case ZERO -> VK10.VK_BLEND_FACTOR_ZERO;
            case ONE -> VK10.VK_BLEND_FACTOR_ONE;
            case SRC_COLOR -> VK10.VK_BLEND_FACTOR_SRC_COLOR;
            case ONE_MINUS_SRC_COLOR -> VK10.VK_BLEND_FACTOR_ONE_MINUS_SRC_COLOR;
            case DST_COLOR -> VK10.VK_BLEND_FACTOR_DST_COLOR;
            case ONE_MINUS_DST_COLOR -> VK10.VK_BLEND_FACTOR_ONE_MINUS_DST_COLOR;
            case SRC_ALPHA -> VK10.VK_BLEND_FACTOR_SRC_ALPHA;
            case ONE_MINUS_SRC_ALPHA -> VK10.VK_BLEND_FACTOR_ONE_MINUS_SRC_ALPHA;
            case DST_ALPHA -> VK10.VK_BLEND_FACTOR_DST_ALPHA;
            case ONE_MINUS_DST_ALPHA -> VK10.VK_BLEND_FACTOR_ONE_MINUS_DST_ALPHA;
            case SRC_ALPHA_SATURATE -> VK10.VK_BLEND_FACTOR_SRC_ALPHA_SATURATE;
        };
    }

    /** The per-buffer override (a null value turns blending off), else the program's blend. */
    private static Optional<BlendMode> blendOf(Program program, int target) {
        if (program.blendPerBuffer().containsKey(target)) {
            return Optional.ofNullable(program.blendPerBuffer().get(target));
        }
        return Optional.ofNullable(program.blend());
    }
}
