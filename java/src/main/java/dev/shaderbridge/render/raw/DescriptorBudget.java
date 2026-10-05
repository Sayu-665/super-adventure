package dev.shaderbridge.render.raw;

import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import org.lwjgl.vulkan.VK10;

/**
 * The size of the descriptor pools the raw path allocates its sets from. All pools have the same
 * size; a pool that cannot serve an allocation is retired (reset once the GPU is done with the
 * frames that used it) and a fresh one is taken, so a program fits as long as one dispatch's sets
 * fit into an empty pool.
 */
public final class DescriptorBudget {
    /** Sets per pool. */
    public static final int SETS = 64;
    /** Descriptors per pool, per {@code VkDescriptorType} the raw path binds. */
    public static final Map<Integer, Integer> DESCRIPTORS = Map.of(
        VK10.VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER, 128,
        VK10.VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER, 1024,
        VK10.VK_DESCRIPTOR_TYPE_STORAGE_IMAGE, 512,
        VK10.VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, 512
    );

    private DescriptorBudget() {
    }

    /**
     * @param plan a program's descriptor plan
     * @return why one dispatch's sets do not fit into an empty pool, empty if they do
     */
    public static List<String> problems(DescriptorPlan plan) {
        List<String> problems = new ArrayList<>();
        if (plan.sets().size() > SETS) {
            problems.add(plan.sets().size() + " descriptor sets exceed a pool's " + SETS);
        }
        plan.descriptorCounts().forEach((type, count) -> {
            int capacity = DESCRIPTORS.getOrDefault(type, 0);
            if (count > capacity) {
                problems.add(count + " descriptors of type " + type + " exceed a pool's " + capacity);
            }
        });
        return problems;
    }
}
