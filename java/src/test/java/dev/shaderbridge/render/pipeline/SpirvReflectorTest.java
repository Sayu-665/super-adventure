package dev.shaderbridge.render.pipeline;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.shaderbridge.model.Program;
import dev.shaderbridge.model.ShaderStage;
import dev.shaderbridge.render.RenderFixture;
import dev.shaderbridge.render.pipeline.SpirvReflection.Descriptor;
import dev.shaderbridge.render.pipeline.SpirvReflection.DescriptorType;
import dev.shaderbridge.render.pipeline.SpirvReflection.ImageDim;
import dev.shaderbridge.render.pipeline.SpirvReflection.InterfaceVariable;
import dev.shaderbridge.render.pipeline.SpirvReflection.ScalarClass;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.function.Function;
import java.util.stream.Collectors;
import org.junit.jupiter.api.Test;

class SpirvReflectorTest {
    /** A minimal SPIR-V assembler for the instructions the reflector reads. */
    private static final class Module {
        private final List<Integer> words = new ArrayList<>();
        private int bound = 1;

        int id() {
            return bound++;
        }

        void op(int opcode, int... operands) {
            words.add((operands.length + 1) << 16 | opcode);
            for (int o : operands) {
                words.add(o);
            }
        }

        /** An instruction with operands, a string literal, then more operands. */
        void op(int opcode, int[] before, String literal, int... after) {
            byte[] bytes = literal.getBytes(StandardCharsets.UTF_8);
            int[] packed = new int[bytes.length / 4 + 1];
            for (int i = 0; i < bytes.length; i++) {
                packed[i / 4] |= (bytes[i] & 0xFF) << (8 * (i % 4));
            }
            int[] all = new int[before.length + packed.length + after.length];
            System.arraycopy(before, 0, all, 0, before.length);
            System.arraycopy(packed, 0, all, before.length, packed.length);
            System.arraycopy(after, 0, all, before.length + packed.length, after.length);
            op(opcode, all);
        }

        void name(int target, String name) {
            op(5, new int[] {target}, name);
        }

        void decorate(int target, int decoration, int... values) {
            int[] operands = new int[values.length + 2];
            operands[0] = target;
            operands[1] = decoration;
            System.arraycopy(values, 0, operands, 2, values.length);
            op(71, operands);
        }

        int variable(int pointerType, int storage) {
            int v = id();
            op(59, pointerType, v, storage);
            return v;
        }

        int pointer(int storage, int type) {
            int p = id();
            op(32, p, storage, type);
            return p;
        }

        ByteBuffer build(int version) {
            ByteBuffer b = ByteBuffer.allocate((words.size() + 5) * 4).order(ByteOrder.LITTLE_ENDIAN);
            b.putInt(SpirvReflector.MAGIC).putInt(version).putInt(0).putInt(bound).putInt(0);
            words.forEach(b::putInt);
            return b.flip();
        }
    }

    private static final int UNIFORM_CONSTANT = 0;
    private static final int INPUT = 1;
    private static final int UNIFORM = 2;
    private static final int OUTPUT = 3;

    /**
     * A fragment shader with a named block, an anonymous block, a buffer block, a combined image
     * sampler, a sampler array, a storage image, inputs (one builtin, one flat ivec2, one outside
     * the entry point interface) and an output. With {@code listAll} the entry point lists every
     * global, as SPIR-V 1.4 requires.
     */
    private static ByteBuffer module(int version, boolean listAll) {
        Module m = new Module();
        int main = m.id();
        int f32 = m.id();
        int i32 = m.id();
        int vec4 = m.id();
        int ivec2 = m.id();
        int frameStruct = m.id();
        int anonStruct = m.id();
        int bufferStruct = m.id();
        int image2d = m.id();
        int sampled = m.id();
        int storageImage = m.id();
        int four = m.id();
        int samplerArray = m.id();
        m.op(22, f32, 32);
        m.op(21, i32, 32, 1);
        m.op(23, vec4, f32, 4);
        m.op(23, ivec2, i32, 2);
        m.op(30, frameStruct, vec4);
        m.op(30, anonStruct, vec4);
        m.op(30, bufferStruct, vec4);
        m.op(25, image2d, f32, 1, 0, 0, 0, 1, 0);
        m.op(27, sampled, image2d);
        m.op(25, storageImage, f32, 1, 0, 0, 0, 2, 4);
        m.op(43, i32, four, 4);
        m.op(28, samplerArray, sampled, four);
        int frame = m.variable(m.pointer(UNIFORM, frameStruct), UNIFORM);
        int anon = m.variable(m.pointer(UNIFORM, anonStruct), UNIFORM);
        int buffer = m.variable(m.pointer(UNIFORM, bufferStruct), UNIFORM);
        int tex = m.variable(m.pointer(UNIFORM_CONSTANT, sampled), UNIFORM_CONSTANT);
        int texArray = m.variable(m.pointer(UNIFORM_CONSTANT, samplerArray), UNIFORM_CONSTANT);
        int img = m.variable(m.pointer(UNIFORM_CONSTANT, storageImage), UNIFORM_CONSTANT);
        int color = m.variable(m.pointer(INPUT, vec4), INPUT);
        int ids = m.variable(m.pointer(INPUT, ivec2), INPUT);
        int fragCoord = m.variable(m.pointer(INPUT, vec4), INPUT);
        int unused = m.variable(m.pointer(INPUT, vec4), INPUT);
        int out = m.variable(m.pointer(OUTPUT, vec4), OUTPUT);
        List<Integer> iface = new ArrayList<>(List.of(color, ids, fragCoord, out));
        if (listAll) {
            iface.addAll(List.of(frame, anon, tex, img));
        }
        // Debug and annotation instructions go before the types in a real module; the reflector
        // reads them in any order, which this module relies on.
        m.op(15, new int[] {4, main}, "main", iface.stream().mapToInt(Integer::intValue).toArray());
        m.name(frameStruct, "sb_Frame");
        m.name(frame, "frame");
        m.name(buffer, "data");
        m.name(bufferStruct, "");
        m.name(tex, "colortex0");
        m.name(texArray, "lights");
        m.name(img, "debugImage");
        m.name(color, "vColor");
        m.name(ids, "vIds");
        m.name(out, "outColor");
        m.decorate(frameStruct, 2);
        m.decorate(anonStruct, 2);
        m.decorate(bufferStruct, 3);
        for (int v : List.of(frame, anon, buffer, tex, texArray, img)) {
            m.decorate(v, 34, 0);
            m.decorate(v, 33, v);
        }
        m.decorate(color, 30, 0);
        m.decorate(ids, 30, 1);
        m.decorate(ids, 14);
        m.decorate(fragCoord, 11, 15);
        m.decorate(unused, 30, 5);
        m.decorate(out, 30, 2);
        return m.build(version);
    }

    private static Map<String, Descriptor> byName(SpirvReflection r) {
        return r.descriptors().stream().collect(Collectors.toMap(Descriptor::name, Function.identity()));
    }

    @Test
    void namesAndClassifiesLikeSpirvCross() {
        SpirvReflection r = SpirvReflector.reflect(module(0x10000, false));
        assertEquals(SpirvReflection.Stage.FRAGMENT, r.stage());
        Map<String, Descriptor> d = byName(r);
        // Block type name; the instance name when the type is unnamed; else _<type>_<variable>.
        assertEquals(DescriptorType.UNIFORM_BUFFER, d.get("sb_Frame").type());
        assertEquals(DescriptorType.STORAGE_BUFFER, d.get("data").type());
        assertTrue(d.keySet().stream().anyMatch(n -> n.matches("_\\d+_\\d+")), d.keySet().toString());
        Descriptor tex = d.get("colortex0");
        assertEquals(DescriptorType.SAMPLED_IMAGE, tex.type());
        assertEquals(ImageDim.D2, tex.dim());
        assertEquals(1, tex.arraySize());
        assertEquals(ScalarClass.FLOAT, tex.sampled());
        assertTrue(tex.decorated());
        assertEquals(4, d.get("lights").arraySize());
        assertEquals(DescriptorType.STORAGE_IMAGE, d.get("debugImage").type());
        // Before SPIR-V 1.4 every descriptor counts; inputs and outputs only in the interface.
        assertEquals(6, r.descriptors().size());
        assertEquals(List.of("vColor", "vIds"), r.inputs().stream().map(InterfaceVariable::name).toList());
        InterfaceVariable ids = r.inputs().get(1);
        assertEquals(ScalarClass.INT, ids.scalar());
        assertEquals(2, ids.vectorSize());
        assertTrue(ids.flat());
        assertEquals(1, ids.location());
        assertEquals(List.of("outColor"), r.outputs().stream().map(InterfaceVariable::name).toList());
        assertEquals(2, r.outputs().getFirst().location());
    }

    @Test
    void spirv14OnlyCountsTheEntryPointInterface() {
        SpirvReflection r = SpirvReflector.reflect(module(0x10500, true));
        assertEquals(List.of("sb_Frame", "colortex0", "debugImage"),
            r.descriptors().stream().map(Descriptor::name).filter(n -> !n.startsWith("_")).toList());
        assertEquals(4, r.descriptors().size());
        assertFalse(byName(r).containsKey("data"));
    }

    @Test
    void rejectsMalformedModules() {
        assertThrows(IllegalArgumentException.class, () -> SpirvReflector.reflect(ByteBuffer.allocate(8)));
        ByteBuffer badMagic = module(0x10000, false);
        badMagic.putInt(0, 0x12345678);
        assertThrows(SpirvReflector.InvalidSpirvException.class, () -> SpirvReflector.reflect(badMagic));
        ByteBuffer truncated = module(0x10000, false);
        truncated.limit(truncated.limit() - 4);
        assertThrows(SpirvReflector.InvalidSpirvException.class, () -> SpirvReflector.reflect(truncated));
    }

    @Test
    void reflectsEveryStageOfACompiledPack() {
        RenderFixture fixture = RenderFixture.load(RenderFixture.TUTORIAL4);
        for (Program program : fixture.dim().programs()) {
            ProgramInterface iface = ProgramInterface.reflect(program, fixture.blobs());
            assertEquals(List.of(), iface.conflicts(), program.name());
            assertEquals(program.stages().size(), iface.stages().size(), program.name());
            iface.descriptors().values().forEach(d -> assertTrue(d.decorated(), program.name() + ": " + d.name()));
            if (program.stage(ShaderStage.FRAGMENT).isPresent()) {
                // Every declared output is at the physical location of one of the program's outputs
                // (outputs the shader never writes are not declared).
                assertTrue(program.outputSlots().containsAll(iface.fragmentOutputs().keySet()),
                    program.name() + ": " + iface.fragmentOutputs().keySet() + " vs " + program.outputSlots());
                assertFalse(iface.fragmentOutputs().isEmpty(), program.name());
            }
        }
    }
}
