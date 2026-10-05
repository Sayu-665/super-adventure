package dev.shaderbridge.render.raw;

import com.mojang.renderpearl.api.GpuFormat;
import com.mojang.renderpearl.api.textures.GpuTextureView;
import com.mojang.renderpearl.backend.vulkan.VulkanGpuTextureView;
import dev.shaderbridge.model.DimensionPipeline;
import dev.shaderbridge.model.IndirectDispatch;
import dev.shaderbridge.model.PassGroup;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.ProgramKind;
import dev.shaderbridge.render.pipeline.AttachmentLayout;
import dev.shaderbridge.render.pipeline.ProgramVariant;
import dev.shaderbridge.render.pipeline.RawDispatch;
import dev.shaderbridge.render.pipeline.RawDraw;
import dev.shaderbridge.render.pipeline.RawProgram;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;
import java.util.function.Consumer;
import org.lwjgl.vulkan.VK10;
import org.lwjgl.vulkan.VkCommandBuffer;

/**
 * The raw Vulkan path of one dimension pipeline on Minecraft's Vulkan device: compute programs as
 * Vulkan compute pipelines and composite-style programs as graphics pipelines, built from the
 * pack's SPIR-V and run between Minecraft's passes in their own command buffers
 * ({@code VulkanCommandEncoder.allocateAndBeginTransientCommandBuffer} and {@code execute}),
 * bracketed by full memory barriers, with every image in {@code GENERAL}. Geometry programs are
 * declined (their slots fall back along the pack's program chain). Render thread only.
 */
final class VulkanRawPath implements RawBackend.DimensionRawPath {
    private final VulkanContext ctx;
    private final RawContext raw;
    private final RawResources resources;
    private final RawSamplers samplers;
    private final DescriptorAllocator descriptors;
    private final RawBindings bindings;

    /**
     * @param ctx       the Vulkan context
     * @param raw       what the raw path works with
     * @param resources the raw path's own resources of the dimension pipeline
     */
    VulkanRawPath(VulkanContext ctx, RawContext raw, RawResources resources) {
        this.ctx = ctx;
        this.raw = raw;
        this.resources = resources;
        this.samplers = new RawSamplers(ctx);
        this.descriptors = new DescriptorAllocator(ctx);
        this.bindings = new RawBindings(ctx, raw, resources, samplers);
    }

    @Override
    public Admission admit(DimensionPipeline dim, ProgramVariant program, List<String> renderpearlProblems) {
        return switch (RawAdmission.check(dim, program, ctx.features(), ctx.compute())) {
            case RawAdmission.Result.Compute compute -> new Admission.Accepted(RawShaderProgram.compute(ctx, program, compute, raw.compiler()));
            case RawAdmission.Result.Fullscreen fullscreen -> {
                List<ColorStates.Slot> expected = ColorStates.of(program.program(), expectedFormats(dim, program.program()),
                    fullscreen.fragmentOutputs());
                Optional<String> problem = ColorStates.problem(expected, ctx.features().has(RawFeature.INDEPENDENT_BLEND));
                yield problem.<Admission>map(Admission.Rejected::new)
                    .orElseGet(() -> new Admission.Accepted(RawShaderProgram.fullscreen(ctx, program, fullscreen, expected, raw.compiler())));
            }
            case RawAdmission.Result.Rejected rejected -> new Admission.Rejected(rejected.reason());
        };
    }

    /** The attachments a composite-style program draws with when all its targets exist: its draw buffers, or the main target for {@code final}. */
    private List<Optional<GpuFormat>> expectedFormats(DimensionPipeline dim, Program program) {
        if (program.kind() instanceof ProgramKind.Composite c && c.group() == PassGroup.FINAL) {
            return List.of(Optional.of(raw.mainColor().get()));
        }
        return AttachmentLayout.fullscreen(dim, program).attachments().stream().map(a -> Optional.of(a.format())).toList();
    }

    @Override
    public int[] maxWorkGroups() {
        return ctx.compute().maxGroupCountArray();
    }

    @Override
    public void dispatch(RawProgram program, RawDispatch dispatch) {
        RawShaderProgram shader = (RawShaderProgram) program;
        PipelineObjects objects = shader.objects();
        Program model = program.program().program();
        updateResources();
        Optional<VmaBuffer> indirect = indirectBuffer(model);
        if (model.compute().indirect() != null && indirect.isEmpty()) {
            return;
        }
        RawUse use = new RawUse(model, dispatch.frame(), dispatch.frameUniforms(), dispatch.drawUniforms(), dispatch.colorAlt(),
            dispatch.shadowColorAlt());
        record(shader, objects, use, VK10.VK_PIPELINE_BIND_POINT_COMPUTE, objects.compute(), indirect, commands -> {
            if (indirect.isPresent()) {
                VK10.vkCmdDispatchIndirect(commands, indirect.get().buffer(), Integer.toUnsignedLong(model.compute().indirect().offset()));
            } else {
                List<Integer> groups = dispatch.workGroups();
                VK10.vkCmdDispatch(commands, groups.get(0), groups.get(1), groups.get(2));
            }
        });
    }

    @Override
    public List<Boolean> draw(RawProgram program, RawDraw draw) {
        RawShaderProgram shader = (RawShaderProgram) program;
        PipelineObjects objects = shader.objects();
        Program model = program.program().program();
        List<Optional<GpuFormat>> formats = draw.attachments().stream().map(v -> v.map(view -> view.texture().getFormat())).toList();
        List<ColorStates.Slot> slots = ColorStates.of(model, formats, shader.fragmentOutputs());
        Optional<String> problem = ColorStates.problem(slots, ctx.features().has(RawFeature.INDEPENDENT_BLEND));
        if (problem.isPresent()) {
            throw new IllegalStateException(problem.get());
        }
        long pipeline = objects.graphics(slots);
        updateResources();
        RawUse use = new RawUse(model, draw.frame(), draw.frameUniforms(), draw.drawUniforms(), draw.colorAlt(), draw.shadowColorAlt());
        List<Long> views = draw.attachments().stream().map(v -> v.map(VulkanRawPath::vkView).orElse(VK10.VK_NULL_HANDLE)).toList();
        record(shader, objects, use, VK10.VK_PIPELINE_BIND_POINT_GRAPHICS, pipeline, Optional.empty(),
            commands -> FullscreenRendering.draw(commands, views, draw.width(), draw.height()));
        return slots.stream().map(ColorStates.Slot::write).toList();
    }

    private static long vkView(GpuTextureView view) {
        return ((VulkanGpuTextureView) view).vkImageView();
    }

    /** Creates the raw path's own resources on first use, and recreates the screen-relative ones after a resize. */
    private void updateResources() {
        int[] screen = raw.targets().screenSize();
        resources.update(screen[0], screen[1]);
    }

    /**
     * Resolves and writes the program's descriptor sets, then records and submits its own command
     * buffer: the {@link CommandPlan} around binding the pipeline and sets and running {@code work}.
     */
    private void record(RawShaderProgram shader, PipelineObjects objects, RawUse use, int bindPoint, long pipeline, Optional<VmaBuffer> extra,
                        Consumer<VkCommandBuffer> work) {
        DescriptorPlan plan = shader.plan();
        long[] sets = descriptors.allocate(plan.sets().stream().map(s -> objects.setLayouts().get(s.set())).toList());
        List<Object> used = new ArrayList<>();
        List<DescriptorWrites.Write> writes = new ArrayList<>();
        for (int i = 0; i < plan.sets().size(); i++) {
            for (DescriptorPlan.Binding binding : plan.sets().get(i).bindings()) {
                RawBindings.Bound bound = bindings.resolve(binding, use);
                writes.add(new DescriptorWrites.Write(sets[i], binding, bound));
                if (bound.own() != null) {
                    used.add(bound.own());
                }
            }
        }
        extra.ifPresent(used::add);
        DescriptorWrites.apply(ctx.vk(), writes);
        VkCommandBuffer cb = ctx.encoder().allocateAndBeginTransientCommandBuffer();
        CommandRecorder.record(cb, CommandPlan.of(resources.init().before(used, use.frame())), new CommandRecorder.Steps() {
            @Override
            public void fill(VkCommandBuffer commands, Object resource, boolean initial) {
                resources.fill(commands, resource, initial);
            }

            @Override
            public void run(VkCommandBuffer commands) {
                VK10.vkCmdBindPipeline(commands, bindPoint, pipeline);
                for (int i = 0; i < plan.sets().size(); i++) {
                    VK10.vkCmdBindDescriptorSets(commands, bindPoint, objects.layout(), plan.sets().get(i).set(), new long[] {sets[i]}, null);
                }
                work.accept(commands);
            }
        });
        RawVulkanException.check(VK10.vkEndCommandBuffer(cb), "vkEndCommandBuffer");
        ctx.encoder().execute(cb);
    }

    /** The storage buffer of an indirect dispatch, empty (reported) if it cannot hold the arguments. */
    private Optional<VmaBuffer> indirectBuffer(Program model) {
        IndirectDispatch indirect = model.compute().indirect();
        if (indirect == null) {
            return Optional.empty();
        }
        Optional<VmaBuffer> buffer = resources.buffer(indirect.buffer());
        Optional<String> problem = ComputeLimits.indirectProblem(indirect, buffer.map(VmaBuffer::size));
        problem.ifPresent(p -> raw.diagnostics().report(model.name() + " was not dispatched: " + p));
        return problem.isPresent() ? Optional.empty() : buffer;
    }

    @Override
    public void close() {
        descriptors.close();
        samplers.close();
        resources.close();
    }
}
