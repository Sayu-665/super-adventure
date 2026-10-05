package dev.shaderbridge;

import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import net.fabricmc.loader.api.Version;
import net.fabricmc.loader.api.VersionParsingException;
import net.fabricmc.loader.api.metadata.version.VersionPredicate;
import org.junit.jupiter.api.Test;

/**
 * {@code fabric.mod.json}: the Distant Horizons range ShaderBridge's integration is written for
 * (3.3.x, verified against 3.3.4: reversed-Z LODs and the {@code BLAZE_3D} vertex format) is
 * suggested, and older releases are declared incompatible, evaluated with Fabric Loader's own
 * version predicates.
 */
class ModMetadataTest {
    private static JsonObject mod() throws IOException {
        try (InputStream in = ModMetadataTest.class.getResourceAsStream("/fabric.mod.json")) {
            assertNotNull(in, "fabric.mod.json");
            return JsonParser.parseString(new String(in.readAllBytes(), StandardCharsets.UTF_8)).getAsJsonObject();
        }
    }

    private static boolean matches(String predicate, String version) throws VersionParsingException {
        return VersionPredicate.parse(predicate).test(Version.parse(version));
    }

    @Test
    void distantHorizonsBefore33IsDeclaredBroken() throws IOException, VersionParsingException {
        JsonObject mod = mod();
        String suggested = mod.getAsJsonObject("suggests").get("distanthorizons").getAsString();
        String broken = mod.getAsJsonObject("breaks").get("distanthorizons").getAsString();
        for (String supported : new String[] {"3.3.0", "3.3.4", "3.4.0"}) {
            assertTrue(matches(suggested, supported), suggested + " should accept " + supported);
            assertFalse(matches(broken, supported), broken + " should not break " + supported);
        }
        for (String old : new String[] {"3.2.1", "3.1.0", "2.3.2"}) {
            assertFalse(matches(suggested, old), suggested + " should not accept " + old);
            assertTrue(matches(broken, old), broken + " should break " + old);
        }
        assertTrue(mod.getAsJsonObject("breaks").has("iris"), "Iris and ShaderBridge both replace world rendering");
    }
}
