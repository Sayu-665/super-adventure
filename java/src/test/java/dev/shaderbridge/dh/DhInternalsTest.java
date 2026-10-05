package dev.shaderbridge.dh;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.util.Arrays;
import java.util.Comparator;
import java.util.List;
import org.junit.jupiter.api.Test;

/**
 * {@link DhInternals} against the Distant Horizons 3.3.4 jar on the test class path: every
 * internal member ShaderBridge reflects on exists with the expected type, the renderer interfaces
 * have exactly the one abstract {@code render} method the stand-ins replace, and a class path
 * without Distant Horizons fails resolution with the missing name instead of failing later.
 */
class DhInternalsTest {
    private static List<Method> abstractMethods(Class<?> type) {
        return Arrays.stream(type.getMethods()).filter(m -> Modifier.isAbstract(m.getModifiers())).toList();
    }

    @Test
    void resolvesAgainstDistantHorizons334() throws ReflectiveOperationException {
        DhInternals dh = DhInternals.resolve(DhInternalsTest.class.getClassLoader());
        assertEquals(4, renderParameters(dh.terrainRendererInterface), "render(RenderParams, boolean opaquePass, SortedArraySet, IProfilerWrapper)");
        assertEquals(1, renderParameters(dh.farFadeRendererInterface));
        assertEquals(1, renderParameters(dh.antiAliasRendererInterface));
        assertEquals(1, renderParameters(dh.vanillaFadeRendererInterface));
        assertEquals(boolean.class, abstractMethods(dh.terrainRendererInterface).getFirst().getParameterTypes()[1]);
        assertTrue(Modifier.isPrivate(dh.terrainRenderer.getModifiers()));
        assertEquals(dh.terrainRendererInterface, dh.terrainRenderer.getType());
        assertEquals(dh.farFadeRendererInterface, dh.farFadeRenderer.getType());
        assertEquals(dh.antiAliasRendererInterface, dh.antiAliasRenderer.getType());
    }

    @Test
    void readsDistantHorizonsRenderListThroughTheHandles() throws ReflectiveOperationException {
        DhInternals dh = DhInternals.resolve(DhInternalsTest.class.getClassLoader());
        // SortedArraySet has no static state, so a real one can be built without starting Distant Horizons.
        Class<?> setClass = Class.forName("com.seibel.distanthorizons.core.util.objects.SortedArraySet", false, DhInternalsTest.class.getClassLoader());
        Object set = setClass.getConstructor(Comparator.class).newInstance(Comparator.naturalOrder());
        Method add = setClass.getMethod("add", Object.class);
        add.invoke(set, "b");
        add.invoke(set, "a");
        assertEquals(2, dh.size(set));
        assertEquals(List.of("b", "a"), List.of(dh.container(set, 0), dh.container(set, 1)), "insertion order (Distant Horizons' near-to-far order)");
    }

    /** The parameter count of a renderer interface's only abstract method, which must be {@code render}. */
    private static int renderParameters(Class<?> renderer) {
        assertTrue(renderer.isInterface(), renderer.getName());
        List<Method> methods = abstractMethods(renderer);
        assertEquals(1, methods.size(), renderer.getName() + " has one abstract method");
        assertEquals("render", methods.getFirst().getName());
        return methods.getFirst().getParameterCount();
    }

    @Test
    void genericRendererHandlesResolveAgainstDistantHorizons334() throws ReflectiveOperationException {
        DhGenericHandles generic = DhGenericHandles.resolve(DhInternalsTest.class.getClassLoader());
        assertTrue(!generic.isParams("not render params"));
        Class<?> renderer = Class.forName("com.seibel.distanthorizons.core.wrapperInterfaces.render.renderPass.IDhGenericRenderer", false,
            DhInternalsTest.class.getClassLoader());
        Method render = renderer.getMethod("render", Class.forName("com.seibel.distanthorizons.core.render.RenderParams", false,
            DhInternalsTest.class.getClassLoader()), Class.forName("com.seibel.distanthorizons.core.wrapperInterfaces.minecraft.IProfilerWrapper", false,
            DhInternalsTest.class.getClassLoader()), boolean.class);
        assertEquals(void.class, render.getReturnType());
        assertThrows(ClassNotFoundException.class, () -> DhGenericHandles.resolve(ClassLoader.getPlatformClassLoader()));
    }

    @Test
    void withoutDistantHorizonsResolutionNamesTheMissingClass() {
        ClassNotFoundException e = assertThrows(ClassNotFoundException.class, () -> DhInternals.resolve(ClassLoader.getPlatformClassLoader()));
        assertTrue(e.getMessage().contains("LodRenderer"), e.getMessage());
    }
}
