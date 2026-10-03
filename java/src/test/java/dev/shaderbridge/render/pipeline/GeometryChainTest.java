package dev.shaderbridge.render.pipeline;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assumptions.assumeTrue;

import dev.shaderbridge.model.GeometryProgram;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Arrays;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import org.junit.jupiter.api.Test;

class GeometryChainTest {
    private static final Pattern ENTRY = Pattern.compile("(\\w+) = \"(\\w+)\", fallback = (?:None|Some\\(G::(\\w+)\\)), group");

    @Test
    void fallbacksMatchTheRustTable() throws IOException {
        String configured = System.getProperty("shaderbridge.repoRoot");
        Path file = (configured != null ? Path.of(configured) : Path.of("..")).resolve("crates/sb-core/src/program.rs");
        assumeTrue(Files.isRegularFile(file), "sb-core sources not available");
        Map<String, String> fileOfVariant = new HashMap<>();
        Map<String, String> fallbackOfFile = new HashMap<>();
        Matcher m = ENTRY.matcher(Files.readString(file));
        while (m.find()) {
            fileOfVariant.put(m.group(1), m.group(2));
            fallbackOfFile.put(m.group(2), m.group(3));
        }
        assertEquals(GeometryProgram.values().length, fallbackOfFile.size(), "every geometry program is in the Rust table");
        for (GeometryProgram program : GeometryProgram.values()) {
            String rustFallback = fallbackOfFile.get(program.fileName());
            Optional<String> expected = Optional.ofNullable(rustFallback).map(fileOfVariant::get);
            assertEquals(expected, GeometryChain.fallback(program).map(GeometryProgram::fileName), program.fileName());
        }
    }

    @Test
    void chainsEndAtTheirRoots() {
        assertEquals(List.of(GeometryProgram.WATER, GeometryProgram.TERRAIN, GeometryProgram.TEXTURED_LIT, GeometryProgram.TEXTURED,
            GeometryProgram.BASIC), GeometryChain.chain(GeometryProgram.WATER));
        assertEquals(List.of(GeometryProgram.SHADOW_LIGHTNING, GeometryProgram.SHADOW_ENTITIES, GeometryProgram.SHADOW),
            GeometryChain.chain(GeometryProgram.SHADOW_LIGHTNING));
        assertEquals(List.of(GeometryProgram.DH_SHADOW), GeometryChain.chain(GeometryProgram.DH_SHADOW));
        Arrays.stream(GeometryProgram.values()).forEach(p -> assertEquals(p, GeometryChain.chain(p).getFirst()));
    }
}
