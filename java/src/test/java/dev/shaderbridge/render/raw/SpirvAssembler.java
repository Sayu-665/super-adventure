package dev.shaderbridge.render.raw;

import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;

/** A minimal SPIR-V assembler for hand-built test modules (opcodes and operands as numbers). */
final class SpirvAssembler {
    static final int OP_NAME = 5;
    static final int OP_ENTRY_POINT = 15;
    static final int OP_CAPABILITY = 17;
    static final int OP_TYPE_INT = 21;
    static final int OP_TYPE_FLOAT = 22;
    static final int OP_TYPE_VECTOR = 23;
    static final int OP_TYPE_MATRIX = 24;
    static final int OP_TYPE_IMAGE = 25;
    static final int OP_TYPE_SAMPLED_IMAGE = 27;
    static final int OP_TYPE_ARRAY = 28;
    static final int OP_TYPE_RUNTIME_ARRAY = 29;
    static final int OP_TYPE_STRUCT = 30;
    static final int OP_TYPE_POINTER = 32;
    static final int OP_CONSTANT = 43;
    static final int OP_VARIABLE = 59;
    static final int OP_DECORATE = 71;
    static final int OP_MEMBER_DECORATE = 72;

    static final int UNIFORM_CONSTANT = 0;
    static final int UNIFORM = 2;
    static final int STORAGE_BUFFER = 12;

    private final List<Integer> words = new ArrayList<>();
    private int bound = 1;

    int id() {
        return bound++;
    }

    SpirvAssembler op(int opcode, int... operands) {
        words.add((operands.length + 1) << 16 | opcode);
        for (int o : operands) {
            words.add(o);
        }
        return this;
    }

    void entryPoint(int executionModel, int function, String name, int... iface) {
        byte[] bytes = name.getBytes(StandardCharsets.UTF_8);
        int[] packed = new int[bytes.length / 4 + 1];
        for (int i = 0; i < bytes.length; i++) {
            packed[i / 4] |= (bytes[i] & 0xFF) << (8 * (i % 4));
        }
        int[] all = new int[2 + packed.length + iface.length];
        all[0] = executionModel;
        all[1] = function;
        System.arraycopy(packed, 0, all, 2, packed.length);
        System.arraycopy(iface, 0, all, 2 + packed.length, iface.length);
        op(OP_ENTRY_POINT, all);
    }

    int pointer(int storage, int type) {
        int id = id();
        op(OP_TYPE_POINTER, id, storage, type);
        return id;
    }

    int variable(int pointerType, int storage) {
        int id = id();
        op(OP_VARIABLE, pointerType, id, storage);
        return id;
    }

    int constant(int type, int value) {
        int id = id();
        op(OP_CONSTANT, type, id, value);
        return id;
    }

    void binding(int variable, int set, int binding) {
        op(OP_DECORATE, variable, 34, set);
        op(OP_DECORATE, variable, 33, binding);
    }

    /** @return the module (SPIR-V 1.5), little-endian */
    ByteBuffer build() {
        ByteBuffer out = ByteBuffer.allocate((5 + words.size()) * 4).order(ByteOrder.LITTLE_ENDIAN);
        out.putInt(0x07230203).putInt(0x10500).putInt(0).putInt(bound).putInt(0);
        words.forEach(out::putInt);
        return out.flip();
    }
}
