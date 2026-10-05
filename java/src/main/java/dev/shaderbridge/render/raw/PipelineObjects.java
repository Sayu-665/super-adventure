package dev.shaderbridge.render.raw;

import com.mojang.renderpearl.backend.vulkan.Destroyable;
import dev.shaderbridge.model.ShaderStage;
import java.nio.ByteBuffer;
import java.nio.LongBuffer;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import org.lwjgl.system.MemoryStack;
import org.lwjgl.system.MemoryUtil;
import org.lwjgl.vulkan.VK10;
import org.lwjgl.vulkan.VkComputePipelineCreateInfo;
import org.lwjgl.vulkan.VkDescriptorSetLayoutBinding;
import org.lwjgl.vulkan.VkDescriptorSetLayoutCreateInfo;
import org.lwjgl.vulkan.VkDevice;
import org.lwjgl.vulkan.VkPipelineLayoutCreateInfo;
import org.lwjgl.vulkan.VkShaderModuleCreateInfo;

/**
 * The Vulkan objects of one raw program: its descriptor set layouts (one per set number up to the
 * highest the program uses, empty for the gaps), its pipeline layout and its pipelines. A compute
 * program has one pipeline; a fullscreen program keeps its shader modules and has one graphics
 * pipeline per attachment configuration it is drawn with. Creation may run on any thread; later
 * graphics pipelines are made on the render thread. The objects are destroyed through
 * Minecraft's deferred destruction once the GPU is done with them.
 */
public final class PipelineObjects implements Destroyable {
    private final VkDevice device;
    private final List<Long> setLayouts;
    private final long layout;
    private final List<GraphicsPipelines.Stage> stages;
    private final Map<List<ColorStates.Slot>, Long> graphics = new HashMap<>();
    private final long compute;

    private PipelineObjects(VkDevice device, List<Long> setLayouts, long layout, List<GraphicsPipelines.Stage> stages, long compute) {
        this.device = device;
        this.setLayouts = List.copyOf(setLayouts);
        this.layout = layout;
        this.stages = List.copyOf(stages);
        this.compute = compute;
    }

    /**
     * A shader stage's code.
     *
     * @param stage      the stage
     * @param spirv      its SPIR-V
     * @param entryPoint its entry point
     */
    public record Code(ShaderStage stage, ByteBuffer spirv, String entryPoint) {
    }

    /**
     * Creates a compute pipeline.
     *
     * @param device the device
     * @param plan   the program's descriptor plan
     * @param code   its compute module
     * @return the objects
     * @throws RawVulkanException if Vulkan fails (nothing is left behind)
     */
    public static PipelineObjects compute(VkDevice device, DescriptorPlan plan, Code code) {
        Builder b = new Builder(device);
        try (MemoryStack stack = MemoryStack.stackPush()) {
            long layout = b.layout(plan);
            long module = b.module(code.spirv());
            VkComputePipelineCreateInfo.Buffer info = VkComputePipelineCreateInfo.calloc(1, stack).sType$Default().layout(layout);
            info.stage().sType$Default().stage(VK10.VK_SHADER_STAGE_COMPUTE_BIT).module(module).pName(stack.UTF8(code.entryPoint()));
            LongBuffer pPipeline = stack.mallocLong(1);
            RawVulkanException.check(VK10.vkCreateComputePipelines(device, VK10.VK_NULL_HANDLE, info, null, pPipeline), "vkCreateComputePipelines");
            b.destroyModules();
            return new PipelineObjects(device, b.setLayouts, layout, List.of(), pPipeline.get(0));
        } catch (RuntimeException e) {
            b.abandon();
            throw e;
        }
    }

    /**
     * Creates the layout and shader modules of a fullscreen program, and its pipeline for the
     * attachments it is expected to draw with.
     *
     * @param device   the device
     * @param plan     the program's descriptor plan
     * @param code     its stages
     * @param expected the color states it is expected to draw with
     * @return the objects
     * @throws RawVulkanException if Vulkan fails (nothing is left behind)
     */
    public static PipelineObjects fullscreen(VkDevice device, DescriptorPlan plan, List<Code> code, List<ColorStates.Slot> expected) {
        Builder b = new Builder(device);
        try {
            long layout = b.layout(plan);
            List<GraphicsPipelines.Stage> stages = new ArrayList<>();
            for (Code c : code) {
                stages.add(new GraphicsPipelines.Stage(StageFlags.of(c.stage()), b.module(c.spirv()), c.entryPoint()));
            }
            PipelineObjects objects = new PipelineObjects(device, b.setLayouts, layout, stages, VK10.VK_NULL_HANDLE);
            objects.graphics(expected);
            return objects;
        } catch (RuntimeException e) {
            b.abandon();
            throw e;
        }
    }

    /** @return the set layouts, indexed by set number */
    public List<Long> setLayouts() {
        return setLayouts;
    }

    /** @return the pipeline layout */
    public long layout() {
        return layout;
    }

    /** @return the compute pipeline */
    public long compute() {
        return compute;
    }

    /**
     * @param slots color attachment states
     * @return the graphics pipeline drawing with them, created on first use
     * @throws RawVulkanException if Vulkan fails
     */
    public long graphics(List<ColorStates.Slot> slots) {
        Long known = graphics.get(slots);
        if (known != null) {
            return known;
        }
        long pipeline = GraphicsPipelines.create(device, layout, stages, slots);
        graphics.put(List.copyOf(slots), pipeline);
        return pipeline;
    }

    @Override
    public void destroy() {
        if (compute != VK10.VK_NULL_HANDLE) {
            VK10.vkDestroyPipeline(device, compute, null);
        }
        graphics.values().forEach(p -> VK10.vkDestroyPipeline(device, p, null));
        stages.forEach(s -> VK10.vkDestroyShaderModule(device, s.module(), null));
        VK10.vkDestroyPipelineLayout(device, layout, null);
        setLayouts.forEach(l -> VK10.vkDestroyDescriptorSetLayout(device, l, null));
    }

    /** Creates the objects step by step, cleaning up after a failure. */
    private static final class Builder {
        private final VkDevice device;
        private final List<Long> setLayouts = new ArrayList<>();
        private final List<Long> modules = new ArrayList<>();
        private long layout = VK10.VK_NULL_HANDLE;

        Builder(VkDevice device) {
            this.device = device;
        }

        long layout(DescriptorPlan plan) {
            int count = plan.sets().isEmpty() ? 0 : plan.sets().getLast().set() + 1;
            for (int set = 0; set < count; set++) {
                setLayouts.add(setLayout(plan, set));
            }
            try (MemoryStack stack = MemoryStack.stackPush()) {
                LongBuffer layouts = stack.mallocLong(count);
                setLayouts.forEach(layouts::put);
                layouts.flip();
                VkPipelineLayoutCreateInfo info = VkPipelineLayoutCreateInfo.calloc(stack).sType$Default().pSetLayouts(layouts);
                LongBuffer pLayout = stack.mallocLong(1);
                RawVulkanException.check(VK10.vkCreatePipelineLayout(device, info, null, pLayout), "vkCreatePipelineLayout");
                layout = pLayout.get(0);
                return layout;
            }
        }

        private long setLayout(DescriptorPlan plan, int set) {
            List<DescriptorPlan.Binding> bindings = plan.sets().stream().filter(s -> s.set() == set).findFirst()
                .map(DescriptorPlan.SetLayout::bindings).orElse(List.of());
            try (MemoryStack stack = MemoryStack.stackPush()) {
                VkDescriptorSetLayoutBinding.Buffer infos = VkDescriptorSetLayoutBinding.calloc(bindings.size(), stack);
                for (int i = 0; i < bindings.size(); i++) {
                    DescriptorPlan.Binding b = bindings.get(i);
                    infos.get(i).binding(b.binding()).descriptorType(b.type()).descriptorCount(b.count()).stageFlags(b.stages());
                }
                VkDescriptorSetLayoutCreateInfo info = VkDescriptorSetLayoutCreateInfo.calloc(stack).sType$Default().pBindings(infos);
                LongBuffer pLayout = stack.mallocLong(1);
                RawVulkanException.check(VK10.vkCreateDescriptorSetLayout(device, info, null, pLayout), "vkCreateDescriptorSetLayout");
                return pLayout.get(0);
            }
        }

        long module(ByteBuffer spirv) {
            ByteBuffer code = MemoryUtil.memAlloc(spirv.remaining());
            try (MemoryStack stack = MemoryStack.stackPush()) {
                code.put(spirv.duplicate()).flip();
                VkShaderModuleCreateInfo info = VkShaderModuleCreateInfo.calloc(stack).sType$Default().pCode(code);
                LongBuffer pModule = stack.mallocLong(1);
                RawVulkanException.check(VK10.vkCreateShaderModule(device, info, null, pModule), "vkCreateShaderModule");
                modules.add(pModule.get(0));
                return pModule.get(0);
            } finally {
                MemoryUtil.memFree(code);
            }
        }

        void destroyModules() {
            modules.forEach(m -> VK10.vkDestroyShaderModule(device, m, null));
            modules.clear();
        }

        void abandon() {
            destroyModules();
            if (layout != VK10.VK_NULL_HANDLE) {
                VK10.vkDestroyPipelineLayout(device, layout, null);
            }
            setLayouts.forEach(l -> VK10.vkDestroyDescriptorSetLayout(device, l, null));
        }
    }
}
