package dev.shaderbridge.render.pipeline;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.Test;

class TomlSubsetTest {
    @Test
    void parsesTheProfileSubset() {
        Map<String, Object> root = TomlSubset.parse("""
            # comment
            name = "p" # trailing comment
            fullscreen = true
            literal = 'C:\\path'
            [[inputs]]
            name = "Position"
            location = 0
            instanced = false
            [[inputs]]
            name = "UV0"
            location = 2
            [semantics]
            position = "vec4(\\"x\\")"
            [code]
            vertex = \"""
            void f() {}
            \"""
            provides = ["a",
                "b", ]
            """);
        assertEquals("p", root.get("name"));
        assertEquals(true, root.get("fullscreen"));
        assertEquals("C:\\path", root.get("literal"));
        @SuppressWarnings("unchecked")
        List<Map<String, Object>> inputs = (List<Map<String, Object>>) root.get("inputs");
        assertEquals(2, inputs.size());
        assertEquals(2L, inputs.get(1).get("location"));
        @SuppressWarnings("unchecked")
        Map<String, Object> code = (Map<String, Object>) root.get("code");
        assertEquals("void f() {}\n", code.get("vertex"));
        assertEquals(List.of("a", "b"), code.get("provides"));
        @SuppressWarnings("unchecked")
        Map<String, Object> semantics = (Map<String, Object>) root.get("semantics");
        assertEquals("vec4(\"x\")", semantics.get("position"));
    }

    @Test
    void rejectsSyntaxOutsideTheSubset() {
        assertThrows(IllegalArgumentException.class, () -> TomlSubset.parse("a.b = 1"));
        assertThrows(IllegalArgumentException.class, () -> TomlSubset.parse("a = 1\na = 2"));
        assertThrows(IllegalArgumentException.class, () -> TomlSubset.parse("a = 1.5"));
        assertThrows(IllegalArgumentException.class, () -> TomlSubset.parse("a = { b = 1 }"));
        assertThrows(IllegalArgumentException.class, () -> TomlSubset.parse("a = \"unterminated"));
    }

    @Test
    void drawProfileDefinitionsAreValidated() {
        DrawProfileInfo info = DrawProfileInfo.parse("""
            name = "x"
            [[inputs]]
            name = "a"
            type = "uvec2"
            location = 1
            instanced = true
            [[blocks]]
            name = "B"
            [[samplers]]
            name = "S"
            provides = ["gtexture"]
            """);
        assertEquals(List.of(new DrawProfileInfo.Input("a", "uvec2", 1, true)), info.inputs());
        assertEquals(List.of("B"), info.blocks());
        assertEquals("sampler2D", info.sampler("S").orElseThrow().type());
        assertTrue(info.sampler("T").isEmpty());
        assertThrows(IllegalArgumentException.class, () -> DrawProfileInfo.parse("description = \"no name\""));
        assertThrows(IllegalArgumentException.class, () -> DrawProfileInfo.parse("name = \"x\"\n[[inputs]]\nname = \"a\"\ntype = \"vec2\"\nlocation = \"0\""));
        DrawProfiles profiles = DrawProfiles.get();
        assertTrue(profiles.profile("../escape").isEmpty());
        assertTrue(profiles.profile("no_such_profile").isEmpty());
    }
}
