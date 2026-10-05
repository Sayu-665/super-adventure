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
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import org.junit.jupiter.api.Test;
import org.objectweb.asm.ClassReader;
import org.objectweb.asm.Opcodes;
import org.objectweb.asm.Type;
import org.objectweb.asm.tree.AbstractInsnNode;
import org.objectweb.asm.tree.AnnotationNode;
import org.objectweb.asm.tree.ClassNode;
import org.objectweb.asm.tree.MethodInsnNode;
import org.objectweb.asm.tree.MethodNode;
import org.objectweb.asm.tree.TypeInsnNode;

/**
 * Verifies the {@code @Redirect} injectors the other mixin tests do not cover against the Minecraft
 * 26.3 classes, from the class files as Mixin reads them. Constructor redirects
 * ({@code @At("NEW")}): the selector names exactly one method, which instantiates the target class
 * exactly once; the handler takes that constructor's parameters, returns the class, and is not
 * static (the targets are instance methods).
 */
class RedirectMixinsTest {
    private static final String MIXIN = "Lorg/spongepowered/asm/mixin/Mixin;";
    private static final String REDIRECT = "Lorg/spongepowered/asm/mixin/injection/Redirect;";

    private record Mixin(String name, ClassNode node, ClassNode target) {
    }

    private static List<Mixin> mixins() throws IOException {
        JsonObject config;
        try (InputStream in = RedirectMixinsTest.class.getResourceAsStream("/shaderbridge.mixins.json")) {
            assertNotNull(in);
            config = JsonParser.parseString(new String(in.readAllBytes(), StandardCharsets.UTF_8)).getAsJsonObject();
        }
        String pkg = config.get("package").getAsString();
        List<Mixin> out = new ArrayList<>();
        for (JsonElement name : config.getAsJsonArray("client")) {
            ClassNode node = read(pkg + "." + name.getAsString(), ClassReader.SKIP_CODE);
            AnnotationNode mixin = annotation(node, MIXIN);
            assertNotNull(mixin, name + " has no @Mixin");
            @SuppressWarnings("unchecked")
            List<Type> targets = (List<Type>) value(mixin, "value");
            out.add(new Mixin(name.getAsString(), node, read(targets.getFirst().getClassName(), 0)));
        }
        return out;
    }

    private static ClassNode read(String className, int flags) throws IOException {
        try (InputStream in = RedirectMixinsTest.class.getResourceAsStream("/" + className.replace('.', '/') + ".class")) {
            assertNotNull(in, className);
            ClassNode node = new ClassNode();
            new ClassReader(in.readAllBytes()).accept(node, flags);
            return node;
        }
    }

    private static AnnotationNode annotation(ClassNode node, String desc) {
        return find(node.visibleAnnotations, node.invisibleAnnotations, desc);
    }

    private static AnnotationNode find(List<AnnotationNode> visible, List<AnnotationNode> invisible, String desc) {
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

    @Test
    void constructorRedirectsFitTheirInstantiations() throws IOException {
        int checked = 0;
        for (Mixin m : mixins()) {
            for (MethodNode handler : m.node().methods) {
                AnnotationNode redirect = find(handler.visibleAnnotations, handler.invisibleAnnotations, REDIRECT);
                if (redirect == null) {
                    continue;
                }
                String where = m.name() + "#" + handler.name;
                Object atValue = value(redirect, "at");
                AnnotationNode at = atValue instanceof List<?> list ? (AnnotationNode) list.getFirst() : (AnnotationNode) atValue;
                assertEquals("NEW", value(at, "value"), where + ": only constructor redirects are verified here");
                String type = ((String) value(at, "target")).replace('.', '/');
                @SuppressWarnings("unchecked")
                List<String> selectors = (List<String>) value(redirect, "method");
                assertFalse(selectors.isEmpty(), where);
                for (String selector : selectors) {
                    MethodNode target = method(m.target(), selector, where);
                    assertEquals(0, target.access & Opcodes.ACC_STATIC, where + ": instance target");
                    List<MethodInsnNode> inits = new ArrayList<>();
                    int news = 0;
                    for (AbstractInsnNode insn : target.instructions) {
                        if (insn instanceof TypeInsnNode t && t.getOpcode() == Opcodes.NEW && t.desc.equals(type)) {
                            news++;
                        }
                        if (insn instanceof MethodInsnNode call && call.getOpcode() == Opcodes.INVOKESPECIAL && call.owner.equals(type)
                            && call.name.equals("<init>")) {
                            inits.add(call);
                        }
                    }
                    assertEquals(1, news, where + ": one instantiation of " + type);
                    assertEquals(1, inits.size(), where + ": one constructor call of " + type);
                    assertEquals(Arrays.asList(Type.getArgumentTypes(inits.getFirst().desc)), Arrays.asList(Type.getArgumentTypes(handler.desc)),
                        where + ": the constructor's parameters");
                    assertEquals(Type.getObjectType(type), Type.getReturnType(handler.desc), where + ": returns the instance");
                    assertEquals(0, handler.access & Opcodes.ACC_STATIC, where + ": instance handler");
                }
                assertTrue(value(redirect, "require") instanceof Integer require && require == 0, where + ": optional");
                checked++;
            }
        }
        assertTrue(checked >= 1, "redirects checked: " + checked);
    }
}
