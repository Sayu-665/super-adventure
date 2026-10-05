package dev.shaderbridge.compat.sodium;

import java.util.List;
import java.util.Optional;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Whether ShaderBridge's Sodium integration is in place. It is decided once, while Mixin loads
 * {@code shaderbridge-sodium.mixins.json} ({@code SodiumMixinPlugin}), before any game or Sodium
 * class is loaded: the integration's mixins are applied all together, and only when Sodium is
 * installed and every member they need exists ({@link SodiumTargets}); otherwise none is applied.
 * Thread-safe.
 *
 * <p>This class must not reference game or Sodium classes: it is used during Mixin's bootstrap.
 */
public final class SodiumIntegration {
    private static final Logger LOGGER = LoggerFactory.getLogger("ShaderBridge");

    /** The outcome of the decision. */
    public sealed interface Status {
        /** Sodium is not installed. */
        record Absent() implements Status {
        }

        /**
         * Sodium is installed and the integration's mixins are applied.
         *
         * @param version the installed Sodium version
         */
        record Active(String version) implements Status {
        }

        /**
         * Sodium is installed but the integration is not applied.
         *
         * @param version  the installed Sodium version
         * @param problems why (members of Sodium that are missing, a failed check)
         */
        record Unavailable(String version, List<String> problems) implements Status {
            public Unavailable {
                problems = List.copyOf(problems);
            }
        }

        /**
         * Sodium is installed but the integration's mixin configuration was never loaded (a broken
         * installation: the decision was not made).
         *
         * @param version the installed Sodium version
         */
        record NotLoaded(String version) implements Status {
        }
    }

    private static volatile Status decided;

    private SodiumIntegration() {
    }

    /**
     * Decides whether the integration's mixins are applied. Called once by the mixin
     * configuration plugin.
     *
     * @param sodiumVersion the installed Sodium version, if Sodium is installed
     * @param classes       reads class files without loading them
     * @return whether to apply the integration's mixins
     */
    public static boolean decide(Optional<String> sodiumVersion, SodiumTargets.ClassSource classes) {
        Status status = evaluate(sodiumVersion, classes);
        decided = status;
        switch (status) {
            case Status.Active a -> LOGGER.info("Sodium {} detected: shader packs shade Sodium's terrain", a.version());
            case Status.Unavailable u -> LOGGER.warn("Sodium {} detected, but ShaderBridge's Sodium integration cannot be applied: {}", u.version(),
                String.join("; ", u.problems()));
            default -> {
            }
        }
        return status instanceof Status.Active;
    }

    /**
     * @param sodiumVersion the installed Sodium version, if Sodium is installed
     * @param classes       reads class files without loading them
     * @return the decision
     */
    static Status evaluate(Optional<String> sodiumVersion, SodiumTargets.ClassSource classes) {
        if (sodiumVersion.isEmpty()) {
            return new Status.Absent();
        }
        List<String> problems;
        try {
            problems = SodiumTargets.problems(classes);
        } catch (RuntimeException | LinkageError e) {
            problems = List.of("the check of Sodium's classes failed: " + e);
        }
        return problems.isEmpty() ? new Status.Active(sodiumVersion.get()) : new Status.Unavailable(sodiumVersion.get(), problems);
    }

    /**
     * @param installedSodium the installed Sodium version, if Sodium is installed
     * @return the decision; {@link Status.NotLoaded} when Sodium is installed but nothing was decided
     */
    public static Status status(Optional<String> installedSodium) {
        Status status = decided;
        if (status != null) {
            return status;
        }
        return installedSodium.<Status>map(Status.NotLoaded::new).orElseGet(Status.Absent::new);
    }

    /** @return whether the integration's mixins are applied */
    public static boolean active() {
        return decided instanceof Status.Active;
    }

    /** Forgets the decision (tests). */
    static void reset() {
        decided = null;
    }
}
