package dev.shaderbridge.render.pipeline;

import dev.shaderbridge.render.pipeline.SpirvReflection.Descriptor;
import dev.shaderbridge.render.pipeline.SpirvReflection.DescriptorType;
import dev.shaderbridge.render.pipeline.SpirvReflection.ImageDim;
import dev.shaderbridge.render.pipeline.SpirvReflection.InterfaceVariable;
import dev.shaderbridge.render.pipeline.SpirvReflection.ScalarClass;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;

/**
 * Reads the resource interface of a SPIR-V module, with the naming rules SPIRV-Cross (and through
 * it Mojang's pipeline builder) applies. Pure Java; the module is not modified.
 */
public final class SpirvReflector {
    /** First word of every SPIR-V module. */
    public static final int MAGIC = 0x07230203;

    private static final int OP_NAME = 5;
    private static final int OP_ENTRY_POINT = 15;
    private static final int OP_TYPE_INT = 21;
    private static final int OP_TYPE_FLOAT = 22;
    private static final int OP_TYPE_VECTOR = 23;
    private static final int OP_TYPE_MATRIX = 24;
    private static final int OP_TYPE_IMAGE = 25;
    private static final int OP_TYPE_SAMPLER = 26;
    private static final int OP_TYPE_SAMPLED_IMAGE = 27;
    private static final int OP_TYPE_ARRAY = 28;
    private static final int OP_TYPE_RUNTIME_ARRAY = 29;
    private static final int OP_TYPE_STRUCT = 30;
    private static final int OP_TYPE_POINTER = 32;
    private static final int OP_CONSTANT = 43;
    private static final int OP_SPEC_CONSTANT = 50;
    private static final int OP_VARIABLE = 59;
    private static final int OP_DECORATE = 71;
    private static final int OP_MEMBER_DECORATE = 72;

    private static final int DECORATION_BLOCK = 2;
    private static final int DECORATION_BUFFER_BLOCK = 3;
    private static final int DECORATION_BUILT_IN = 11;
    private static final int DECORATION_FLAT = 14;
    private static final int DECORATION_LOCATION = 30;
    private static final int DECORATION_COMPONENT = 31;
    private static final int DECORATION_BINDING = 33;
    private static final int DECORATION_DESCRIPTOR_SET = 34;

    private static final int STORAGE_UNIFORM_CONSTANT = 0;
    private static final int STORAGE_INPUT = 1;
    private static final int STORAGE_UNIFORM = 2;
    private static final int STORAGE_OUTPUT = 3;
    private static final int STORAGE_PUSH_CONSTANT = 9;
    private static final int STORAGE_STORAGE_BUFFER = 12;

    /** SPIR-V 1.4: entry point interfaces list every referenced global. */
    private static final int VERSION_1_4 = 0x10400;

    private final Words words;
    private final Map<Integer, String> names = new HashMap<>();
    private final Map<Integer, Map<Integer, Integer>> decorations = new HashMap<>();
    private final Set<Integer> structsWithBuiltIns = new HashSet<>();
    private final Map<Integer, int[]> types = new HashMap<>();
    private final Map<Integer, Integer> constants = new HashMap<>();
    private final List<int[]> variables = new ArrayList<>();
    private final Set<Integer> entryInterface = new HashSet<>();
    private int executionModel = -1;

    private SpirvReflector(ByteBuffer module) {
        if (module.remaining() % 4 != 0 || module.remaining() < 20) {
            throw new InvalidSpirvException("a SPIR-V module has a whole number of words and a 5-word header, got "
                + module.remaining() + " bytes");
        }
        ByteBuffer le = module.duplicate().order(ByteOrder.LITTLE_ENDIAN);
        this.words = new Words(le, module.position(), module.remaining() / 4);
    }

    /**
     * @param module a SPIR-V module, little-endian words from its position to its limit
     * @return what the module declares
     * @throws InvalidSpirvException if the module is malformed
     */
    public static SpirvReflection reflect(ByteBuffer module) {
        return new SpirvReflector(module).run();
    }

    private SpirvReflection run() {
        if (words.get(0) != MAGIC) {
            throw new InvalidSpirvException(String.format("bad SPIR-V magic 0x%08x", words.get(0)));
        }
        int version = words.get(1);
        scan();
        boolean filterByInterface = version >= VERSION_1_4;
        List<Descriptor> descriptors = new ArrayList<>();
        List<InterfaceVariable> inputs = new ArrayList<>();
        List<InterfaceVariable> outputs = new ArrayList<>();
        int pushConstants = 0;
        for (int[] v : variables) {
            int id = v[1];
            int storage = v[2];
            if (filterByInterface && !entryInterface.contains(id)) {
                continue;
            }
            int pointee = pointee(v[0]);
            switch (storage) {
                case STORAGE_UNIFORM_CONSTANT, STORAGE_UNIFORM, STORAGE_STORAGE_BUFFER -> descriptors.add(descriptor(id, storage, pointee));
                case STORAGE_INPUT, STORAGE_OUTPUT -> {
                    if (!isBuiltIn(id, pointee)) {
                        (storage == STORAGE_INPUT ? inputs : outputs).add(interfaceVariable(id, pointee));
                    }
                }
                case STORAGE_PUSH_CONSTANT -> pushConstants++;
                default -> {
                }
            }
        }
        return new SpirvReflection(SpirvReflection.Stage.of(executionModel), descriptors, inputs, outputs, pushConstants);
    }

    private void scan() {
        int i = 5;
        while (i < words.size()) {
            int word = words.get(i);
            int count = word >>> 16;
            int op = word & 0xFFFF;
            if (count == 0 || i + count > words.size()) {
                throw new InvalidSpirvException("truncated instruction (opcode " + op + ") at word " + i);
            }
            instruction(op, i + 1, count - 1);
            i += count;
        }
    }

    private void instruction(int op, int at, int operands) {
        switch (op) {
            case OP_NAME -> names.put(words.get(at), string(at + 1, at + operands));
            case OP_ENTRY_POINT -> entryPoint(at, operands);
            case OP_DECORATE -> {
                if (operands >= 2) {
                    decorations.computeIfAbsent(words.get(at), k -> new HashMap<>())
                        .put(words.get(at + 1), operands >= 3 ? words.get(at + 2) : 0);
                }
            }
            case OP_MEMBER_DECORATE -> {
                if (operands >= 3 && words.get(at + 2) == DECORATION_BUILT_IN) {
                    structsWithBuiltIns.add(words.get(at));
                }
            }
            case OP_TYPE_INT, OP_TYPE_FLOAT, OP_TYPE_VECTOR, OP_TYPE_MATRIX, OP_TYPE_IMAGE, OP_TYPE_SAMPLER, OP_TYPE_SAMPLED_IMAGE,
                 OP_TYPE_ARRAY, OP_TYPE_RUNTIME_ARRAY, OP_TYPE_STRUCT, OP_TYPE_POINTER -> {
                int[] type = new int[operands];
                type[0] = op;
                for (int k = 1; k < operands; k++) {
                    type[k] = words.get(at + k);
                }
                types.put(words.get(at), type);
            }
            case OP_CONSTANT, OP_SPEC_CONSTANT -> {
                if (operands >= 3) {
                    constants.put(words.get(at + 1), words.get(at + 2));
                }
            }
            case OP_VARIABLE -> {
                if (operands >= 3) {
                    variables.add(new int[] {words.get(at), words.get(at + 1), words.get(at + 2)});
                }
            }
            default -> {
            }
        }
    }

    private void entryPoint(int at, int operands) {
        int end = at + operands;
        int nameStart = at + 2;
        int nameWords = stringWords(nameStart, end);
        if (executionModel < 0) {
            executionModel = words.get(at);
        }
        for (int k = nameStart + nameWords; k < end; k++) {
            entryInterface.add(words.get(k));
        }
    }

    private Descriptor descriptor(int variable, int storage, int pointee) {
        int arraySize = 1;
        int type = pointee;
        while (isOp(type, OP_TYPE_ARRAY) || isOp(type, OP_TYPE_RUNTIME_ARRAY)) {
            int[] t = types.get(type);
            arraySize = t[0] == OP_TYPE_RUNTIME_ARRAY ? 0 : arraySize * constants.getOrDefault(t[2], 0);
            type = t[1];
        }
        boolean decorated = decoration(variable, DECORATION_DESCRIPTOR_SET) != null && decoration(variable, DECORATION_BINDING) != null;
        int[] t = types.get(type);
        if (t == null) {
            return new Descriptor(nameOf(variable), DescriptorType.OTHER, ImageDim.NONE, false, false, arraySize, ScalarClass.OTHER, decorated);
        }
        return switch (t[0]) {
            case OP_TYPE_STRUCT -> {
                boolean storageBlock = storage == STORAGE_STORAGE_BUFFER || decoration(type, DECORATION_BUFFER_BLOCK) != null;
                DescriptorType kind = storageBlock ? DescriptorType.STORAGE_BUFFER
                    : decoration(type, DECORATION_BLOCK) != null ? DescriptorType.UNIFORM_BUFFER : DescriptorType.OTHER;
                yield new Descriptor(blockName(variable, type), kind, ImageDim.NONE, false, false, arraySize, ScalarClass.OTHER, decorated);
            }
            case OP_TYPE_SAMPLED_IMAGE -> image(variable, types.get(t[1]), DescriptorType.SAMPLED_IMAGE, arraySize, decorated);
            case OP_TYPE_IMAGE -> {
                ImageDim dim = ImageDim.of(t[2]);
                DescriptorType kind = dim == ImageDim.SUBPASS_DATA ? DescriptorType.OTHER
                    : t[6] == 2 ? DescriptorType.STORAGE_IMAGE : DescriptorType.SEPARATE_IMAGE;
                yield image(variable, t, kind, arraySize, decorated);
            }
            case OP_TYPE_SAMPLER ->
                new Descriptor(nameOf(variable), DescriptorType.SEPARATE_SAMPLER, ImageDim.NONE, false, false, arraySize, ScalarClass.OTHER, decorated);
            default -> new Descriptor(nameOf(variable), DescriptorType.OTHER, ImageDim.NONE, false, false, arraySize, ScalarClass.OTHER, decorated);
        };
    }

    /** {@code image} is an OpTypeImage: sampled type, dim, depth, arrayed, MS, sampled, format. */
    private Descriptor image(int variable, int[] image, DescriptorType kind, int arraySize, boolean decorated) {
        if (image == null || image[0] != OP_TYPE_IMAGE || image.length < 7) {
            return new Descriptor(nameOf(variable), DescriptorType.OTHER, ImageDim.NONE, false, false, arraySize, ScalarClass.OTHER, decorated);
        }
        return new Descriptor(nameOf(variable), kind, ImageDim.of(image[2]), image[4] != 0, image[5] != 0, arraySize, scalarClass(image[1]),
            decorated);
    }

    private InterfaceVariable interfaceVariable(int variable, int pointee) {
        int locations = 1;
        int type = pointee;
        while (isOp(type, OP_TYPE_ARRAY)) {
            int[] t = types.get(type);
            locations *= Math.max(1, constants.getOrDefault(t[2], 1));
            type = t[1];
        }
        int[] t = types.get(type);
        ScalarClass scalar;
        int vectorSize;
        boolean struct = t != null && t[0] == OP_TYPE_STRUCT;
        if (t != null && t[0] == OP_TYPE_MATRIX) {
            locations *= t[2];
            int[] column = types.get(t[1]);
            scalar = column == null ? ScalarClass.OTHER : scalarClass(column[1]);
            vectorSize = column == null ? 0 : column[2];
        } else if (t != null && t[0] == OP_TYPE_VECTOR) {
            scalar = scalarClass(t[1]);
            vectorSize = t[2];
        } else {
            scalar = struct ? ScalarClass.OTHER : scalarClass(type);
            vectorSize = 1;
        }
        Integer location = decoration(variable, DECORATION_LOCATION);
        return new InterfaceVariable(nameOf(variable), location == null ? -1 : location, scalar, vectorSize, locations,
            decoration(variable, DECORATION_FLAT) != null, decoration(variable, DECORATION_COMPONENT) != null, struct);
    }

    private ScalarClass scalarClass(int type) {
        int[] t = types.get(type);
        if (t == null || t.length < 2) {
            return ScalarClass.OTHER;
        }
        if (t[0] == OP_TYPE_FLOAT) {
            return t[1] == 32 ? ScalarClass.FLOAT : ScalarClass.OTHER;
        }
        if (t[0] == OP_TYPE_INT && t.length >= 3 && t[1] == 32) {
            return t[2] != 0 ? ScalarClass.INT : ScalarClass.UINT;
        }
        return ScalarClass.OTHER;
    }

    private boolean isBuiltIn(int variable, int pointee) {
        if (decoration(variable, DECORATION_BUILT_IN) != null) {
            return true;
        }
        int type = pointee;
        while (isOp(type, OP_TYPE_ARRAY) || isOp(type, OP_TYPE_RUNTIME_ARRAY)) {
            type = types.get(type)[1];
        }
        return structsWithBuiltIns.contains(type);
    }

    /** SPIRV-Cross: the block type name, else the variable name, else {@code _<type>_<variable>}. */
    private String blockName(int variable, int structType) {
        String type = names.getOrDefault(structType, "");
        if (!type.isEmpty()) {
            return type;
        }
        String var = names.getOrDefault(variable, "");
        return var.isEmpty() ? "_" + structType + "_" + variable : var;
    }

    private String nameOf(int id) {
        return names.getOrDefault(id, "_" + id);
    }

    private int pointee(int pointerType) {
        int[] t = types.get(pointerType);
        if (t == null || t[0] != OP_TYPE_POINTER || t.length < 3) {
            throw new InvalidSpirvException("variable type %" + pointerType + " is not a pointer type");
        }
        return t[2];
    }

    private boolean isOp(int type, int op) {
        int[] t = types.get(type);
        return t != null && t[0] == op;
    }

    private Integer decoration(int id, int decoration) {
        Map<Integer, Integer> d = decorations.get(id);
        return d == null ? null : d.get(decoration);
    }

    /** A nul-terminated UTF-8 literal in words {@code [from, end)}. */
    private String string(int from, int end) {
        byte[] bytes = new byte[Math.max(0, end - from) * 4];
        int length = 0;
        outer:
        for (int k = from; k < end; k++) {
            int w = words.get(k);
            for (int b = 0; b < 4; b++) {
                byte c = (byte) (w >>> (8 * b));
                if (c == 0) {
                    break outer;
                }
                bytes[length++] = c;
            }
        }
        return new String(bytes, 0, length, StandardCharsets.UTF_8);
    }

    /** Words the nul-terminated literal starting at {@code from} occupies. */
    private int stringWords(int from, int end) {
        for (int k = from; k < end; k++) {
            int w = words.get(k);
            if ((w & 0xFF) == 0 || (w & 0xFF00) == 0 || (w & 0xFF0000) == 0 || (w & 0xFF000000) == 0) {
                return k - from + 1;
            }
        }
        throw new InvalidSpirvException("unterminated string literal");
    }

    /** Little-endian word view of a byte buffer region. */
    private record Words(ByteBuffer bytes, int offset, int size) {
        int get(int word) {
            return bytes.getInt(offset + word * 4);
        }
    }

    /** A SPIR-V module that cannot be read. */
    public static final class InvalidSpirvException extends RuntimeException {
        private static final long serialVersionUID = 1L;

        InvalidSpirvException(String message) {
            super(message);
        }
    }
}
