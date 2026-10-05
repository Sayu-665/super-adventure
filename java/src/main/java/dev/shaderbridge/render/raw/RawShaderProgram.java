package dev.shaderbridge.render.raw;

import dev.shaderbridge.model.StageModule;
import dev.shaderbridge.render.pipeline.ProgramVariant;
import dev.shaderbridge.render.pipeline.RawProgram;
import dev.shaderbridge.render.pipeline.SpirvReflection.ScalarClass;
import java.nio.ByteBuffer;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.CompletionException;
import java.util.concurrent.Executor;
import java.util.function.Supplier;

/**
 * A program on the raw path: its descriptor plan, and its pipeline objects, created off the render
 * thread. Objects of a program closed while still compiling are destroyed when the compile ends
 * (they were never used); those of a ready program through Minecraft's deferred destruction.
 * Render thread only, apart from the compile.
 */
final class RawShaderProgram implements RawProgram {
    private final ProgramVariant variant;
    private final DescriptorPlan plan;
    private final Map<Integer, ScalarClass> fragmentOutputs;
    private final VulkanContext ctx;
    private final CompletableFuture<PipelineObjects> objects;
    private boolean closed;

    private RawShaderProgram(ProgramVariant variant, DescriptorPlan plan, Map<Integer, ScalarClass> fragmentOutputs, VulkanContext ctx,
                             Supplier<PipelineObjects> create, Executor executor) {
        this.variant = variant;
        this.plan = plan;
        this.fragmentOutputs = Map.copyOf(fragmentOutputs);
        this.ctx = ctx;
        this.objects = CompletableFuture.supplyAsync(create, executor);
    }

    /**
     * Starts creating a compute program.
     *
     * @param ctx      the Vulkan context
     * @param variant  the program
     * @param admitted its admission
     * @param executor runs the creation
     * @return the program, {@link State.Preparing} until its pipeline exists
     */
    static RawShaderProgram compute(VulkanContext ctx, ProgramVariant variant, RawAdmission.Result.Compute admitted, Executor executor) {
        PipelineObjects.Code code = code(variant, admitted.module());
        return new RawShaderProgram(variant, admitted.plan(), Map.of(), ctx, () -> PipelineObjects.compute(ctx.vk(), admitted.plan(), code), executor);
    }

    /**
     * Starts creating a composite-style program.
     *
     * @param ctx      the Vulkan context
     * @param variant  the program
     * @param admitted its admission
     * @param expected the color states it is expected to draw with (its pipeline for them is made now)
     * @param executor runs the creation
     * @return the program, {@link State.Preparing} until its layout, modules and expected pipeline exist
     */
    static RawShaderProgram fullscreen(VulkanContext ctx, ProgramVariant variant, RawAdmission.Result.Fullscreen admitted,
                                       List<ColorStates.Slot> expected, Executor executor) {
        List<PipelineObjects.Code> code = admitted.modules().stream().map(m -> code(variant, m)).toList();
        return new RawShaderProgram(variant, admitted.plan(), admitted.fragmentOutputs(), ctx,
            () -> PipelineObjects.fullscreen(ctx.vk(), admitted.plan(), code, expected), executor);
    }

    /** A copy of a module's SPIR-V, independent of the pack's blob buffer (which may be freed while compiling). */
    private static PipelineObjects.Code code(ProgramVariant variant, StageModule module) {
        ByteBuffer source = variant.blobs().spirv(module.spirv());
        byte[] spirv = new byte[source.remaining()];
        source.duplicate().get(spirv);
        return new PipelineObjects.Code(module.stage(), ByteBuffer.wrap(spirv), module.entryPoint());
    }

    @Override
    public ProgramVariant program() {
        return variant;
    }

    /** @return the descriptor plan */
    DescriptorPlan plan() {
        return plan;
    }

    /** @return the fragment output locations and their numeric class (empty for compute programs) */
    Map<Integer, ScalarClass> fragmentOutputs() {
        return fragmentOutputs;
    }

    @Override
    public State state() {
        if (!objects.isDone()) {
            return new State.Preparing();
        }
        if (objects.isCompletedExceptionally()) {
            Throwable cause = objects.handle((o, e) -> e instanceof CompletionException c && c.getCause() != null ? c.getCause() : e).join();
            return new State.Failed("its pipeline could not be created: " + cause.getMessage());
        }
        return new State.Ready();
    }

    /**
     * @return the pipeline objects
     * @throws IllegalStateException unless the program is {@linkplain State.Ready ready}
     */
    PipelineObjects objects() {
        if (!(state() instanceof State.Ready) || closed) {
            throw new IllegalStateException(variant.program().name() + " is not ready");
        }
        return objects.join();
    }

    @Override
    public void close() {
        if (closed) {
            return;
        }
        closed = true;
        if (objects.isDone()) {
            if (!objects.isCompletedExceptionally()) {
                ctx.destroyLater(objects.join());
            }
        } else {
            objects.thenAccept(PipelineObjects::destroy);
        }
    }
}
