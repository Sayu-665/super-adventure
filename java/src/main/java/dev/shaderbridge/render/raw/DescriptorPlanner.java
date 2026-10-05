package dev.shaderbridge.render.raw;

import dev.shaderbridge.model.BindingEntry;
import dev.shaderbridge.model.BindingUse;
import dev.shaderbridge.model.BlockLayout;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.ResourceKind;
import dev.shaderbridge.model.ResourceRef;
import dev.shaderbridge.model.ShaderStage;
import dev.shaderbridge.render.pipeline.SpirvReflection;
import dev.shaderbridge.render.pipeline.SpirvReflection.Descriptor;
import dev.shaderbridge.render.pipeline.SpirvReflection.ImageDim;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.TreeMap;
import org.lwjgl.vulkan.VK10;

/**
 * Plans the descriptor set layouts of a raw program from its stages' SPIR-V and the model: every
 * descriptor the entry points use, at its decorated set and binding, with the
 * {@code VkDescriptorType} its SPIR-V type requires and the stages that use it, matched to
 * {@code sb_Frame} / {@code sb_Draw} (the dimension's uniform layout) or to the pack resource of
 * the binding table at that set and binding ({@code bindings_used} first, for its {@code use_alt}).
 */
public final class DescriptorPlanner {
    private DescriptorPlanner() {
    }

    /**
     * @param dim     the program's dimension pipeline
     * @param program the program
     * @param stages  the reflection of each of its stages
     * @return the plan; its problems say why the raw path cannot bind the program
     */
    public static DescriptorPlan plan(DimensionPipeline dim, Program program, Map<ShaderStage, SpirvReflection> stages) {
        List<String> problems = new ArrayList<>();
        Map<Integer, Map<Integer, DescriptorPlan.Binding>> sets = new TreeMap<>();
        for (Map.Entry<ShaderStage, SpirvReflection> stage : stages.entrySet()) {
            for (Descriptor d : stage.getValue().descriptors()) {
                Optional<DescriptorPlan.Binding> binding = binding(dim, program, d, StageFlags.of(stage.getKey()), problems);
                binding.ifPresent(b -> merge(sets.computeIfAbsent(b.set(), k -> new TreeMap<>()), b, problems));
            }
        }
        List<DescriptorPlan.SetLayout> layouts = new ArrayList<>();
        sets.forEach((set, bindings) -> layouts.add(new DescriptorPlan.SetLayout(set, List.copyOf(bindings.values()))));
        return new DescriptorPlan(layouts, problems);
    }

    private static Optional<DescriptorPlan.Binding> binding(DimensionPipeline dim, Program program, Descriptor d, int stage, List<String> problems) {
        if (!d.decorated()) {
            problems.add(d.name() + " has no descriptor set and binding");
            return Optional.empty();
        }
        if (d.set() >= DescriptorPlan.MAX_SETS) {
            problems.add(d.name() + " is in descriptor set " + d.set() + "; devices only guarantee sets 0 to " + (DescriptorPlan.MAX_SETS - 1));
            return Optional.empty();
        }
        if (d.arraySize() < 1) {
            problems.add(d.name() + " is a runtime-sized descriptor array");
            return Optional.empty();
        }
        Optional<Integer> type = vkType(d);
        if (type.isEmpty()) {
            problems.add(d.name() + " is a " + d.type() + " descriptor (" + d.dim() + (d.arrayed() ? ", array" : "")
                + (d.multisampled() ? ", multisampled" : "") + "), which the raw path does not bind");
            return Optional.empty();
        }
        Optional<DescriptorPlan.Source> source = source(dim, program, d, type.get(), problems);
        return source.map(s -> new DescriptorPlan.Binding(d.set(), d.binding(), d.name(), type.get(), d.arraySize(), stage, d, s));
    }

    /**
     * The descriptor type of a reflected descriptor, empty for kinds the raw path does not bind:
     * besides buffers, only single-sampled 1D, 2D and 3D images that are not arrays.
     */
    static Optional<Integer> vkType(Descriptor d) {
        boolean image = d.type() == SpirvReflection.DescriptorType.SAMPLED_IMAGE || d.type() == SpirvReflection.DescriptorType.STORAGE_IMAGE;
        if (image && (d.multisampled() || d.arrayed() || d.dim() != ImageDim.D1 && d.dim() != ImageDim.D2 && d.dim() != ImageDim.D3)) {
            return Optional.empty();
        }
        return switch (d.type()) {
            case UNIFORM_BUFFER -> Optional.of(VK10.VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER);
            case STORAGE_BUFFER -> Optional.of(VK10.VK_DESCRIPTOR_TYPE_STORAGE_BUFFER);
            case SAMPLED_IMAGE -> Optional.of(VK10.VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER);
            case STORAGE_IMAGE -> Optional.of(VK10.VK_DESCRIPTOR_TYPE_STORAGE_IMAGE);
            case SEPARATE_IMAGE, SEPARATE_SAMPLER, OTHER -> Optional.empty();
        };
    }

    private static Optional<DescriptorPlan.Source> source(DimensionPipeline dim, Program program, Descriptor d, int type, List<String> problems) {
        if (type == VK10.VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER) {
            if (at(dim.uniforms().frame(), d)) {
                return Optional.of(new DescriptorPlan.Source.Frame());
            }
            if (at(dim.uniforms().draw(), d)) {
                return Optional.of(new DescriptorPlan.Source.Draw());
            }
        }
        Optional<DescriptorPlan.Source.Pack> pack = packResource(dim, program, d);
        if (pack.isEmpty()) {
            problems.add(d.name() + " (set " + d.set() + ", binding " + d.binding() + ") is not in the pack's binding table");
            return Optional.empty();
        }
        BindingEntry entry = pack.get().entry();
        if (entry.resource() instanceof ResourceRef.UniformBlock block) {
            problems.add(d.name() + " is Minecraft's " + block.name() + " block, which only Minecraft's draws provide");
            return Optional.empty();
        }
        if (!matches(entry.kind(), type)) {
            problems.add(d.name() + " is declared as descriptor type " + type + " but the binding table has a " + entry.kind());
            return Optional.empty();
        }
        return Optional.of(pack.get());
    }

    private static boolean at(BlockLayout block, Descriptor d) {
        return block.size() > 0 && block.set() == d.set() && block.binding() == d.binding();
    }

    private static Optional<DescriptorPlan.Source.Pack> packResource(DimensionPipeline dim, Program program, Descriptor d) {
        for (BindingUse use : program.bindingsUsed()) {
            if (use.set() == d.set() && use.binding() == d.binding()) {
                Optional<BindingEntry> entry = dim.bindings().entries().stream().filter(e -> e.name().equals(use.name())).findFirst();
                if (entry.isPresent()) {
                    return Optional.of(new DescriptorPlan.Source.Pack(entry.get(), use.useAlt()));
                }
            }
        }
        return dim.bindings().entries().stream().filter(e -> e.set() == d.set() && e.binding() == d.binding()).findFirst()
            .map(e -> new DescriptorPlan.Source.Pack(e, false));
    }

    private static boolean matches(ResourceKind kind, int type) {
        return switch (kind) {
            case ResourceKind.Sampler s -> type == VK10.VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER;
            case ResourceKind.StorageImage s -> type == VK10.VK_DESCRIPTOR_TYPE_STORAGE_IMAGE;
            case ResourceKind.StorageBuffer s -> type == VK10.VK_DESCRIPTOR_TYPE_STORAGE_BUFFER;
            case ResourceKind.UniformBuffer u -> type == VK10.VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER;
        };
    }

    /** Adds a stage's binding to its set, or merges its stage into the same binding of another stage. */
    private static void merge(Map<Integer, DescriptorPlan.Binding> set, DescriptorPlan.Binding b, List<String> problems) {
        DescriptorPlan.Binding known = set.get(b.binding());
        if (known == null) {
            set.put(b.binding(), b);
        } else if (known.type() != b.type() || known.count() != b.count() || !known.source().equals(b.source())) {
            problems.add("set " + b.set() + ", binding " + b.binding() + " is " + known.name() + " in one stage and " + b.name() + " in another");
        } else {
            set.put(b.binding(), new DescriptorPlan.Binding(known.set(), known.binding(), known.name(), known.type(), known.count(),
                known.stages() | b.stages(), known.descriptor(), known.source()));
        }
    }
}
