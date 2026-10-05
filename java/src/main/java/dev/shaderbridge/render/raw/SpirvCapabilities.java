package dev.shaderbridge.render.raw;

import dev.shaderbridge.model.ShaderStage;
import dev.shaderbridge.render.pipeline.SpirvReflector;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeSet;

/**
 * The SPIR-V capabilities a module declares, and which of them Minecraft's device can execute. A
 * pipeline whose shaders declare a capability the device did not enable is invalid (undefined
 * behaviour, often a driver crash), so such programs are rejected before any pipeline is made.
 * Capability numbers and their Vulkan requirements follow the SPIR-V specification and the
 * Vulkan specification's "SPIR-V Environment" appendix.
 */
public final class SpirvCapabilities {
    private static final int OP_CAPABILITY = 17;

    /** Capabilities every Vulkan 1.2 device supports without enabling a feature. */
    private static final Set<Integer> CORE = Set.of(
        0, // Matrix
        1, // Shader
        40, // InputAttachment
        43, // Sampled1D
        44, // Image1D
        46, // SampledBuffer
        47, // ImageBuffer
        50, // ImageQuery
        51, // DerivativeControl
        52, // InterpolationFunction
        4427, // DrawParameters (Minecraft enables shaderDrawParameters)
        4437, // DeviceGroup
        5301 // ShaderNonUniform
    );

    /** Capabilities that need one of ShaderBridge's {@link RawFeature}s. */
    private static final Map<Integer, RawFeature> FEATURES = Map.ofEntries(
        Map.entry(2, RawFeature.GEOMETRY_SHADER),
        Map.entry(3, RawFeature.TESSELLATION_SHADER),
        Map.entry(10, RawFeature.FLOAT64),
        Map.entry(11, RawFeature.INT64),
        Map.entry(22, RawFeature.INT16),
        Map.entry(23, RawFeature.TESSELLATION_AND_GEOMETRY_POINT_SIZE),
        Map.entry(24, RawFeature.TESSELLATION_AND_GEOMETRY_POINT_SIZE),
        Map.entry(25, RawFeature.IMAGE_GATHER_EXTENDED),
        Map.entry(28, RawFeature.UNIFORM_BUFFER_ARRAY_DYNAMIC_INDEXING),
        Map.entry(29, RawFeature.SAMPLED_IMAGE_ARRAY_DYNAMIC_INDEXING),
        Map.entry(30, RawFeature.STORAGE_BUFFER_ARRAY_DYNAMIC_INDEXING),
        Map.entry(31, RawFeature.STORAGE_IMAGE_ARRAY_DYNAMIC_INDEXING),
        Map.entry(32, RawFeature.CLIP_DISTANCE),
        Map.entry(33, RawFeature.CULL_DISTANCE),
        Map.entry(34, RawFeature.IMAGE_CUBE_ARRAY),
        Map.entry(35, RawFeature.SAMPLE_RATE_SHADING),
        Map.entry(45, RawFeature.IMAGE_CUBE_ARRAY),
        Map.entry(49, RawFeature.STORAGE_IMAGE_EXTENDED_FORMATS),
        Map.entry(55, RawFeature.STORAGE_IMAGE_READ_WITHOUT_FORMAT),
        Map.entry(56, RawFeature.STORAGE_IMAGE_WRITE_WITHOUT_FORMAT)
    );

    /**
     * Subgroup capabilities ({@code GroupNonUniform*}) and the
     * {@code VkSubgroupFeatureFlagBits} each needs.
     */
    private static final Map<Integer, Integer> SUBGROUP = Map.of(
        61, 0x1, // GroupNonUniform: BASIC
        62, 0x2, // GroupNonUniformVote: VOTE
        63, 0x4, // GroupNonUniformArithmetic: ARITHMETIC
        64, 0x8, // GroupNonUniformBallot: BALLOT
        65, 0x10, // GroupNonUniformShuffle: SHUFFLE
        66, 0x20, // GroupNonUniformShuffleRelative: SHUFFLE_RELATIVE
        67, 0x40, // GroupNonUniformClustered: CLUSTERED
        68, 0x80 // GroupNonUniformQuad: QUAD
    );

    private SpirvCapabilities() {
    }

    /**
     * @param module a SPIR-V module, little-endian words from its position to its limit
     * @return the capabilities it declares, in ascending order
     * @throws SpirvReflector.InvalidSpirvException if the module is malformed
     */
    public static Set<Integer> read(ByteBuffer module) {
        ByteBuffer le = module.duplicate().order(ByteOrder.LITTLE_ENDIAN);
        int words = le.remaining() / 4;
        int base = le.position();
        if (words < 5 || le.getInt(base) != SpirvReflector.MAGIC) {
            throw new SpirvReflector.InvalidSpirvException("not a SPIR-V module");
        }
        Set<Integer> out = new TreeSet<>();
        int i = 5;
        while (i < words) {
            int word = le.getInt(base + i * 4);
            int count = word >>> 16;
            if (count == 0 || i + count > words) {
                throw new SpirvReflector.InvalidSpirvException("truncated instruction at word " + i);
            }
            if ((word & 0xFFFF) == OP_CAPABILITY && count >= 2) {
                out.add(le.getInt(base + (i + 1) * 4));
            }
            i += count;
        }
        return out;
    }

    /**
     * @param capabilities the capabilities of one stage's module
     * @param stage        the stage
     * @param device       what the device enabled
     * @return why the device cannot run the module, by ascending capability, empty if it can
     */
    public static List<String> problems(Set<Integer> capabilities, ShaderStage stage, EnabledFeatures device) {
        List<String> problems = new ArrayList<>();
        for (int capability : new TreeSet<>(capabilities)) {
            if (CORE.contains(capability)) {
                continue;
            }
            RawFeature feature = FEATURES.get(capability);
            Integer subgroupBit = SUBGROUP.get(capability);
            if (feature != null) {
                if (!device.has(feature)) {
                    problems.add(stageName(stage) + " needs the device feature " + feature.vkName() + " (SPIR-V capability " + capability + ")");
                }
            } else if (subgroupBit != null) {
                if ((device.subgroupStages() & StageFlags.of(stage)) == 0 || (device.subgroupOperations() & subgroupBit) == 0) {
                    problems.add(stageName(stage) + " uses subgroup operations the device does not support in that stage (SPIR-V capability "
                        + capability + ")");
                }
            } else {
                problems.add(stageName(stage) + " declares SPIR-V capability " + capability + ", which ShaderBridge does not enable");
            }
        }
        return problems;
    }

    private static String stageName(ShaderStage stage) {
        return "the " + stage.packExtension() + " stage";
    }
}
