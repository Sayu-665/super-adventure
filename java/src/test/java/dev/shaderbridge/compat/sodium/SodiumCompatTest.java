package dev.shaderbridge.compat.sodium;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.util.List;
import java.util.Optional;
import org.junit.jupiter.api.Test;

/**
 * {@link SodiumCompat}: packs render without Sodium and with a Sodium the integration fits; with a
 * Sodium it does not fit, the player is told why and what to do.
 */
class SodiumCompatTest {
    @Test
    void withoutSodiumPacksRender() {
        assertEquals(Optional.empty(), SodiumCompat.reason(new SodiumIntegration.Status.Absent(), List.of()));
    }

    @Test
    void withAFittingSodiumPacksRender() {
        assertEquals(Optional.empty(), SodiumCompat.reason(new SodiumIntegration.Status.Active("0.9.3-alpha.1+mc26.3"), List.of()));
    }

    @Test
    void anotherSodiumVersionNamesWhatIsMissing() {
        String message = SodiumCompat.reason(new SodiumIntegration.Status.Unavailable("0.10.0+mc26.4", List.of("SodiumWorldRenderer.initRenderer()V is missing")),
            List.of()).orElseThrow();
        assertTrue(message.startsWith("Sodium 0.10.0+mc26.4 is installed (ShaderBridge supports Sodium 0.9.x), "), message);
        assertTrue(message.contains("SodiumWorldRenderer.initRenderer()V is missing"), message);
        assertTrue(message.endsWith("remove Sodium to use shader packs"), message);
    }

    @Test
    void aSupportedVersionIsNotCalledUnsupported() {
        String message = SodiumCompat.reason(new SodiumIntegration.Status.Unavailable("0.9.4+mc26.3", List.of("x is missing")), List.of()).orElseThrow();
        assertTrue(message.startsWith("Sodium 0.9.4+mc26.3 is installed, and ShaderBridge cannot shade its terrain: "), message);
    }

    @Test
    void aVertexFormatMismatchBlocksPacks() {
        String message = SodiumCompat.reason(new SodiumIntegration.Status.Active("0.9.3-alpha.1+mc26.3"), List.of("Sodium's chunk vertex: vertex size 24, expected 20"))
            .orElseThrow();
        assertTrue(message.contains("its chunk vertex format is not the one ShaderBridge extends (Sodium's chunk vertex: vertex size 24, expected 20)"), message);
    }

    @Test
    void anIntegrationThatDidNotLoadBlocksPacks() {
        String message = SodiumCompat.reason(new SodiumIntegration.Status.NotLoaded("0.9.2+mc26.3"), List.of()).orElseThrow();
        assertTrue(message.contains("ShaderBridge's Sodium integration was not loaded"), message);
    }
}
