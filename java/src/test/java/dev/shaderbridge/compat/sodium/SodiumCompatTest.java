package dev.shaderbridge.compat.sodium;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.util.Optional;
import org.junit.jupiter.api.Test;

/** {@link SodiumCompat}: packs render without Sodium; with Sodium the player is told why they do not. */
class SodiumCompatTest {
    @Test
    void withoutSodiumPacksRender() {
        assertEquals(Optional.empty(), SodiumCompat.reason(Optional.empty()));
    }

    @Test
    void withSodiumThePlayerIsToldWhy() {
        String message = SodiumCompat.reason(Optional.of("0.9.3-alpha.1+mc26.3")).orElseThrow();
        assertTrue(message.startsWith("Sodium 0.9.3-alpha.1+mc26.3 is installed, "), message);
        assertTrue(message.contains("remove Sodium to use shader packs"), message);
    }

    @Test
    void otherSodiumVersionsAreNamedAsUntested() {
        String message = SodiumCompat.reason(Optional.of("0.10.0+mc26.4")).orElseThrow();
        assertTrue(message.contains("written against Sodium 0.9.x"), message);
    }
}
