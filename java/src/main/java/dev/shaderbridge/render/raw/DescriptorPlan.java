package dev.shaderbridge.render.raw;

import dev.shaderbridge.model.BindingEntry;
import dev.shaderbridge.render.pipeline.SpirvReflection;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

/**
 * The descriptor set layouts of a program on the raw path, as its SPIR-V declares them (explicit
 * {@code set}/{@code binding} decorations of the Vulkan target: {@code sb_Frame}/{@code sb_Draw}
 * in set 0, samplers in set 1, storage images and buffers in set 2), each descriptor with what the
 * raw path binds to it.
 *
 * @param sets     the set layouts in ascending set order; sets no stage declares are absent
 * @param problems why the raw path cannot bind the program's descriptors, empty if it can
 */
public record DescriptorPlan(List<SetLayout> sets, List<String> problems) {
    /** Descriptor sets every Vulkan device can bind at once ({@code maxBoundDescriptorSets} minimum). */
    public static final int MAX_SETS = 4;

    public DescriptorPlan {
        sets = List.copyOf(sets);
        problems = List.copyOf(problems);
    }

    /**
     * One descriptor set layout.
     *
     * @param set      the set number
     * @param bindings its bindings in ascending binding order
     */
    public record SetLayout(int set, List<Binding> bindings) {
        public SetLayout {
            bindings = List.copyOf(bindings);
        }
    }

    /**
     * One descriptor binding.
     *
     * @param set        the set number
     * @param binding    the binding number
     * @param name       the descriptor's name (block type name for buffers, variable name otherwise)
     * @param type       its {@code VkDescriptorType}
     * @param count      descriptors in the binding (array size)
     * @param stages     {@code VkShaderStageFlags} of the stages that declare it
     * @param descriptor the reflected descriptor (image dimensionality, texel class)
     * @param source     what is bound to it
     */
    public record Binding(int set, int binding, String name, int type, int count, int stages, SpirvReflection.Descriptor descriptor,
                          Source source) {
    }

    /** What a binding is bound to. */
    public sealed interface Source {
        /** The frame's {@code sb_Frame} block. */
        record Frame() implements Source {
        }

        /** The {@code sb_Draw} block of the dispatch or draw. */
        record Draw() implements Source {
        }

        /**
         * A pack resource of the binding table.
         *
         * @param entry  the binding table entry
         * @param useAlt the program's {@code BindingUse.use_alt} for it
         */
        record Pack(BindingEntry entry, boolean useAlt) implements Source {
        }
    }

    /** @return whether the raw path can bind every descriptor */
    public boolean bindable() {
        return problems.isEmpty();
    }

    /** @return the total descriptor count of each {@code VkDescriptorType} over all sets */
    public Map<Integer, Integer> descriptorCounts() {
        Map<Integer, Integer> counts = new TreeMap<>();
        for (SetLayout set : sets) {
            for (Binding b : set.bindings()) {
                counts.merge(b.type(), b.count(), Integer::sum);
            }
        }
        return counts;
    }
}
