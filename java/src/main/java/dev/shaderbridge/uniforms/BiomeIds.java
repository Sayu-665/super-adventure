package dev.shaderbridge.uniforms;

import java.lang.reflect.Field;
import java.lang.reflect.Modifier;
import java.util.Collection;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.Locale;
import java.util.Map;
import java.util.TreeSet;
import net.minecraft.client.multiplayer.ClientLevel;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.ResourceKey;
import net.minecraft.world.level.biome.Biomes;

/**
 * Numbers biomes for the {@code BIOME_*} macros and the {@code biome} uniform. Ids follow the
 * sorted order of the namespaced biome ids, so they are stable for a given biome set and match
 * the order of data-driven registries. The macro name is {@code BIOME_} plus the upper-cased path,
 * as in Iris; the first biome wins when two namespaces share a path.
 */
public final class BiomeIds {
    /** Prefix of the biome macros. */
    public static final String MACRO_PREFIX = "BIOME_";

    private BiomeIds() {
    }

    /**
     * @param namespacedIds biome ids such as {@code minecraft:plains}
     * @return macro name to id, in id order
     */
    public static Map<String, Integer> assign(Collection<String> namespacedIds) {
        Map<String, Integer> out = new LinkedHashMap<>();
        int next = 0;
        for (String id : new TreeSet<>(namespacedIds)) {
            String macro = macroName(id);
            if (!out.containsKey(macro)) {
                out.put(macro, next++);
            }
        }
        return Collections.unmodifiableMap(out);
    }

    /**
     * @param namespacedId a biome id such as {@code minecraft:snowy_taiga}
     * @return its macro name, e.g. {@code BIOME_SNOWY_TAIGA}
     */
    public static String macroName(String namespacedId) {
        int colon = namespacedId.indexOf(':');
        String path = colon >= 0 ? namespacedId.substring(colon + 1) : namespacedId;
        return MACRO_PREFIX + path.replace('/', '_').replace('.', '_').toUpperCase(Locale.ROOT);
    }

    /**
     * @param macros macros of a compile environment
     * @return the {@code BIOME_*} entries parsed back into ids
     */
    public static Map<String, Integer> fromMacros(Map<String, String> macros) {
        Map<String, Integer> out = new LinkedHashMap<>();
        macros.forEach((name, value) -> {
            if (name.startsWith(MACRO_PREFIX) && value != null) {
                try {
                    out.put(name, Integer.parseInt(value));
                } catch (NumberFormatException ignored) {
                    // Not one of ours; a user macro that happens to share the prefix.
                }
            }
        });
        return Collections.unmodifiableMap(out);
    }

    /**
     * The biomes known right now: the vanilla biomes plus those of the current world's registry.
     *
     * @param level the current world, or null outside a world
     * @return macro name to id
     */
    public static Map<String, Integer> current(ClientLevel level) {
        TreeSet<String> ids = new TreeSet<>(vanillaBiomes());
        if (level != null) {
            level.registryAccess().lookupOrThrow(Registries.BIOME).keySet().forEach(id -> ids.add(id.toString()));
        }
        return assign(ids);
    }

    private static Collection<String> vanillaBiomes() {
        TreeSet<String> ids = new TreeSet<>();
        for (Field field : Biomes.class.getFields()) {
            if (Modifier.isStatic(field.getModifiers()) && field.getType() == ResourceKey.class) {
                try {
                    Identifier id = ((ResourceKey<?>) field.get(null)).identifier();
                    ids.add(id.toString());
                } catch (IllegalAccessException e) {
                    throw new IllegalStateException("Biomes fields are public", e);
                }
            }
        }
        return ids;
    }
}
