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
import org.objectweb.asm.tree.AbstractInsnNode;
import org.objectweb.asm.tree.AnnotationNode;
import org.objectweb.asm.tree.ClassNode;
import org.objectweb.asm.tree.MethodInsnNode;
import org.objectweb.asm.tree.MethodNode;

/**
 * Verifies what the other mixin tests do not cover, against the Minecraft 26.3 classes on the test
 * class path, from the class files as Mixin reads them:
 *
 * <ul>
 *   <li>{@code @ModifyExpressionValue} at {@code INVOKE}: the target method calls the {@code @At}
 *   target exactly once, and the handler takes and returns the call's result type;</li>
 *   <li>the main pass takeover: every call ShaderBridge wraps in the body of
 *   {@code LevelRenderer.addMainPass} ({@code lambda$addMainPass$0}) is made there exactly once,
 *   with {@code executeSolid} before {@code executeClassicTransparency}, both after the render
 *   pass is created, as the takeover assumes.</li>
 * </ul>
 */
class ExpressionMixinsTest {
    private static final String MIXIN = "Lorg/spongepowered/asm/mixin/Mixin;";
    private static final String MODIFY_EXPRESSION_VALUE = "Lcom/llamalad7/mixinextras/injector/ModifyExpressionValue;";

    private record Mixin(String name, ClassNode node, ClassNode target) {
    }

    private static List<Mixin> mixins() throws IOException {
        JsonObject config;
        try (InputStream in = ExpressionMixinsTest.class.getResourceAsStream("/shaderbridge.mixins.json")) {
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
            out.add(new Mixin(name.getAsString(), node, read(targets.getFirst().getClassName(), 0)));
        }
        return out;
    }

    private static ClassNode read(String className, int flags) throws IOException {
        try (InputStream in = ExpressionMixinsTest.class.getResourceAsStream("/" + className.replace('.', '/') + ".class")) {
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

    private static MethodNode method(ClassNode target, String selector, String where) {
        int paren = selector.indexOf('(');
        String name = paren < 0 ? selector : selector.substring(0, paren);
        String desc = paren < 0 ? null : selector.substring(paren);
        List<MethodNode> matches = target.methods.stream().filter(m -> m.name.equals(name) && (desc == null || m.desc.equals(desc))).toList();
        assertEquals(1, matches.size(), where + ": " + selector + " must name exactly one method of " + target.name);
        return matches.getFirst();
    }

    /** The positions in a method's code of the calls of {@code Lowner;name(desc)ret}. */
    private static List<Integer> calls(MethodNode method, String target) {
        int semicolon = target.indexOf(';');
        int paren = target.indexOf('(');
        String owner = target.substring(1, semicolon);
        String name = target.substring(semicolon + 1, paren);
        String desc = target.substring(paren);
        List<Integer> out = new ArrayList<>();
        int index = 0;
        for (AbstractInsnNode insn : method.instructions) {
            if (insn instanceof MethodInsnNode call && call.owner.equals(owner) && call.name.equals(name) && call.desc.equals(desc)) {
                out.add(index);
            }
            index++;
        }
        return out;
    }

    @Test
    void expressionModifiersFitTheirCalls() throws IOException {
        int checked = 0;
        for (Mixin m : mixins()) {
            for (MethodNode handler : m.node().methods) {
                AnnotationNode modify = annotation(handler.visibleAnnotations, handler.invisibleAnnotations, MODIFY_EXPRESSION_VALUE);
                if (modify == null) {
                    continue;
                }
                String where = m.name() + "#" + handler.name;
                Object atValue = value(modify, "at");
                AnnotationNode at = atValue instanceof List<?> list ? (AnnotationNode) list.getFirst() : (AnnotationNode) atValue;
                assertEquals("INVOKE", value(at, "value"), where);
                String target = (String) value(at, "target");
                Type result = Type.getReturnType(target.substring(target.indexOf('(')));
                @SuppressWarnings("unchecked")
                List<String> selectors = (List<String>) value(modify, "method");
                for (String selector : selectors) {
                    assertEquals(1, calls(method(m.target(), selector, where), target).size(), where + ": one call of " + target);
                }
                assertEquals(List.of(result), Arrays.asList(Type.getArgumentTypes(handler.desc)), where + ": takes the call's result");
                assertEquals(result, Type.getReturnType(handler.desc), where + ": returns it");
                assertEquals(0, handler.access & Opcodes.ACC_STATIC, where + ": the target method is an instance method");
                checked++;
            }
        }
        assertTrue(checked >= 1, "expression modifiers checked: " + checked);
    }

    @Test
    void theMainPassBodyMakesTheWrappedCallsInOrder() throws IOException {
        ClassNode level = read("net.minecraft.client.renderer.LevelRenderer", 0);
        MethodNode body = method(level, "lambda$addMainPass$0", "main pass body");
        List<Integer> create = calls(body, "Lcom/mojang/renderpearl/api/commands/CommandEncoder;createRenderPass(Ljava/util/function/Supplier;"
            + "Lcom/mojang/renderpearl/api/textures/GpuTextureView;Ljava/util/Optional;Lcom/mojang/renderpearl/api/textures/GpuTextureView;"
            + "Ljava/util/OptionalDouble;)Lcom/mojang/renderpearl/api/commands/RenderPass;");
        String args = "(Lnet/minecraft/client/renderer/chunk/ChunkSectionsToRender;Lnet/minecraft/client/renderer/feature/FeatureRenderDispatcher$PreparedFrame;"
            + "Lcom/mojang/renderpearl/api/commands/RenderPass;)V";
        List<Integer> solid = calls(body, "Lnet/minecraft/client/renderer/LevelRenderer;executeSolid" + args);
        List<Integer> translucent = calls(body, "Lnet/minecraft/client/renderer/LevelRenderer;executeClassicTransparency" + args);
        assertEquals(1, create.size());
        assertEquals(1, solid.size());
        assertEquals(1, translucent.size());
        assertTrue(create.getFirst() < solid.getFirst() && solid.getFirst() < translucent.getFirst(),
            "the pass is created, then the opaque geometry, then the translucent geometry is drawn");
        List<Integer> outline = calls(body, "Lnet/minecraft/client/renderer/LevelRenderer;executeOutline("
            + "Lnet/minecraft/client/renderer/feature/FeatureRenderDispatcher$PreparedFrame;)V");
        assertEquals(1, outline.size());
        assertTrue(translucent.getFirst() < outline.getFirst(), "outlines are drawn after the pack frame ended, into Minecraft's target");
        ClassNode sky = read("net.minecraft.client.renderer.LevelRenderer", 0);
        assertEquals(1, calls(method(sky, "lambda$addSkyPass$0", "sky pass body"), "Lnet/minecraft/client/renderer/SkyRenderer;render("
            + "Lcom/mojang/renderpearl/api/buffers/GpuBufferSlice;Lnet/minecraft/client/renderer/state/level/SkyRenderState;)V").size());
    }
}
