package dev.shaderbridge.render.pipeline;

import com.mojang.renderpearl.api.pipeline.RenderPipeline;
import com.mojang.renderpearl.api.vertex.VertexFormat;
import dev.shaderbridge.model.DepthMode;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.ShaderStage;
import dev.shaderbridge.model.StageModule;
import java.util.ArrayList;
import java.util.List;
import net.minecraft.resources.Identifier;

/**
 * Turns a pack program into a renderpearl {@link RenderPipeline}: reflects its SPIR-V, checks it
 * against renderpearl's limits, plans its color targets, registers its modules for the compiler
 * hook and assembles the pipeline (shader ids, bind group layout, color targets, depth state per
 * {@link DepthMode}, culling, vertex bindings and topology of the replaced draw). Nothing is
 * compiled here; see {@link PackPipelineCache}.
 */
public final class PackPipelineFactory {
    private final String packHash;
    private final SpirvModules modules;
    private final PipelineCapabilities capabilities;
    private final DrawProfiles profiles;

    /**
     * @param packHash     the pack's source hash (pipeline locations)
     * @param modules      the registry the compiler hook reads
     * @param capabilities device capabilities
     * @param profiles     known draw profiles
     */
    public PackPipelineFactory(String packHash, SpirvModules modules, PipelineCapabilities capabilities, DrawProfiles profiles) {
        this.packHash = packHash;
        this.modules = modules;
        this.capabilities = capabilities;
        this.profiles = profiles;
    }

    /** Outcome of {@link #build}. */
    public sealed interface Result {
        /** @param pipeline the pipeline, its modules registered */
        record Built(PackPipeline pipeline) implements Result {
        }

        /** @param reasons why renderpearl cannot run the program as asked */
        record Ineligible(List<String> reasons) implements Result {
        }
    }

    /**
     * @param dim       the program's dimension pipeline (binding table, uniform layout, targets)
     * @param variant   the program and its blobs
     * @param shape     the draw it replaces
     * @param layout    the attachments of the pass it draws in
     * @param depthMode the depth convention the pack was compiled for
     * @return the pipeline, or why there is none
     */
    public Result build(DimensionPipeline dim, ProgramVariant variant, PipelineShape shape, AttachmentLayout layout, DepthMode depthMode) {
        Program program = variant.program();
        ProgramInterface iface;
        try {
            iface = ProgramInterface.reflect(program, variant.blobs());
        } catch (IllegalArgumentException e) {
            return new Result.Ineligible(List.of("its SPIR-V cannot be read: " + e.getMessage()));
        }
        List<String> problems = new ArrayList<>(Eligibility.check(program, iface, shape, capabilities));
        AttachmentPlan attachments = AttachmentPlanner.plan(program, layout, iface.fragmentOutputs(), capabilities);
        problems.addAll(attachments.problems());
        BindingPlan bindings = BindingPlan.of(iface, program, dim.bindings(), dim.uniforms(), profiles.profile(variant.profile()));
        bindings.unresolved().forEach(n -> problems.add("nothing provides descriptor " + n));
        if (!problems.isEmpty()) {
            return new Result.Ineligible(problems);
        }
        PipelineKey key = new PipelineKey(variant.folder(), program.name(), variant.profile(), shape.id(), layout.id());
        Identifier vertex = modules.register(variant.blobs().spirv(module(program, ShaderStage.VERTEX).spirv()));
        Identifier fragment = modules.register(variant.blobs().spirv(module(program, ShaderStage.FRAGMENT).spirv()));
        try {
            RenderPipeline pipeline = assemble(key, program, iface, shape, attachments, depthMode, vertex, fragment);
            return new Result.Built(new PackPipeline(key, pipeline, bindings, attachments, List.of(vertex, fragment)));
        } catch (IllegalArgumentException | IllegalStateException e) {
            modules.release(vertex);
            modules.release(fragment);
            return new Result.Ineligible(List.of("Mojang's pipeline builder rejects it: " + e.getMessage()));
        }
    }

    private RenderPipeline assemble(PipelineKey key, Program program, ProgramInterface iface, PipelineShape shape, AttachmentPlan attachments,
                                    DepthMode depthMode, Identifier vertex, Identifier fragment) {
        RenderPipeline.Builder builder = RenderPipeline.builder()
            .withLocation(key.location(packHash))
            .withVertexShader(vertex)
            .withFragmentShader(fragment)
            .withBindGroupLayout(BindGroups.layout(iface, shape))
            .withPolygonMode(shape.polygonMode())
            .withCull(program.cull() != null ? program.cull() : shape.cull())
            .withPrimitiveTopology(shape.topology())
            .withPushConstantSize(Math.max(program.pushConstantSize(), shape.pushConstantSize()));
        for (int slot = 0; slot < attachments.slots().size(); slot++) {
            builder.withColorTargetState(slot, attachments.slots().get(slot).state());
        }
        if (shape.depth() != null) {
            builder.withDepthStencilState(DepthStates.forPack(depthMode, shape.depth()));
        }
        List<VertexFormat> bindings = shape.vertexBindings();
        for (int i = 0; i < bindings.size(); i++) {
            if (bindings.get(i) != null) {
                builder.withVertexBinding(i, bindings.get(i));
            }
        }
        return builder.build();
    }

    private static StageModule module(Program program, ShaderStage stage) {
        return program.stage(stage).orElseThrow(() -> new IllegalStateException(program.name() + " has no " + stage.wireName() + " stage"));
    }
}
