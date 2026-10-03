package dev.shaderbridge;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.io.IOException;
import java.io.InputStream;
import java.lang.reflect.Constructor;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import org.junit.jupiter.api.Test;
import org.objectweb.asm.ClassReader;
import org.objectweb.asm.Type;
import org.objectweb.asm.tree.AnnotationNode;
import org.objectweb.asm.tree.ClassNode;
import org.objectweb.asm.tree.MethodNode;

/**
 * Verifies every injection of every mixin of the mod against the Minecraft 26.3 jar on the test
 * class path: the target classes exist and each {@code @Inject}'s {@code method} selector names a
 * declared method (with the exact descriptor when the selector gives one), and the handler's
 * parameters are the target method's parameters plus the callback info. The annotations are read
 * from the class files with ASM, as Mixin itself does.
 */
class MixinTargetsTest {
    private static final String MIXIN = "Lorg/spongepowered/asm/mixin/Mixin;";
    private static final String INJECT = "Lorg/spongepowered/asm/mixin/injection/Inject;";
    private static final String CALLBACK_INFO = "Lorg/spongepowered/asm/mixin/injection/callback/CallbackInfo";

    private static List<String> mixinClasses() throws IOException {
        JsonObject config;
        try (InputStream in = MixinTargetsTest.class.getResourceAsStream("/shaderbridge.mixins.json")) {
            assertNotNull(in);
            config = JsonParser.parseString(new String(in.readAllBytes(), StandardCharsets.UTF_8)).getAsJsonObject();
        }
        String pkg = config.get("package").getAsString();
        List<String> out = new ArrayList<>();
        for (String side : List.of("mixins", "client", "server")) {
            if (config.has(side)) {
                for (JsonElement name : config.getAsJsonArray(side)) {
                    out.add(pkg + "." + name.getAsString());
                }
            }
        }
        return out;
    }

    private static ClassNode read(String className) throws IOException {
        try (InputStream in = MixinTargetsTest.class.getResourceAsStream("/" + className.replace('.', '/') + ".class")) {
            assertNotNull(in, className);
            ClassNode node = new ClassNode();
            new ClassReader(in.readAllBytes()).accept(node, ClassReader.SKIP_CODE);
            return node;
        }
    }

    private static Object value(AnnotationNode annotation, String key) {
        if (annotation.values == null) {
            return null;
        }
        for (int i = 0; i < annotation.values.size(); i += 2) {
            if (annotation.values.get(i).equals(key)) {
                return annotation.values.get(i + 1);
            }
        }
        return null;
    }

    private static List<AnnotationNode> annotations(List<AnnotationNode> visible, List<AnnotationNode> invisible) {
        List<AnnotationNode> out = new ArrayList<>();
        if (visible != null) {
            out.addAll(visible);
        }
        if (invisible != null) {
            out.addAll(invisible);
        }
        return out;
    }

    private static String descriptor(Method m) {
        return Type.getMethodDescriptor(m);
    }

    @Test
    void everyInjectionMatchesItsTargetMethod() throws Exception {
        List<String> classes = mixinClasses();
        assertFalse(classes.isEmpty());
        int injections = 0;
        for (String mixinClass : classes) {
            ClassNode node = read(mixinClass);
            AnnotationNode mixin = annotations(node.visibleAnnotations, node.invisibleAnnotations).stream().filter(a -> a.desc.equals(MIXIN))
                .findFirst().orElseThrow(() -> new AssertionError(mixinClass + " has no @Mixin"));
            @SuppressWarnings("unchecked")
            List<Type> targets = (List<Type>) value(mixin, "value");
            assertNotNull(targets, mixinClass + ": @Mixin without class targets");
            List<Class<?>> targetClasses = new ArrayList<>();
            for (Type t : targets) {
                targetClasses.add(Class.forName(t.getClassName(), false, MixinTargetsTest.class.getClassLoader()));
            }
            for (MethodNode handler : node.methods) {
                for (AnnotationNode a : annotations(handler.visibleAnnotations, handler.invisibleAnnotations)) {
                    if (!a.desc.equals(INJECT)) {
                        continue;
                    }
                    @SuppressWarnings("unchecked")
                    List<String> selectors = (List<String>) value(a, "method");
                    for (String selector : selectors) {
                        checkInjection(mixinClass + "#" + handler.name, selector, handler, targetClasses);
                        injections++;
                    }
                }
            }
        }
        assertTrue(injections >= 1);
    }

    private static void checkInjection(String where, String selector, MethodNode handler, List<Class<?>> targets) {
        int paren = selector.indexOf('(');
        String name = paren < 0 ? selector : selector.substring(0, paren);
        String desc = paren < 0 ? null : selector.substring(paren);
        List<Method> matches = targets.stream().flatMap(c -> Arrays.stream(c.getDeclaredMethods()))
            .filter(m -> m.getName().equals(name) && (desc == null || descriptor(m).equals(desc))).toList();
        assertEquals(1, matches.size(), where + ": selector " + selector + " must match exactly one method, matched " + matches);
        Method target = matches.getFirst();
        Type[] handlerArgs = Type.getArgumentTypes(handler.desc);
        Type[] targetArgs = Type.getArgumentTypes(target);
        assertTrue(handlerArgs.length == targetArgs.length + 1, where + ": handler takes the target's parameters and the callback info");
        for (int i = 0; i < targetArgs.length; i++) {
            assertEquals(targetArgs[i], handlerArgs[i], where + ": parameter " + i);
        }
        assertTrue(handlerArgs[targetArgs.length].getInternalName().startsWith(CALLBACK_INFO.substring(1)), where + ": last parameter");
    }

    @Test
    void injectedSpirvModuleConstructorExists() throws Exception {
        Class<?> module = Class.forName("com.mojang.renderpearl.frontend.shaders.SPIRVModule", false, MixinTargetsTest.class.getClassLoader());
        Constructor<?> constructor = module.getConstructor(java.nio.ByteBuffer.class,
            Class.forName("com.mojang.renderpearl.api.pipeline.ShaderType", false, MixinTargetsTest.class.getClassLoader()));
        assertNotNull(constructor);
        Class<?> spvModule = Class.forName("com.mojang.renderpearl.backend.api.SpvModule", false, MixinTargetsTest.class.getClassLoader());
        assertTrue(spvModule.isAssignableFrom(module));
    }
}
