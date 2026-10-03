package dev.shaderbridge.render.pipeline;

import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Optional;

/**
 * The parts of a draw profile ({@code crates/sb-transform/profiles/*.toml}, ARCHITECTURE §6) the
 * host needs: the vertex attributes it must feed and the host blocks and samplers it binds itself.
 *
 * @param name       profile id
 * @param fullscreen the profile draws a fullscreen triangle without vertex buffers
 * @param inputs     vertex attributes, named like the host vertex format elements
 * @param blocks     host uniform block type names
 * @param samplers   host samplers
 */
public record DrawProfileInfo(String name, boolean fullscreen, List<Input> inputs, List<String> blocks, List<Sampler> samplers) {
    public DrawProfileInfo {
        inputs = List.copyOf(inputs);
        blocks = List.copyOf(blocks);
        samplers = List.copyOf(samplers);
    }

    /**
     * A vertex attribute.
     *
     * @param name     attribute and vertex format element name
     * @param type     GLSL type ({@code vec3}, {@code ivec2}, ...)
     * @param location attribute location
     */
    public record Input(String name, String type, int location) {
    }

    /**
     * A host sampler.
     *
     * @param name     the name the host binds it under ({@code Sampler0}, {@code uLightMap})
     * @param type     GLSL sampler type
     * @param provides canonical pack sampler names it stands for ({@code gtexture}, {@code lightmap})
     */
    public record Sampler(String name, String type, List<String> provides) {
        public Sampler {
            provides = List.copyOf(provides);
        }
    }

    /**
     * @param name a host sampler name
     * @return the profile's sampler of that name
     */
    public Optional<Sampler> sampler(String name) {
        return samplers.stream().filter(s -> s.name().equals(name)).findFirst();
    }

    /**
     * Reads a profile definition.
     *
     * @param toml profile TOML
     * @return the profile
     * @throws IllegalArgumentException if the text is not a valid profile
     */
    public static DrawProfileInfo parse(String toml) {
        Map<String, Object> root = TomlSubset.parse(toml);
        String name = string(root, "name");
        boolean fullscreen = Boolean.TRUE.equals(root.get("fullscreen"));
        List<Input> inputs = new ArrayList<>();
        for (Map<String, Object> t : tables(root, "inputs")) {
            if (!(required(t, "location") instanceof Long location)) {
                throw new IllegalArgumentException("`location` must be an integer");
            }
            inputs.add(new Input(string(t, "name"), string(t, "type"), Math.toIntExact(location)));
        }
        List<String> blocks = new ArrayList<>();
        for (Map<String, Object> t : tables(root, "blocks")) {
            blocks.add(string(t, "name"));
        }
        List<Sampler> samplers = new ArrayList<>();
        for (Map<String, Object> t : tables(root, "samplers")) {
            List<String> provides = new ArrayList<>();
            if (t.get("provides") instanceof List<?> list) {
                list.forEach(p -> provides.add((String) p));
            }
            Object type = t.getOrDefault("type", "sampler2D");
            samplers.add(new Sampler(string(t, "name"), (String) type, provides));
        }
        return new DrawProfileInfo(name, fullscreen, inputs, blocks, samplers);
    }

    private static Object required(Map<String, Object> table, String key) {
        Object value = table.get(key);
        if (value == null) {
            throw new IllegalArgumentException("missing `" + key + "`");
        }
        return value;
    }

    private static String string(Map<String, Object> table, String key) {
        if (required(table, key) instanceof String s) {
            return s;
        }
        throw new IllegalArgumentException("`" + key + "` must be a string");
    }

    @SuppressWarnings("unchecked")
    private static List<Map<String, Object>> tables(Map<String, Object> root, String key) {
        Object value = root.get(key);
        if (value == null) {
            return List.of();
        }
        if (value instanceof List<?> list && list.stream().allMatch(Map.class::isInstance)) {
            return (List<Map<String, Object>>) value;
        }
        throw new IllegalArgumentException("`" + key + "` must be an array of tables");
    }
}
