package dev.shaderbridge;

import static org.junit.jupiter.api.Assertions.assertDoesNotThrow;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.io.IOException;
import java.io.InputStream;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import org.junit.jupiter.api.Test;

/**
 * Checks that every class-tweaker entry and every mixin class of the mod names something that
 * exists in the Minecraft 26.3 jar on the test class path (classes are looked up without being
 * initialized). A Minecraft update that renames a target fails here instead of at game start.
 */
class GameTargetsTest {
    private static String resource(String name) throws IOException {
        try (InputStream in = GameTargetsTest.class.getResourceAsStream("/" + name)) {
            assertNotNull(in, name);
            return new String(in.readAllBytes(), StandardCharsets.UTF_8);
        }
    }

    private static Class<?> load(String internalName) throws ClassNotFoundException {
        return Class.forName(internalName.replace('/', '.'), false, GameTargetsTest.class.getClassLoader());
    }

    /** {@code Lpkg/Name;} or a primitive descriptor to the binary type name. */
    private static String typeName(String descriptor) {
        int dims = 0;
        while (descriptor.charAt(dims) == '[') {
            dims++;
        }
        String element = descriptor.substring(dims);
        String name = switch (element.charAt(0)) {
            case 'L' -> element.substring(1, element.length() - 1).replace('/', '.');
            case 'Z' -> "boolean";
            case 'B' -> "byte";
            case 'C' -> "char";
            case 'S' -> "short";
            case 'I' -> "int";
            case 'J' -> "long";
            case 'F' -> "float";
            case 'D' -> "double";
            default -> throw new AssertionError("unsupported descriptor " + descriptor);
        };
        return name + "[]".repeat(dims);
    }

    @Test
    void classTweakerTargetsExist() throws Exception {
        List<String> lines = resource("shaderbridge.classtweaker").lines().map(String::strip)
            .filter(l -> !l.isEmpty() && !l.startsWith("#")).toList();
        assertTrue(lines.getFirst().matches("classTweaker\\s+v1\\s+official"), lines.getFirst());
        List<String> checked = new ArrayList<>();
        for (String line : lines.subList(1, lines.size())) {
            String[] f = line.split("\\s+");
            Class<?> owner = load(f[2]);
            switch (f[1]) {
                case "class" -> assertEquals(3, f.length, line);
                case "field" -> {
                    Field field = assertDoesNotThrow(() -> owner.getDeclaredField(f[3]), line);
                    assertEquals(typeName(f[4]), field.getType().getTypeName(), line);
                }
                case "method" -> assertTrue(Arrays.stream(owner.getDeclaredMethods()).anyMatch(m -> m.getName().equals(f[3])
                    && descriptor(m).equals(f[4])), line);
                default -> throw new AssertionError("unsupported entry " + line);
            }
            checked.add(line);
        }
        assertTrue(checked.size() >= 4, checked.toString());
    }

    private static String descriptor(Method method) {
        StringBuilder out = new StringBuilder("(");
        for (Class<?> parameter : method.getParameterTypes()) {
            out.append(parameter.descriptorString());
        }
        return out.append(')').append(method.getReturnType().descriptorString()).toString();
    }

    @Test
    void mixinClassesExist() throws Exception {
        JsonObject config = JsonParser.parseString(resource("shaderbridge.mixins.json")).getAsJsonObject();
        String pkg = config.get("package").getAsString();
        for (String side : List.of("mixins", "client", "server")) {
            if (config.has(side)) {
                for (JsonElement name : config.getAsJsonArray(side)) {
                    assertDoesNotThrow(() -> load((pkg + "." + name.getAsString()).replace('.', '/')), name.getAsString());
                }
            }
        }
        JsonObject mod = JsonParser.parseString(resource("fabric.mod.json")).getAsJsonObject();
        assertEquals("shaderbridge.classtweaker", mod.get("accessWidener").getAsString());
        assertEquals("shaderbridge.mixins.json", mod.getAsJsonArray("mixins").get(0).getAsString());
        for (JsonElement entrypoint : mod.getAsJsonObject("entrypoints").getAsJsonArray("client")) {
            assertDoesNotThrow(() -> load(entrypoint.getAsString().replace('.', '/')), entrypoint.getAsString());
        }
    }
}
