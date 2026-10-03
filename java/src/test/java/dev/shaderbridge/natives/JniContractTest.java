package dev.shaderbridge.natives;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.io.IOException;
import java.io.InputStream;
import java.lang.reflect.Constructor;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;
import java.util.Arrays;
import java.util.Map;
import java.util.TreeMap;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import org.junit.jupiter.api.Test;

/**
 * Compares {@link ShaderBridgeNative} with the fixed JNI contract shared with {@code crates/sb-jni}
 * ({@code jni_contract.txt} is the declaration block of the contract, verbatim): same methods, same
 * parameter and return types, all {@code public static native}, nothing extra. A mismatch would
 * only surface as an {@link UnsatisfiedLinkError} at run time.
 */
class JniContractTest {
    private static final Pattern DECLARATION = Pattern.compile(
        "public static native ([\\w.]+) (\\w+)\\(([^)]*)\\);");
    private static final Map<String, Class<?>> TYPES = Map.of(
        "void", void.class,
        "boolean", boolean.class,
        "long", long.class,
        "float", float.class,
        "String", String.class,
        "java.nio.ByteBuffer", ByteBuffer.class);

    private static String contract() throws IOException {
        try (InputStream in = JniContractTest.class.getResourceAsStream("jni_contract.txt")) {
            assertNotNull(in, "jni_contract.txt");
            return new String(in.readAllBytes(), StandardCharsets.UTF_8);
        }
    }

    private static Class<?> type(String name) {
        Class<?> type = TYPES.get(name);
        assertNotNull(type, "unexpected type in the contract: " + name);
        return type;
    }

    @Test
    void nativeMethodsMatchTheContractExactly() throws IOException {
        Map<String, String> expected = new TreeMap<>();
        Matcher m = DECLARATION.matcher(contract());
        while (m.find()) {
            Class<?>[] parameters = m.group(3).isBlank()
                ? new Class<?>[0]
                : Arrays.stream(m.group(3).split(",")).map(p -> type(p.trim().split("\\s+")[0])).toArray(Class<?>[]::new);
            expected.put(m.group(2), signature(type(m.group(1)), parameters));
        }
        assertEquals(17, expected.size(), "methods in the contract");

        Map<String, String> actual = new TreeMap<>();
        for (Method method : ShaderBridgeNative.class.getDeclaredMethods()) {
            if (method.isSynthetic()) {
                continue;
            }
            int modifiers = method.getModifiers();
            assertTrue(Modifier.isPublic(modifiers) && Modifier.isStatic(modifiers) && Modifier.isNative(modifiers),
                method + " must be public static native");
            assertFalse(method.getName().contains("_"), "JNI would mangle '_' in " + method.getName());
            String previous = actual.put(method.getName(), signature(method.getReturnType(), method.getParameterTypes()));
            assertEquals(null, previous, "overloads cannot be bound by the short JNI names: " + method.getName());
        }
        assertEquals(expected, actual);
    }

    @Test
    void bindingClassIsAFinalNonInstantiableHolder() {
        assertTrue(Modifier.isFinal(ShaderBridgeNative.class.getModifiers()));
        assertEquals("dev.shaderbridge.natives.ShaderBridgeNative", ShaderBridgeNative.class.getName(),
            "the JNI symbol prefix is Java_dev_shaderbridge_natives_ShaderBridgeNative_");
        for (Constructor<?> constructor : ShaderBridgeNative.class.getDeclaredConstructors()) {
            assertTrue(Modifier.isPrivate(constructor.getModifiers()));
        }
    }

    private static String signature(Class<?> returnType, Class<?>[] parameters) {
        return returnType.getName() + Arrays.stream(parameters).map(Class::getName).toList();
    }
}
