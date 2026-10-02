package dev.shaderbridge.natives;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.util.Optional;
import org.junit.jupiter.api.Test;

class NativePlatformTest {
    @Test
    void mapsJavaPropertiesToResourceNames() {
        assertEquals(Optional.of(new NativePlatform("linux", "x86_64")), NativePlatform.detect("Linux", "amd64"));
        assertEquals(Optional.of(new NativePlatform("windows", "x86_64")), NativePlatform.detect("Windows 11", "amd64"));
        assertEquals(Optional.of(new NativePlatform("macos", "aarch64")), NativePlatform.detect("Mac OS X", "aarch64"));
        assertEquals(Optional.of(new NativePlatform("linux", "aarch64")), NativePlatform.detect("Linux", "arm64"));
    }

    @Test
    void rejectsUnsupportedPlatforms() {
        assertTrue(NativePlatform.detect("Linux", "x86").isEmpty());
        assertTrue(NativePlatform.detect("SunOS", "amd64").isEmpty());
        assertTrue(NativePlatform.detect("FreeBSD", "amd64").isEmpty());
    }

    @Test
    void libraryNamesFollowTheJniContract() {
        assertEquals("/natives/linux-x86_64/libsb_jni.so", new NativePlatform("linux", "x86_64").resourcePath());
        assertEquals("/natives/windows-x86_64/sb_jni.dll", new NativePlatform("windows", "x86_64").resourcePath());
        assertEquals("/natives/macos-aarch64/libsb_jni.dylib", new NativePlatform("macos", "aarch64").resourcePath());
    }
}
