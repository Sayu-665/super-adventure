package dev.shaderbridge.uniforms;

import com.mojang.renderpearl.api.buffers.GpuBufferSlice;
import com.mojang.renderpearl.api.commands.CommandEncoder;
import dev.shaderbridge.model.BlockLayout;
import dev.shaderbridge.model.BlockMember;
import dev.shaderbridge.model.UniformSource;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * The {@code sb_Frame} block of one dimension pipeline: builtin members are computed from the
 * {@link FrameState}, then the native evaluator writes the custom members, and the block is
 * uploaded to a uniform buffer (one per frame in flight). Members the pack declares but nobody
 * provides keep their constant initializer, or zero.
 */
public final class FrameUniforms implements AutoCloseable {
    private static final Logger LOGGER = LoggerFactory.getLogger("ShaderBridge");

    private final ByteBuffer block;
    private final Std140Writer writer;
    private final List<Binding> builtins;
    private final UniformEvaluator evaluator;
    private final DrawState defaultDraw = new DrawState();
    private final UniformWriter out = new UniformWriter();
    private final GpuBufferRing ring;
    private boolean evaluatorFailed;

    /** A builtin-sourced member and its provider. */
    record Binding(int offset, BlockMember member, BuiltinUniform builtin) {
    }

    /**
     * @param layout    the pipeline's {@code sb_Frame} layout
     * @param evaluator the custom-uniform evaluator of the pipeline, or null if it has none;
     *                  ownership passes to this object
     */
    public FrameUniforms(BlockLayout layout, UniformEvaluator evaluator) {
        int size = Math.max(16, layout.size());
        this.block = ByteBuffer.allocateDirect(size).order(ByteOrder.LITTLE_ENDIAN);
        this.writer = new Std140Writer(block);
        this.evaluator = evaluator;
        this.builtins = bind(layout, writer);
        this.ring = new GpuBufferRing("ShaderBridge sb_Frame", size);
    }

    /**
     * Resolves builtin members and writes the constant initializers of unset members.
     *
     * @return the builtin bindings in offset order
     */
    static List<Binding> bind(BlockLayout layout, Std140Writer writer) {
        List<Binding> bindings = new ArrayList<>();
        for (BlockMember member : layout.members()) {
            if (member.offset() < 0 || (long) member.offset() + Std140Writer.size(member.ty()) > Math.max(16, layout.size())) {
                LOGGER.warn("Ignoring {} member {} at {}: outside the {}-byte block", layout.name(), member.name(), member.offset(), layout.size());
                continue;
            }
            switch (member.source()) {
                case UniformSource.Builtin builtin -> {
                    Optional<BuiltinUniform> uniform = BuiltinUniforms.get(builtin.name());
                    if (uniform.isPresent()) {
                        bindings.add(new Binding(member.offset(), member, uniform.get()));
                    } else {
                        LOGGER.warn("Unknown builtin uniform {} in {}; it stays zero", builtin.name(), layout.name());
                    }
                }
                case UniformSource.Unset _ -> {
                    if (member.defaultValues() != null) {
                        writer.putDefault(member.offset(), member.ty(), member.defaultValues());
                    }
                }
                case UniformSource.Custom _ -> {
                    // Written by the native evaluator.
                }
            }
        }
        return List.copyOf(bindings);
    }

    /**
     * Computes the block for a frame.
     *
     * @param frame             the frame's state, already {@linkplain FrameState#update() updated}
     * @param frameDeltaSeconds time since the previous frame, for the custom-uniform {@code smooth()}
     * @return a read-only view of the block (position 0, limit = block size)
     */
    public ByteBuffer fill(FrameState frame, float frameDeltaSeconds) {
        defaultDraw.reset(frame);
        defaultDraw.update();
        for (Binding binding : builtins) {
            binding.builtin().provider().write(frame, defaultDraw, out.bind(writer, binding.offset(), binding.member().ty()));
        }
        if (evaluator != null && !evaluatorFailed) {
            evaluator.evaluate(block, frameDeltaSeconds).ifPresent(error -> {
                evaluatorFailed = true;
                LOGGER.error("Custom uniforms disabled after an evaluation error: {}", error);
            });
        }
        return block.asReadOnlyBuffer().order(ByteOrder.LITTLE_ENDIAN).clear();
    }

    /**
     * Uploads the last {@linkplain #fill filled} block into the next frame's buffer. Must be called
     * outside a render pass.
     *
     * @param encoder the command encoder of the frame
     * @return the slice to bind as {@code sb_Frame}
     */
    public GpuBufferSlice upload(CommandEncoder encoder) {
        GpuBufferSlice slice = ring.next().slice();
        encoder.writeToBuffer(slice, block.duplicate().clear());
        return slice;
    }

    @Override
    public void close() {
        ring.close();
        if (evaluator != null) {
            evaluator.close();
        }
    }
}
