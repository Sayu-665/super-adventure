package dev.shaderbridge.compat.sodium;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.io.IOException;
import java.io.InputStream;
import java.net.URISyntaxException;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Set;
import java.util.TreeSet;
import java.util.stream.Stream;
import org.junit.jupiter.api.Test;
import org.objectweb.asm.Opcodes;
import org.objectweb.asm.Type;
import org.objectweb.asm.tree.AbstractInsnNode;
import org.objectweb.asm.tree.AnnotationNode;
import org.objectweb.asm.tree.ClassNode;
import org.objectweb.asm.tree.MethodInsnNode;
import org.objectweb.asm.tree.MethodNode;

/**
 * Checks the mixins of {@code shaderbridge-sodium.mixins.json} against the Sodium and Minecraft
 * class files on the test class path, as Mixin would apply them: every target class and method
 * exists, every {@code @At INVOKE} call site exists in its method, every handler's parameters fit
 * its injector, and every member a mixin hooks is one {@link SodiumTargets} checks before the
 * mixins are applied (so a Sodium version without it disables the integration instead of failing
 * in Mixin).
 */
class SodiumMixinsTest {
    private static final String CONFIG = "shaderbridge-sodium.mixins.json";
    private static final String MIXIN = "Lorg/spongepowered/asm/mixin/Mixin;";
    private static final String INJECT = "Lorg/spongepowered/asm/mixin/injection/Inject;";
    private static final String WRAP_OPERATION = "Lcom/llamalad7/mixinextras/injector/wrapoperation/WrapOperation;";
    private static final String CALLBACK_INFO = "org/spongepowered/asm/mixin/injection/callback/CallbackInfo";
    private static final String CALLBACK_INFO_RETURNABLE = "org/spongepowered/asm/mixin/injection/callback/CallbackInfoReturnable";
    private static final String OPERATION = "com/llamalad7/mixinextras/injector/wrapoperation/Operation";

    private static String resource(String name) throws IOException {
        try (InputStream in = SodiumMixinsTest.class.getResourceAsStream("/" + name)) {
            assertNotNull(in, name);
            return new String(in.readAllBytes(), StandardCharsets.UTF_8);
        }
    }

    private static JsonObject config() throws IOException {
        return JsonParser.parseString(resource(CONFIG)).getAsJsonObject();
    }

    private static List<String> mixinClasses() throws IOException {
        JsonObject config = config();
        String pkg = config.get("package").getAsString().replace('.', '/');
        List<String> out = new ArrayList<>();
        for (JsonElement name : config.getAsJsonArray("client")) {
            out.add(pkg + "/" + name.getAsString());
        }
        return out;
    }

    private static Object value(AnnotationNode annotation, String key) {
        if (annotation.values != null) {
            for (int i = 0; i < annotation.values.size(); i += 2) {
                if (annotation.values.get(i).equals(key)) {
                    return annotation.values.get(i + 1);
                }
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

    private static MethodNode method(ClassNode node, String selector) {
        int paren = selector.indexOf('(');
        return node.methods.stream().filter(m -> m.name.equals(selector.substring(0, paren)) && m.desc.equals(selector.substring(paren)))
            .findFirst().orElse(null);
    }

    private static MethodInsnNode call(MethodNode method, String target) {
        int semicolon = target.indexOf(';');
        String owner = target.substring(1, semicolon);
        String name = target.substring(semicolon + 1, target.indexOf('('));
        String desc = target.substring(target.indexOf('('));
        for (AbstractInsnNode insn : method.instructions) {
            if (insn instanceof MethodInsnNode m && m.owner.equals(owner) && m.name.equals(name) && m.desc.equals(desc)) {
                return m;
            }
        }
        return null;
    }

    @Test
    void theConfigurationIsOptionalAndGatedByThePlugin() throws IOException {
        JsonObject config = config();
        assertFalse(config.get("required").getAsBoolean(), "a missing Sodium must not fail the game");
        assertEquals(SodiumMixinPlugin.class.getName(), config.get("plugin").getAsString());
        assertFalse(SodiumMixinPlugin.class.getName().startsWith("dev.shaderbridge.mixin."), "Mixin forbids loading classes of its packages");
        assertEquals(0, config.getAsJsonObject("injectors").get("defaultRequire").getAsInt());
        JsonObject mod = JsonParser.parseString(resource("fabric.mod.json")).getAsJsonObject();
        List<String> configs = new ArrayList<>();
        mod.getAsJsonArray("mixins").forEach(e -> configs.add(e.getAsString()));
        assertTrue(configs.contains(CONFIG), configs.toString());
        assertTrue(mod.getAsJsonObject("depends").get("sodium") == null, "Sodium stays optional");
    }

    @Test
    void theConfigurationListsEveryMixinOfThePackage() throws IOException, URISyntaxException {
        URL dir = SodiumMixinsTest.class.getResource("/dev/shaderbridge/mixin/sodium");
        assertNotNull(dir);
        Set<String> found = new TreeSet<>();
        try (Stream<Path> files = Files.list(Path.of(dir.toURI()))) {
            for (Path file : files.toList()) {
                String name = file.getFileName().toString();
                if (name.endsWith(".class") && !name.contains("$")) {
                    ClassNode node = SodiumTargetsTest.readClass("dev/shaderbridge/mixin/sodium/" + name.substring(0, name.length() - 6));
                    boolean isMixin = annotations(node.visibleAnnotations, node.invisibleAnnotations).stream().anyMatch(a -> a.desc.equals(MIXIN));
                    if (isMixin) {
                        found.add(node.name);
                    }
                }
            }
        }
        assertEquals(found, new TreeSet<>(mixinClasses()));
    }

    @Test
    void everyInjectionMatchesItsTargetAndIsCheckedBeforehand() throws IOException {
        Set<String> checked = new HashSet<>();
        for (SodiumTargets.Requirement r : SodiumTargets.ALL) {
            switch (r) {
                case SodiumTargets.Method m -> checked.add(m.owner() + "#" + m.selector());
                case SodiumTargets.Call c -> checked.add(c.owner() + "#" + c.selector() + "@" + c.target());
                case SodiumTargets.Field f -> checked.add(f.owner() + "." + f.name());
            }
        }
        int injections = 0;
        for (String mixinClass : mixinClasses()) {
            ClassNode node = SodiumTargetsTest.readClass(mixinClass);
            assertNotNull(node, mixinClass);
            AnnotationNode mixin = annotations(node.visibleAnnotations, node.invisibleAnnotations).stream().filter(a -> a.desc.equals(MIXIN))
                .findFirst().orElseThrow(() -> new AssertionError(mixinClass + " has no @Mixin"));
            @SuppressWarnings("unchecked")
            List<Type> targets = (List<Type>) value(mixin, "value");
            assertNotNull(targets, mixinClass);
            assertEquals(1, targets.size(), mixinClass);
            String owner = targets.getFirst().getInternalName();
            ClassNode target = SodiumTargetsTest.readClass(owner);
            assertNotNull(target, mixinClass + " targets " + owner);
            for (MethodNode handler : node.methods) {
                for (AnnotationNode a : annotations(handler.visibleAnnotations, handler.invisibleAnnotations)) {
                    if (!a.desc.equals(INJECT) && !a.desc.equals(WRAP_OPERATION)) {
                        continue;
                    }
                    String where = mixinClass + "#" + handler.name;
                    @SuppressWarnings("unchecked")
                    List<String> selectors = (List<String>) value(a, "method");
                    assertEquals(1, selectors.size(), where);
                    String selector = selectors.getFirst();
                    MethodNode targetMethod = method(target, selector);
                    assertNotNull(targetMethod, where + ": " + owner + "." + selector);
                    @SuppressWarnings("unchecked")
                    List<AnnotationNode> at = (List<AnnotationNode>) value(a, "at");
                    assertEquals(1, at.size(), where);
                    String point = (String) value(at.getFirst(), "value");
                    String callTarget = (String) value(at.getFirst(), "target");
                    if (point.equals("INVOKE")) {
                        assertNotNull(call(targetMethod, callTarget), where + ": no call to " + callTarget);
                        assertTrue(checked.contains(owner + "#" + selector + "@" + callTarget), where + ": SodiumTargets does not check " + callTarget);
                    } else {
                        assertTrue(point.equals("HEAD") || point.equals("TAIL"), where + ": " + point);
                        assertTrue(checked.contains(owner + "#" + selector), where + ": SodiumTargets does not check " + selector);
                    }
                    if (a.desc.equals(INJECT)) {
                        checkInjectHandler(where, handler, targetMethod);
                    } else {
                        checkWrapHandler(where, handler, call(targetMethod, callTarget));
                    }
                    assertEquals((targetMethod.access & Opcodes.ACC_STATIC) != 0, (handler.access & Opcodes.ACC_STATIC) != 0, where + ": static-ness");
                    injections++;
                }
            }
        }
        assertTrue(injections >= 9, "injections: " + injections);
    }

    private static void checkInjectHandler(String where, MethodNode handler, MethodNode target) {
        Type[] handlerArgs = Type.getArgumentTypes(handler.desc);
        Type[] targetArgs = Type.getArgumentTypes(target.desc);
        String callback = handlerArgs[handlerArgs.length - 1].getInternalName();
        boolean returns = Type.getReturnType(target.desc) != Type.VOID_TYPE;
        assertEquals(returns ? CALLBACK_INFO_RETURNABLE : CALLBACK_INFO, callback, where + ": callback type");
        if (handlerArgs.length > 1) {
            assertEquals(targetArgs.length + 1, handlerArgs.length, where + ": the target's parameters and the callback info");
            for (int i = 0; i < targetArgs.length; i++) {
                assertEquals(targetArgs[i], handlerArgs[i], where + ": parameter " + i);
            }
        }
        assertEquals(Type.VOID_TYPE, Type.getReturnType(handler.desc), where);
    }

    private static void checkWrapHandler(String where, MethodNode handler, MethodInsnNode call) {
        List<Type> expected = new ArrayList<>();
        if (call.getOpcode() != Opcodes.INVOKESTATIC) {
            expected.add(Type.getObjectType(call.owner));
        }
        expected.addAll(List.of(Type.getArgumentTypes(call.desc)));
        expected.add(Type.getObjectType(OPERATION));
        assertEquals(expected, List.of(Type.getArgumentTypes(handler.desc)), where + ": receiver, call arguments and the operation");
        assertEquals(Type.getReturnType(call.desc), Type.getReturnType(handler.desc), where + ": return type");
    }
}
