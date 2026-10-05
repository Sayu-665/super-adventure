package dev.shaderbridge;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import org.junit.jupiter.api.Test;
import org.objectweb.asm.ClassReader;
import org.objectweb.asm.Opcodes;
import org.objectweb.asm.Type;
import org.objectweb.asm.tree.AnnotationNode;
import org.objectweb.asm.tree.ClassNode;
import org.objectweb.asm.tree.MethodNode;

/**
 * Verifies the MixinExtras injectors that {@link MixinTargetsTest} and {@link MixinMembersTest} do
 * not cover against the Minecraft 26.3 classes, from the class files as Mixin reads them:
 *
 * <ul>
 *   <li>{@code @WrapMethod}: the selector names exactly one method, the handler takes its
 *   parameters plus the {@code Operation} and returns its type, and is static exactly when the
 *   target is;</li>
 *   <li>{@code @ModifyReturnValue}: at {@code RETURN} of exactly one method, the handler takes and
 *   returns the method's return type, static exactly when the target is.</li>
 * </ul>
 */
class RawMixinsTest {
    private static final String MIXIN = "Lorg/spongepowered/asm/mixin/Mixin;";
    private static final String WRAP_METHOD = "Lcom/llamalad7/mixinextras/injector/wrapmethod/WrapMethod;";
    private static final String MODIFY_RETURN_VALUE = "Lcom/llamalad7/mixinextras/injector/ModifyReturnValue;";
    private static final Type OPERATION = Type.getObjectType("com/llamalad7/mixinextras/injector/wrapoperation/Operation");

    private record Mixin(String name, ClassNode node, ClassNode target) {
    }

    private static List<Mixin> mixins() throws IOException {
        JsonObject config;
        try (InputStream in = RawMixinsTest.class.getResourceAsStream("/shaderbridge.mixins.json")) {
            assertNotNull(in);
            config = JsonParser.parseString(new String(in.readAllBytes(), StandardCharsets.UTF_8)).getAsJsonObject();
        }
        String pkg = config.get("package").getAsString();
        List<Mixin> out = new ArrayList<>();
        for (JsonElement name : config.getAsJsonArray("client")) {
            ClassNode node = read(pkg + "." + name.getAsString());
            AnnotationNode mixin = annotation(node.visibleAnnotations, node.invisibleAnnotations, MIXIN);
            assertNotNull(mixin, name + " has no @Mixin");
            @SuppressWarnings("unchecked")
            List<Type> targets = (List<Type>) value(mixin, "value");
            out.add(new Mixin(name.getAsString(), node, read(targets.getFirst().getClassName())));
        }
        return out;
    }

    private static ClassNode read(String className) throws IOException {
        try (InputStream in = RawMixinsTest.class.getResourceAsStream("/" + className.replace('.', '/') + ".class")) {
            assertNotNull(in, className);
            ClassNode node = new ClassNode();
            new ClassReader(in.readAllBytes()).accept(node, ClassReader.SKIP_CODE);
            return node;
        }
    }

    private static AnnotationNode annotation(List<AnnotationNode> visible, List<AnnotationNode> invisible, String desc) {
        List<AnnotationNode> all = new ArrayList<>();
        if (visible != null) {
            all.addAll(visible);
        }
        if (invisible != null) {
            all.addAll(invisible);
        }
        return all.stream().filter(a -> a.desc.equals(desc)).findFirst().orElse(null);
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

    private static MethodNode method(ClassNode target, String selector, String where) {
        int paren = selector.indexOf('(');
        String name = paren < 0 ? selector : selector.substring(0, paren);
        String desc = paren < 0 ? null : selector.substring(paren);
        List<MethodNode> matches = target.methods.stream().filter(m -> m.name.equals(name) && (desc == null || m.desc.equals(desc))).toList();
        assertEquals(1, matches.size(), where + ": " + selector + " must name exactly one method of " + target.name);
        return matches.getFirst();
    }

    private static boolean isStatic(MethodNode method) {
        return (method.access & Opcodes.ACC_STATIC) != 0;
    }

    @SuppressWarnings("unchecked")
    private static List<String> selectors(AnnotationNode injector) {
        return (List<String>) value(injector, "method");
    }

    @Test
    void methodWrappersFitTheirMethods() throws IOException {
        int checked = 0;
        for (Mixin m : mixins()) {
            for (MethodNode handler : m.node().methods) {
                AnnotationNode wrap = annotation(handler.visibleAnnotations, handler.invisibleAnnotations, WRAP_METHOD);
                if (wrap == null) {
                    continue;
                }
                String where = m.name() + "#" + handler.name;
                for (String selector : selectors(wrap)) {
                    MethodNode target = method(m.target(), selector, where);
                    List<Type> expected = new ArrayList<>(Arrays.asList(Type.getArgumentTypes(target.desc)));
                    expected.add(OPERATION);
                    assertEquals(expected, Arrays.asList(Type.getArgumentTypes(handler.desc)), where + ": the method's parameters, then the operation");
                    assertEquals(Type.getReturnType(target.desc), Type.getReturnType(handler.desc), where + ": return type");
                    assertEquals(isStatic(target), isStatic(handler), where + ": static like the target");
                }
                checked++;
            }
        }
        assertTrue(checked >= 1, "method wrappers checked: " + checked);
    }

    @Test
    void returnValueModifiersFitTheirMethods() throws IOException {
        int checked = 0;
        for (Mixin m : mixins()) {
            for (MethodNode handler : m.node().methods) {
                AnnotationNode modify = annotation(handler.visibleAnnotations, handler.invisibleAnnotations, MODIFY_RETURN_VALUE);
                if (modify == null) {
                    continue;
                }
                String where = m.name() + "#" + handler.name;
                Object at = value(modify, "at");
                AnnotationNode point = at instanceof List<?> list ? (AnnotationNode) list.getFirst() : (AnnotationNode) at;
                assertEquals("RETURN", value(point, "value"), where);
                for (String selector : selectors(modify)) {
                    MethodNode target = method(m.target(), selector, where);
                    Type returned = Type.getReturnType(target.desc);
                    assertEquals(List.of(returned), Arrays.asList(Type.getArgumentTypes(handler.desc)), where + ": takes the return value");
                    assertEquals(returned, Type.getReturnType(handler.desc), where + ": returns it");
                    assertEquals(isStatic(target), isStatic(handler), where + ": static like the target");
                }
                checked++;
            }
        }
        assertTrue(checked >= 1, "return value modifiers checked: " + checked);
    }
}
