package dev.shaderbridge.render.pipeline;

import com.mojang.renderpearl.api.pipeline.BindGroupLayout;
import com.mojang.renderpearl.api.pipeline.UniformType;
import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.ProgramKind;
import dev.shaderbridge.model.ShaderStage;
import dev.shaderbridge.render.pipeline.SpirvReflection.Descriptor;
import dev.shaderbridge.render.pipeline.SpirvReflection.InterfaceVariable;
import dev.shaderbridge.render.pipeline.SpirvReflection.ScalarClass;
import java.util.ArrayList;
import java.util.EnumSet;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;

/**
 * Whether a program can run as a renderpearl {@code RenderPipeline}: Mojang's pipeline API takes a
 * vertex and a fragment stage, uniform buffers, combined image samplers (2D, cube, rectangle) and
 * texel buffers whose format the host declares, at most 128 bytes of push constants, and checks the
 * stage interfaces like {@code PipelineBuilder} does. Programs that fail go to the raw-Vulkan path
 * or fall back along the pack's program chain.
 */
public final class Eligibility {
    /** Mojang's push constant limit in bytes. */
    public static final int MAX_PUSH_CONSTANTS = 128;

    private Eligibility() {
    }

    /**
     * @param program      the program
     * @param iface        its reflected interface
     * @param shape        the draw it would replace
     * @param capabilities device capabilities
     * @return why the program cannot be a renderpearl pipeline; empty if it can
     */
    public static List<String> check(Program program, ProgramInterface iface, PipelineShape shape, PipelineCapabilities capabilities) {
        List<String> problems = new ArrayList<>();
        if (program.requiresRawVulkan()) {
            problems.add("the translator marked it raw-Vulkan only (1D/3D textures, storage images, SSBOs or extra stages)");
        }
        if (program.kind() instanceof ProgramKind.Compute || program.kind() instanceof ProgramKind.GeometryCompute) {
            problems.add("it is a compute program");
        }
        Set<ShaderStage> stages = EnumSet.noneOf(ShaderStage.class);
        program.stages().forEach(s -> stages.add(s.stage()));
        if (!stages.equals(EnumSet.of(ShaderStage.VERTEX, ShaderStage.FRAGMENT))) {
            problems.add("it has stages " + stages + "; renderpearl pipelines have exactly a vertex and a fragment stage");
        }
        if (program.pushConstantSize() > MAX_PUSH_CONSTANTS) {
            problems.add(program.pushConstantSize() + " bytes of push constants exceed " + MAX_PUSH_CONSTANTS);
        }
        iface.conflicts().forEach(n -> problems.add("descriptor " + n + " has different types in different stages"));
        if (iface.descriptors().size() > capabilities.maxDescriptors()) {
            problems.add(iface.descriptors().size() + " descriptors exceed the limit of " + capabilities.maxDescriptors());
        }
        for (Descriptor d : iface.descriptors().values()) {
            descriptorProblem(d, shape).ifPresent(problems::add);
        }
        for (SpirvReflection stage : iface.stages().values()) {
            if (stage.pushConstantBlocks() > 1) {
                problems.add("a stage declares " + stage.pushConstantBlocks() + " push constant blocks");
            }
        }
        problems.addAll(VertexInputCheck.check(shape.vertexBindings(), iface.vertexInputs()));
        problems.addAll(stageInterfaceProblems(iface));
        problems.addAll(locationProblems(iface));
        return problems;
    }

    /**
     * Mojang's reflection looks up the {@code Location} decoration of every stage input and
     * output and fails without one; vertex outputs and fragment inputs are checked with the
     * stage interface, vertex inputs and fragment outputs here.
     */
    static List<String> locationProblems(ProgramInterface iface) {
        List<String> problems = new ArrayList<>();
        for (InterfaceVariable v : iface.vertexInputs()) {
            if (v.location() < 0) {
                problems.add("vertex input " + v.name() + " has no location");
            }
        }
        for (InterfaceVariable v : iface.stage(ShaderStage.FRAGMENT).map(SpirvReflection::outputs).orElse(List.of())) {
            if (v.location() < 0) {
                problems.add("fragment output " + v.name() + " has no location");
            }
        }
        return problems;
    }

    private static Optional<String> descriptorProblem(Descriptor d, PipelineShape shape) {
        String name = "descriptor " + d.name();
        if (!d.decorated()) {
            return Optional.of(name + " has no set/binding decoration");
        }
        if (d.arraySize() != 1) {
            return Optional.of(name + " is an array of descriptors");
        }
        return switch (d.type()) {
            case UNIFORM_BUFFER -> Optional.empty();
            case SAMPLED_IMAGE -> switch (d.dim()) {
                case D2, CUBE, RECT -> d.multisampled() ? Optional.of(name + " is multisampled") : Optional.empty();
                case BUFFER -> {
                    BindGroupLayout.UniformDescription host = shape.hostUniforms().get(d.name());
                    yield host != null && host.type() == UniformType.TEXEL_BUFFER ? Optional.empty()
                        : Optional.of(name + " is a texel buffer the host does not declare");
                }
                default -> Optional.of(name + " is a " + d.dim() + " texture");
            };
            default -> Optional.of(name + " is a " + d.type() + " descriptor");
        };
    }

    /**
     * Mojang's checks of the vertex-to-fragment interface: no struct, 64-bit or Component-decorated
     * variables, and every fragment input location fed by a vertex output of the same type, size and
     * interpolation.
     */
    static List<String> stageInterfaceProblems(ProgramInterface iface) {
        List<String> problems = new ArrayList<>();
        List<InterfaceVariable> outputs = iface.stage(ShaderStage.VERTEX).map(SpirvReflection::outputs).orElse(List.of());
        List<InterfaceVariable> inputs = iface.stage(ShaderStage.FRAGMENT).map(SpirvReflection::inputs).orElse(List.of());
        Map<Integer, InterfaceVariable> outputSlots = slots(outputs, "vertex output", problems);
        Map<Integer, InterfaceVariable> inputSlots = slots(inputs, "fragment input", problems);
        for (Map.Entry<Integer, InterfaceVariable> e : inputSlots.entrySet()) {
            InterfaceVariable in = e.getValue();
            InterfaceVariable out = outputSlots.get(e.getKey());
            if (out == null) {
                problems.add("fragment input " + in.name() + " (location " + e.getKey() + ") has no vertex output");
            } else if (out.scalar() != in.scalar() || out.vectorSize() != in.vectorSize() || out.flat() != in.flat()) {
                problems.add("vertex output " + out.name() + " and fragment input " + in.name() + " differ at location " + e.getKey());
            }
        }
        return problems;
    }

    private static Map<Integer, InterfaceVariable> slots(List<InterfaceVariable> vars, String what, List<String> problems) {
        Map<Integer, InterfaceVariable> out = new HashMap<>();
        for (InterfaceVariable v : vars) {
            if (v.struct() || v.scalar() == ScalarClass.OTHER) {
                problems.add(what + " " + v.name() + " has a type renderpearl does not support (struct, block, bool or 64-bit)");
            } else if (v.component()) {
                problems.add(what + " " + v.name() + " has a Component decoration");
            } else if (v.location() < 0) {
                problems.add(what + " " + v.name() + " has no location");
            } else {
                for (int i = 0; i < v.locationCount(); i++) {
                    out.put(v.location() + i, v);
                }
            }
        }
        return out;
    }
}
