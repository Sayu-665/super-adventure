package dev.shaderbridge.render.pipeline;

import com.mojang.renderpearl.api.GpuFormat;
import com.mojang.renderpearl.api.vertex.VertexFormat;
import com.mojang.renderpearl.api.vertex.VertexFormatElement;
import dev.shaderbridge.render.pipeline.SpirvReflection.InterfaceVariable;
import dev.shaderbridge.render.pipeline.SpirvReflection.ScalarClass;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

/**
 * The vertex attribute rules of Mojang's pipeline builder, checked up front so that a mismatch
 * becomes a diagnostic instead of a failed compile: every vertex shader input needs a vertex
 * format element of the same name, of the same numeric class (normalized and float formats feed
 * float inputs) and with at least as many components; at most 16 elements may be bound.
 */
public final class VertexInputCheck {
    /** Mojang's limit on bound vertex attributes. */
    public static final int MAX_ATTRIBUTES = 16;

    private VertexInputCheck() {
    }

    /**
     * @param bindings vertex format per buffer slot (nulls allowed)
     * @param inputs   the vertex shader's inputs
     * @return problems, empty if the formats feed every input
     */
    public static List<String> check(List<VertexFormat> bindings, List<InterfaceVariable> inputs) {
        List<String> problems = new ArrayList<>();
        Map<String, InterfaceVariable> byName = new HashMap<>();
        inputs.forEach(i -> byName.put(i.name(), i));
        Map<Integer, GpuFormat> formats = new HashMap<>();
        int elements = 0;
        for (VertexFormat format : bindings) {
            if (format == null) {
                continue;
            }
            String previous = null;
            int previousLocation = 0;
            for (VertexFormatElement element : format.getElements()) {
                elements++;
                InterfaceVariable input = byName.get(element.name());
                if (input == null) {
                    continue;
                }
                int location = element.name().equals(previous) ? previousLocation + 1 : input.location();
                formats.put(location, element.format());
                previous = element.name();
                previousLocation = location;
            }
        }
        if (elements > MAX_ATTRIBUTES) {
            problems.add(elements + " vertex attributes exceed Mojang's limit of " + MAX_ATTRIBUTES);
        }
        for (InterfaceVariable input : inputs) {
            if (input.component()) {
                problems.add("vertex input " + input.name() + " has a Component decoration");
                continue;
            }
            GpuFormat format = formats.get(input.location());
            if (format == null) {
                problems.add("vertex input " + input.name() + " has no matching vertex format element");
                continue;
            }
            ScalarClass expected = attributeClass(format);
            if (expected != input.scalar()) {
                problems.add("vertex input " + input.name() + " is " + input.scalar() + " but its element is " + format);
            } else if (input.vectorSize() > format.componentCount()) {
                problems.add("vertex input " + input.name() + " needs " + input.vectorSize() + " components, its element " + format + " has "
                    + format.componentCount());
            }
        }
        return problems;
    }

    /** The shader type class a vertex element format feeds ({@link ScalarClass#OTHER} for packed formats). */
    static ScalarClass attributeClass(GpuFormat format) {
        return switch (format.componentType()) {
            case UNORM_8, SNORM_8, UNORM_16, SNORM_16, FLOAT_16, FLOAT_32 -> ScalarClass.FLOAT;
            case UINT_8, UINT_16, UINT_32 -> ScalarClass.UINT;
            case SINT_8, SINT_16, SINT_32 -> ScalarClass.INT;
            default -> ScalarClass.OTHER;
        };
    }
}
