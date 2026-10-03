package dev.shaderbridge.render.pipeline;

import java.io.IOException;
import java.io.InputStream;
import java.io.UncheckedIOException;
import java.nio.charset.StandardCharsets;
import java.util.Map;
import java.util.Optional;
import java.util.concurrent.ConcurrentHashMap;

/**
 * The draw profiles the host knows: sb-transform's built-in profiles, packaged as resources under
 * {@value #RESOURCE_DIR} by the build, plus profiles registered at runtime (the same TOML also goes
 * to the native {@code registerProfile}). Thread-safe.
 */
public final class DrawProfiles {
    /** Resource directory of the built-in profile files ({@code <name>.toml}). */
    public static final String RESOURCE_DIR = "/dev/shaderbridge/profiles/";
    /**
     * The profile of Distant Horizons programs synthesized from {@code gbuffers_*} programs
     * ({@code sb_transform::DH_SYNTH_PROFILE}). sb-transform derives it in code from
     * {@code dh_terrain}: only its lightmap semantic differs, so its inputs, blocks and samplers
     * are those of {@code dh_terrain}.
     */
    public static final String DH_SYNTH_PROFILE = "dh_terrain_synth";

    private static final DrawProfiles BUILTIN = new DrawProfiles();

    private final Map<String, Optional<DrawProfileInfo>> cache = new ConcurrentHashMap<>();

    private DrawProfiles() {
    }

    /** @return the shared registry */
    public static DrawProfiles get() {
        return BUILTIN;
    }

    /**
     * @param name a profile id
     * @return the profile, if it is built in or registered
     * @throws IllegalStateException if the packaged file of a built-in profile is invalid
     */
    public Optional<DrawProfileInfo> profile(String name) {
        if (name.equals(DH_SYNTH_PROFILE)) {
            return profile("dh_terrain").map(p -> new DrawProfileInfo(DH_SYNTH_PROFILE, p.fullscreen(), p.inputs(), p.blocks(), p.samplers()));
        }
        return cache.computeIfAbsent(name, DrawProfiles::loadBuiltin);
    }

    /**
     * Registers (or replaces) a profile.
     *
     * @param toml the profile definition
     * @return the registered profile
     * @throws IllegalArgumentException if the definition is invalid
     */
    public DrawProfileInfo register(String toml) {
        DrawProfileInfo info = DrawProfileInfo.parse(toml);
        cache.put(info.name(), Optional.of(info));
        return info;
    }

    private static Optional<DrawProfileInfo> loadBuiltin(String name) {
        if (!name.matches("[A-Za-z0-9_]+")) {
            return Optional.empty();
        }
        try (InputStream in = DrawProfiles.class.getResourceAsStream(RESOURCE_DIR + name + ".toml")) {
            if (in == null) {
                return Optional.empty();
            }
            return Optional.of(DrawProfileInfo.parse(new String(in.readAllBytes(), StandardCharsets.UTF_8)));
        } catch (IOException e) {
            throw new UncheckedIOException("Cannot read the draw profile " + name, e);
        } catch (IllegalArgumentException e) {
            throw new IllegalStateException("The packaged draw profile " + name + " is invalid: " + e.getMessage(), e);
        }
    }
}
