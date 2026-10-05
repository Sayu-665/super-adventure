package dev.shaderbridge.render.raw;

import dev.shaderbridge.render.pipeline.SpirvReflector;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.util.HashMap;
import java.util.Map;

/**
 * The static size of every storage buffer block a SPIR-V module declares: the end of its last
 * member by the explicit layout ({@code Offset}, {@code ArrayStride}, {@code MatrixStride}), a
 * trailing runtime array counting as empty. Sizes are rounded up where the layout leaves room
 * (a matrix counts whole strides), which only ever enlarges the buffers created from them.
 */
public final class SpirvBlockSizes {
    private static final int OP_TYPE_INT = 21;
    private static final int OP_TYPE_FLOAT = 22;
    private static final int OP_TYPE_VECTOR = 23;
    private static final int OP_TYPE_MATRIX = 24;
    private static final int OP_TYPE_ARRAY = 28;
    private static final int OP_TYPE_RUNTIME_ARRAY = 29;
    private static final int OP_TYPE_STRUCT = 30;
    private static final int OP_TYPE_POINTER = 32;
    private static final int OP_CONSTANT = 43;
    private static final int OP_VARIABLE = 59;
    private static final int OP_DECORATE = 71;
    private static final int OP_MEMBER_DECORATE = 72;

    private static final int DECORATION_BUFFER_BLOCK = 3;
    private static final int DECORATION_ROW_MAJOR = 4;
    private static final int DECORATION_ARRAY_STRIDE = 6;
    private static final int DECORATION_MATRIX_STRIDE = 7;
    private static final int DECORATION_BINDING = 33;
    private static final int DECORATION_DESCRIPTOR_SET = 34;
    private static final int DECORATION_OFFSET = 35;

    private static final int STORAGE_UNIFORM = 2;
    private static final int STORAGE_STORAGE_BUFFER = 12;

    private final Map<Integer, int[]> types = new HashMap<>();
    private final Map<Integer, Long> constants = new HashMap<>();
    private final Map<Integer, Map<Integer, Integer>> decorations = new HashMap<>();
    /** Struct id, then member index, then decoration and its literal. */
    private final Map<Integer, Map<Integer, Map<Integer, Integer>>> memberDecorations = new HashMap<>();
    private final Map<Integer, int[]> variables = new HashMap<>();

    private SpirvBlockSizes() {
    }

    /**
     * A descriptor location.
     *
     * @param set     descriptor set
     * @param binding binding
     */
    public record Location(int set, int binding) {
    }

    /**
     * @param module a SPIR-V module, little-endian words from its position to its limit
     * @return the static size in bytes of each storage buffer block, by its descriptor location
     * @throws SpirvReflector.InvalidSpirvException if the module is malformed
     */
    public static Map<Location, Long> storageBlocks(ByteBuffer module) {
        SpirvBlockSizes s = new SpirvBlockSizes();
        s.scan(module);
        return s.blocks();
    }

    private void scan(ByteBuffer module) {
        ByteBuffer le = module.duplicate().order(ByteOrder.LITTLE_ENDIAN);
        int base = le.position();
        int words = le.remaining() / 4;
        if (words < 5 || le.getInt(base) != SpirvReflector.MAGIC) {
            throw new SpirvReflector.InvalidSpirvException("not a SPIR-V module");
        }
        int i = 5;
        while (i < words) {
            int word = le.getInt(base + i * 4);
            int count = word >>> 16;
            if (count == 0 || i + count > words) {
                throw new SpirvReflector.InvalidSpirvException("truncated instruction at word " + i);
            }
            int[] operands = new int[count - 1];
            for (int k = 0; k < operands.length; k++) {
                operands[k] = le.getInt(base + (i + 1 + k) * 4);
            }
            instruction(word & 0xFFFF, operands);
            i += count;
        }
    }

    private void instruction(int op, int[] o) {
        switch (op) {
            case OP_TYPE_INT, OP_TYPE_FLOAT, OP_TYPE_VECTOR, OP_TYPE_MATRIX, OP_TYPE_ARRAY, OP_TYPE_RUNTIME_ARRAY, OP_TYPE_STRUCT, OP_TYPE_POINTER -> {
                if (o.length >= 1) {
                    int[] type = new int[o.length];
                    type[0] = op;
                    System.arraycopy(o, 1, type, 1, o.length - 1);
                    types.put(o[0], type);
                }
            }
            case OP_CONSTANT -> {
                if (o.length >= 3) {
                    constants.put(o[1], Integer.toUnsignedLong(o[2]));
                }
            }
            case OP_VARIABLE -> {
                if (o.length >= 3) {
                    variables.put(o[1], new int[] {o[0], o[2]});
                }
            }
            case OP_DECORATE -> {
                if (o.length >= 2) {
                    decorations.computeIfAbsent(o[0], k -> new HashMap<>()).put(o[1], o.length >= 3 ? o[2] : 0);
                }
            }
            case OP_MEMBER_DECORATE -> {
                if (o.length >= 3) {
                    memberDecorations.computeIfAbsent(o[0], k -> new HashMap<>()).computeIfAbsent(o[1], k -> new HashMap<>())
                        .put(o[2], o.length >= 4 ? o[3] : 0);
                }
            }
            default -> {
            }
        }
    }

    private Map<Location, Long> blocks() {
        Map<Location, Long> out = new HashMap<>();
        variables.forEach((id, v) -> {
            int[] pointer = types.get(v[0]);
            Integer set = decoration(id, DECORATION_DESCRIPTOR_SET);
            Integer binding = decoration(id, DECORATION_BINDING);
            if (pointer == null || pointer[0] != OP_TYPE_POINTER || pointer.length < 3 || set == null || binding == null) {
                return;
            }
            int type = pointer[2];
            while (isOp(type, OP_TYPE_ARRAY) || isOp(type, OP_TYPE_RUNTIME_ARRAY)) {
                type = types.get(type)[1];
            }
            boolean storage = v[1] == STORAGE_STORAGE_BUFFER || v[1] == STORAGE_UNIFORM && decoration(type, DECORATION_BUFFER_BLOCK) != null;
            if (storage && isOp(type, OP_TYPE_STRUCT)) {
                out.merge(new Location(set, binding), size(type, Map.of(), 0), Math::max);
            }
        });
        return out;
    }

    /**
     * @param type   a type id
     * @param member the decorations of the struct member holding it (matrix layout)
     * @param depth  nesting depth, bounding malformed recursive types
     * @return its size in bytes under the explicit layout
     */
    private long size(int type, Map<Integer, Integer> member, int depth) {
        int[] t = types.get(type);
        if (t == null || depth > 32) {
            return 0;
        }
        return switch (t[0]) {
            case OP_TYPE_INT, OP_TYPE_FLOAT -> t.length >= 2 ? Math.max(1, t[1] / 8) : 0;
            case OP_TYPE_VECTOR -> t.length >= 3 ? t[2] * size(t[1], member, depth + 1) : 0;
            case OP_TYPE_MATRIX -> matrix(t, member, depth);
            case OP_TYPE_ARRAY -> {
                Integer stride = decoration(type, DECORATION_ARRAY_STRIDE);
                long length = t.length >= 3 ? constants.getOrDefault(t[2], 0L) : 0;
                yield length * (stride != null ? stride : size(t[1], member, depth + 1));
            }
            case OP_TYPE_STRUCT -> struct(type, t, depth);
            default -> 0;
        };
    }

    private long matrix(int[] t, Map<Integer, Integer> member, int depth) {
        if (t.length < 3) {
            return 0;
        }
        int[] column = types.get(t[1]);
        int rows = column != null && column.length >= 3 ? column[2] : 4;
        int lines = member.containsKey(DECORATION_ROW_MAJOR) ? rows : t[2];
        Integer stride = member.get(DECORATION_MATRIX_STRIDE);
        return (long) lines * (stride != null ? stride : size(t[1], member, depth + 1));
    }

    private long struct(int type, int[] t, int depth) {
        Map<Integer, Map<Integer, Integer>> members = memberDecorations.getOrDefault(type, Map.of());
        long end = 0;
        for (int m = 0; m + 1 < t.length; m++) {
            int memberType = t[m + 1];
            Map<Integer, Integer> decorated = members.getOrDefault(m, Map.of());
            Integer offset = decorated.get(DECORATION_OFFSET);
            long size = isOp(memberType, OP_TYPE_RUNTIME_ARRAY) ? 0 : size(memberType, decorated, depth + 1);
            end = Math.max(end, (offset != null ? Integer.toUnsignedLong(offset) : end) + size);
        }
        return end;
    }

    private boolean isOp(int type, int op) {
        int[] t = types.get(type);
        return t != null && t[0] == op;
    }

    private Integer decoration(int id, int decoration) {
        Map<Integer, Integer> d = decorations.get(id);
        return d == null ? null : d.get(decoration);
    }
}
