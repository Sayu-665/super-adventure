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
import org.objectweb.asm.tree.FieldNode;
import org.objectweb.asm.tree.MethodInsnNode;
import org.objectweb.asm.tree.MethodNode;

/**
 * Verifies the mixin members {@link MixinTargetsTest} does not cover against the Minecraft 26.3
 * classes on the test class path, from the class files as Mixin reads them:
 *
 * <ul>
 *   <li>{@code @Invoker} and {@code @Accessor}: the target method (name and descriptor) or field
 *   (name and type) exists;</li>
 *   <li>{@code @Shadow} fields exist with the same type;</li>
 *   <li>{@code @ModifyVariable(argsOnly)}: the target method exists and has exactly one parameter
 *   of the handler's type;</li>
 *   <li>{@code @ModifyArg} and {@code @WrapOperation}: the target method exists, its bytecode
 *   invokes the {@code @At} target exactly once, and the handler's signature fits the call (the
 *   modified argument is the one {@code index} names, or the only one of the handler's type).</li>
 * </ul>
 */
class MixinMembersTest {
    private static final String MIXIN = "Lorg/spongepowered/asm/mixin/Mixin;";
    private static final String INVOKER = "Lorg/spongepowered/asm/mixin/gen/Invoker;";
    private static final String ACCESSOR = "Lorg/spongepowered/asm/mixin/gen/Accessor;";
    private static final String SHADOW = "Lorg/spongepowered/asm/mixin/Shadow;";
    private static final String MODIFY_VARIABLE = "Lorg/spongepowered/asm/mixin/injection/ModifyVariable;";
    private static final String MODIFY_ARG = "Lorg/spongepowered/asm/mixin/injection/ModifyArg;";
    private static final String WRAP_OPERATION = "Lcom/llamalad7/mixinextras/injector/wrapoperation/WrapOperation;";
    private static final String OPERATION = "com/llamalad7/mixinextras/injector/wrapoperation/Operation";

    private record Mixin(String name, ClassNode node, ClassNode target) {
    }

    private static List<Mixin> mixins() throws IOException {
        JsonObject config;
        try (InputStream in = MixinMembersTest.class.getResourceAsStream("/shaderbridge.mixins.json")) {
            assertNotNull(in);
            config = JsonParser.parseString(new String(in.readAllBytes(), StandardCharsets.UTF_8)).getAsJsonObject();
        }
        String pkg = config.get("package").getAsString();
        List<Mixin> out = new ArrayList<>();
        for (JsonElement name : config.getAsJsonArray("client")) {
            ClassNode node = read(pkg + "." + name.getAsString(), ClassReader.SKIP_CODE);
            AnnotationNode mixin = annotation(node.visibleAnnotations, node.invisibleAnnotations, MIXIN);
            assertNotNull(mixin, name + " has no @Mixin");
            @SuppressWarnings("unchecked")
            List<Type> targets = (List<Type>) value(mixin, "value");
            assertEquals(1, targets.size(), name + ": one target class");
            out.add(new Mixin(name.getAsString(), node, read(targets.getFirst().getClassName(), 0)));
        }
        return out;
    }

    private static ClassNode read(String className, int flags) throws IOException {
        try (InputStream in = MixinMembersTest.class.getResourceAsStream("/" + className.replace('.', '/') + ".class")) {
            assertNotNull(in, className);
            ClassNode node = new ClassNode();
            new ClassReader(in.readAllBytes()).accept(node, flags);
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

    /** The single target method a selector ({@code name} or {@code name(desc)ret}) names. */
    private static MethodNode method(ClassNode target, String selector, String where) {
        int paren = selector.indexOf('(');
        String name = paren < 0 ? selector : selector.substring(0, paren);
        String desc = paren < 0 ? null : selector.substring(paren);
        List<MethodNode> matches = target.methods.stream().filter(m -> m.name.equals(name) && (desc == null || m.desc.equals(desc))).toList();
        assertEquals(1, matches.size(), where + ": " + selector + " must name exactly one method of " + target.name);
        return matches.getFirst();
    }

    /** The invocations in a method of an {@code @At(value = "INVOKE")} target {@code Lowner;name(desc)ret}. */
    private static List<MethodInsnNode> invokes(MethodNode method, String target) {
        int semicolon = target.indexOf(';');
        int paren = target.indexOf('(');
        String owner = target.substring(1, semicolon);
        String name = target.substring(semicolon + 1, paren);
        String desc = target.substring(paren);
        List<MethodInsnNode> out = new ArrayList<>();
        for (AbstractInsnNode insn : method.instructions) {
            if (insn instanceof MethodInsnNode call && call.owner.equals(owner) && call.name.equals(name) && call.desc.equals(desc)) {
                out.add(call);
            }
        }
        return out;
    }

    private static AnnotationNode at(Object at) {
        return at instanceof List<?> list ? (AnnotationNode) list.getFirst() : (AnnotationNode) at;
    }

    @Test
    void invokersAndAccessorsNameExistingMembers() throws IOException {
        int checked = 0;
        for (Mixin m : mixins()) {
            for (MethodNode handler : m.node().methods) {
                AnnotationNode invoker = annotation(handler.visibleAnnotations, handler.invisibleAnnotations, INVOKER);
                if (invoker != null) {
                    String where = m.name() + "#" + handler.name;
                    MethodNode target = method(m.target(), (String) value(invoker, "value"), where);
                    assertEquals(target.desc, handler.desc, where + ": the invoker's descriptor is the target's");
                    checked++;
                }
                AnnotationNode accessor = annotation(handler.visibleAnnotations, handler.invisibleAnnotations, ACCESSOR);
                if (accessor != null) {
                    String where = m.name() + "#" + handler.name;
                    String fieldName = (String) value(accessor, "value");
                    FieldNode field = m.target().fields.stream().filter(f -> f.name.equals(fieldName)).findFirst().orElse(null);
                    assertNotNull(field, where + ": field " + fieldName);
                    Type fieldType = Type.getType(field.desc);
                    Type[] args = Type.getArgumentTypes(handler.desc);
                    if (args.length == 0) {
                        assertEquals(fieldType, Type.getReturnType(handler.desc), where + ": getter type");
                    } else {
                        assertEquals(1, args.length, where);
                        assertEquals(fieldType, args[0], where + ": setter type");
                        assertEquals(0, field.access & Opcodes.ACC_FINAL, where + ": a setter needs a non-final field");
                    }
                    checked++;
                }
            }
        }
        assertTrue(checked >= 2, "invokers and accessors checked: " + checked);
    }

    @Test
    void shadowFieldsExist() throws IOException {
        int checked = 0;
        for (Mixin m : mixins()) {
            for (FieldNode field : m.node().fields) {
                if (annotation(field.visibleAnnotations, field.invisibleAnnotations, SHADOW) != null) {
                    FieldNode target = m.target().fields.stream().filter(f -> f.name.equals(field.name)).findFirst().orElse(null);
                    assertNotNull(target, m.name() + ": @Shadow " + field.name);
                    assertEquals(target.desc, field.desc, m.name() + ": @Shadow " + field.name + " type");
                    checked++;
                }
            }
        }
        assertTrue(checked >= 1);
    }

    @Test
    void argumentModifiersFitTheirTargets() throws IOException {
        int checked = 0;
        for (Mixin m : mixins()) {
            for (MethodNode handler : m.node().methods) {
                String where = m.name() + "#" + handler.name;
                AnnotationNode variable = annotation(handler.visibleAnnotations, handler.invisibleAnnotations, MODIFY_VARIABLE);
                if (variable != null) {
                    assertEquals(Boolean.TRUE, value(variable, "argsOnly"), where + ": argsOnly");
                    Type type = Type.getReturnType(handler.desc);
                    assertEquals(List.of(type), Arrays.asList(Type.getArgumentTypes(handler.desc)), where + ": takes and returns the argument");
                    for (String selector : selectors(variable)) {
                        MethodNode target = method(m.target(), selector, where);
                        long same = Arrays.stream(Type.getArgumentTypes(target.desc)).filter(type::equals).count();
                        assertEquals(1, same, where + ": exactly one parameter of type " + type);
                    }
                    checked++;
                }
                AnnotationNode arg = annotation(handler.visibleAnnotations, handler.invisibleAnnotations, MODIFY_ARG);
                if (arg != null) {
                    AnnotationNode at = at(value(arg, "at"));
                    assertEquals("INVOKE", value(at, "value"), where);
                    String invokeTarget = (String) value(at, "target");
                    Type type = Type.getReturnType(handler.desc);
                    assertEquals(List.of(type), Arrays.asList(Type.getArgumentTypes(handler.desc)), where + ": takes and returns the argument");
                    for (String selector : selectors(arg)) {
                        assertEquals(1, invokes(method(m.target(), selector, where), invokeTarget).size(), where + ": one call of " + invokeTarget);
                    }
                    Type[] callArgs = Type.getArgumentTypes(invokeTarget.substring(invokeTarget.indexOf('(')));
                    if (value(arg, "index") instanceof Integer index) {
                        assertTrue(index >= 0 && index < callArgs.length, where + ": argument index " + index + " exists");
                        assertEquals(type, callArgs[index], where + ": argument " + index + " has the handler's type");
                    } else {
                        assertEquals(1, Arrays.stream(callArgs).filter(type::equals).count(), where + ": the call has one argument of type " + type);
                    }
                    checked++;
                }
            }
        }
        assertTrue(checked >= 2, "argument modifiers checked: " + checked);
    }

    @Test
    void operationWrappersFitTheirCalls() throws IOException {
        int checked = 0;
        for (Mixin m : mixins()) {
            for (MethodNode handler : m.node().methods) {
                AnnotationNode wrap = annotation(handler.visibleAnnotations, handler.invisibleAnnotations, WRAP_OPERATION);
                if (wrap == null) {
                    continue;
                }
                String where = m.name() + "#" + handler.name;
                AnnotationNode at = at(value(wrap, "at"));
                assertEquals("INVOKE", value(at, "value"), where);
                String invokeTarget = (String) value(at, "target");
                for (String selector : selectors(wrap)) {
                    List<MethodInsnNode> calls = invokes(method(m.target(), selector, where), invokeTarget);
                    assertEquals(1, calls.size(), where + ": one call of " + invokeTarget);
                    MethodInsnNode call = calls.getFirst();
                    List<Type> expected = new ArrayList<>();
                    if (call.getOpcode() != Opcodes.INVOKESTATIC) {
                        expected.add(Type.getObjectType(call.owner));
                    }
                    expected.addAll(Arrays.asList(Type.getArgumentTypes(call.desc)));
                    expected.add(Type.getObjectType(OPERATION));
                    assertEquals(expected, Arrays.asList(Type.getArgumentTypes(handler.desc)), where + ": receiver, arguments, operation");
                    assertEquals(Type.getReturnType(call.desc), Type.getReturnType(handler.desc), where + ": return type");
                }
                checked++;
            }
        }
        assertTrue(checked >= 1);
    }

    @SuppressWarnings("unchecked")
    private static List<String> selectors(AnnotationNode injector) {
        List<String> selectors = (List<String>) value(injector, "method");
        assertNotNull(selectors);
        assertFalse(selectors.isEmpty());
        return selectors;
    }
}
