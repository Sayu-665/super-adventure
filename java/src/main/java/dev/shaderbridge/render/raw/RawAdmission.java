package dev.shaderbridge.render.raw;

import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.ProgramKind;
import dev.shaderbridge.model.ResourceKind;
import dev.shaderbridge.model.ShaderStage;
import dev.shaderbridge.model.StageModule;
import dev.shaderbridge.render.pipeline.ProgramVariant;
import dev.shaderbridge.render.pipeline.SpirvReflection;
import dev.shaderbridge.render.pipeline.SpirvReflection.InterfaceVariable;
import dev.shaderbridge.render.pipeline.SpirvReflection.ScalarClass;
import dev.shaderbridge.render.pipeline.SpirvReflector;
import java.nio.ByteBuffer;
import java.util.ArrayList;
import java.util.EnumMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import org.lwjgl.vulkan.VK10;

/**
 * Decides whether the raw path can run a program, before any Vulkan object is made: its kind
 * (compute programs run between Minecraft's passes and composite-style programs in passes of
 * their own; geometry programs draw inside Minecraft's render passes, which the raw path cannot
 * record into), its SPIR-V (readable, with only the capabilities the device enabled, no push
 * constants, no vertex inputs for fullscreen draws, the store features its storage writes need),
 * its descriptors (bindable, fitting a pool) and its local size (within the device's limits).
 */
public final class RawAdmission {
    /** {@code VK_SHADER_STAGE_*} flags of the stages before rasterization. */
    private static final int VERTEX_PIPELINE = VK10.VK_SHADER_STAGE_VERTEX_BIT | VK10.VK_SHADER_STAGE_TESSELLATION_CONTROL_BIT
        | VK10.VK_SHADER_STAGE_TESSELLATION_EVALUATION_BIT | VK10.VK_SHADER_STAGE_GEOMETRY_BIT;

    private RawAdmission() {
    }

    /** The outcome. */
    public sealed interface Result {
        /**
         * A compute program the raw path can run.
         *
         * @param plan   its descriptor plan
         * @param module its compute module
         */
        record Compute(DescriptorPlan plan, StageModule module) implements Result {
        }

        /**
         * A composite-style program the raw path can draw.
         *
         * @param plan            its descriptor plan
         * @param modules         its stage modules, in pipeline order
         * @param fragmentOutputs its fragment output locations and their numeric class
         */
        record Fullscreen(DescriptorPlan plan, List<StageModule> modules, Map<Integer, ScalarClass> fragmentOutputs) implements Result {
            public Fullscreen {
                modules = List.copyOf(modules);
                fragmentOutputs = Map.copyOf(fragmentOutputs);
            }
        }

        /** @param reason why the raw path cannot run the program */
        record Rejected(String reason) implements Result {
        }
    }

    /**
     * @param dim      the program's dimension pipeline
     * @param variant  the program and its blobs
     * @param features what the device enabled
     * @param limits   the device's compute limits
     * @return whether and how the raw path runs it
     */
    public static Result check(DimensionPipeline dim, ProgramVariant variant, EnabledFeatures features, ComputeLimits limits) {
        Program program = variant.program();
        return switch (program.kind()) {
            case ProgramKind.Compute c -> compute(dim, variant, features, limits);
            case ProgramKind.GeometryCompute g -> compute(dim, variant, features, limits);
            case ProgramKind.Composite c -> fullscreen(dim, variant, features);
            case ProgramKind.Geometry g -> new Result.Rejected("the raw Vulkan path cannot draw inside Minecraft's render passes, where "
                + g.program().fileName() + " geometry is drawn");
        };
    }

    private static Result compute(DimensionPipeline dim, ProgramVariant variant, EnabledFeatures features, ComputeLimits limits) {
        Program program = variant.program();
        StageModule module = program.stage(ShaderStage.COMPUTE).orElse(null);
        if (module == null || module.spirv() == null || program.compute() == null || program.stages().size() != 1) {
            return new Result.Rejected("it has no compute module");
        }
        List<String> problems = new ArrayList<>();
        Map<ShaderStage, SpirvReflection> reflections = reflect(variant, List.of(module), features, problems);
        if (reflections == null) {
            return new Result.Rejected(String.join("; ", problems));
        }
        DescriptorPlan plan = DescriptorPlanner.plan(dim, program, reflections);
        problems.addAll(plan.problems());
        problems.addAll(DescriptorBudget.problems(plan));
        problems.addAll(limits.problems(program.compute()));
        if (program.compute().indirect() != null
            && dim.targets().buffers().stream().noneMatch(b -> b.index() == program.compute().indirect().buffer())) {
            problems.add("its indirect dispatch reads storage buffer " + program.compute().indirect().buffer() + ", which the pack does not declare");
        }
        return problems.isEmpty() ? new Result.Compute(plan, module) : new Result.Rejected(String.join("; ", problems));
    }

    private static Result fullscreen(DimensionPipeline dim, ProgramVariant variant, EnabledFeatures features) {
        Program program = variant.program();
        List<StageModule> modules = program.stages();
        if (program.stage(ShaderStage.VERTEX).isEmpty() || program.stage(ShaderStage.FRAGMENT).isEmpty()
            || modules.stream().anyMatch(m -> m.spirv() == null || m.stage() == ShaderStage.COMPUTE)) {
            return new Result.Rejected("it lacks a vertex or fragment module");
        }
        if (program.stage(ShaderStage.TESS_CONTROL).isPresent() || program.stage(ShaderStage.TESS_EVAL).isPresent()) {
            return new Result.Rejected("it has tessellation stages, which a fullscreen draw of triangles cannot feed");
        }
        List<String> problems = new ArrayList<>();
        Map<ShaderStage, SpirvReflection> reflections = reflect(variant, modules, features, problems);
        if (reflections == null) {
            return new Result.Rejected(String.join("; ", problems));
        }
        if (!reflections.get(ShaderStage.VERTEX).inputs().isEmpty()) {
            problems.add("its vertex stage reads vertex attributes, which fullscreen draws do not have");
        }
        DescriptorPlan plan = DescriptorPlanner.plan(dim, program, reflections);
        problems.addAll(plan.problems());
        problems.addAll(DescriptorBudget.problems(plan));
        problems.addAll(storeFeatures(plan, features));
        Map<Integer, ScalarClass> outputs = new TreeMap<>();
        for (InterfaceVariable output : reflections.get(ShaderStage.FRAGMENT).outputs()) {
            for (int k = 0; output.location() >= 0 && k < Math.max(1, output.locationCount()); k++) {
                outputs.put(output.location() + k, output.scalar());
            }
        }
        return problems.isEmpty() ? new Result.Fullscreen(plan, modules, outputs) : new Result.Rejected(String.join("; ", problems));
    }

    /** Reflects the modules and checks their capabilities; null (with the problems) if one cannot be read. */
    private static Map<ShaderStage, SpirvReflection> reflect(ProgramVariant variant, List<StageModule> modules, EnabledFeatures features,
                                                            List<String> problems) {
        Map<ShaderStage, SpirvReflection> reflections = new EnumMap<>(ShaderStage.class);
        for (StageModule module : modules) {
            try {
                ByteBuffer spirv = variant.blobs().spirv(module.spirv());
                SpirvReflection reflection = SpirvReflector.reflect(spirv);
                reflections.put(module.stage(), reflection);
                problems.addAll(SpirvCapabilities.problems(SpirvCapabilities.read(spirv), module.stage(), features));
                if (reflection.pushConstantBlocks() > 0) {
                    problems.add("its " + module.stage().packExtension() + " stage declares push constants, which the raw path does not provide");
                }
            } catch (IllegalArgumentException | IndexOutOfBoundsException e) {
                problems.add("its SPIR-V cannot be read: " + e.getMessage());
                return null;
            }
        }
        return reflections;
    }

    /**
     * Storage buffers and writable storage images in graphics stages need
     * {@code fragmentStoresAndAtomics} or {@code vertexPipelineStoresAndAtomics}. Storage buffers
     * count as written (their blocks' access qualifiers are not part of the model).
     */
    private static List<String> storeFeatures(DescriptorPlan plan, EnabledFeatures features) {
        int stages = 0;
        for (DescriptorPlan.SetLayout set : plan.sets()) {
            for (DescriptorPlan.Binding b : set.bindings()) {
                boolean readonlyImage = b.source() instanceof DescriptorPlan.Source.Pack p && p.entry().kind() instanceof ResourceKind.StorageImage i
                    && i.readonly();
                if (b.type() == VK10.VK_DESCRIPTOR_TYPE_STORAGE_BUFFER || b.type() == VK10.VK_DESCRIPTOR_TYPE_STORAGE_IMAGE && !readonlyImage) {
                    stages |= b.stages();
                }
            }
        }
        List<String> problems = new ArrayList<>();
        if ((stages & VK10.VK_SHADER_STAGE_FRAGMENT_BIT) != 0 && !features.has(RawFeature.FRAGMENT_STORES_AND_ATOMICS)) {
            problems.add("its fragment stage uses storage buffers or images, which needs the device feature fragmentStoresAndAtomics");
        }
        if ((stages & VERTEX_PIPELINE) != 0 && !features.has(RawFeature.VERTEX_PIPELINE_STORES_AND_ATOMICS)) {
            problems.add("its vertex stages use storage buffers or images, which needs the device feature vertexPipelineStoresAndAtomics");
        }
        return problems;
    }
}
