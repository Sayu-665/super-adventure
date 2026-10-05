package dev.shaderbridge.dh;

import java.lang.invoke.MethodHandle;
import java.lang.invoke.MethodHandles;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;

/**
 * Reflective handles on Distant Horizons' generic object renderer (beacon beams, clouds, objects
 * added through its API), resolved separately from {@link DhInternals} so that LOD terrain keeps
 * working if these members change:
 *
 * <ul>
 *   <li>{@code RenderParams.genericRenderer}, the public field holding the level's
 *   {@code IDhGenericRenderer} for the frame (set by {@code RenderParams.update} before
 *   {@code LodRenderer} renders);</li>
 *   <li>{@code IDhGenericRenderer.render(RenderParams, IProfilerWrapper, boolean)}, which draws the
 *   box groups of one SSAO set, each in a render pass of its own.</li>
 * </ul>
 */
final class DhGenericHandles {
    private static final String CORE = "com.seibel.distanthorizons.core.";

    private final Class<?> paramsClass;
    private final MethodHandle rendererOf;
    private final MethodHandle render;

    private DhGenericHandles(ClassLoader loader) throws ReflectiveOperationException {
        paramsClass = Class.forName(CORE + "render.RenderParams", false, loader);
        Class<?> rendererClass = Class.forName(CORE + "wrapperInterfaces.render.renderPass.IDhGenericRenderer", false, loader);
        Class<?> profilerClass = Class.forName(CORE + "wrapperInterfaces.minecraft.IProfilerWrapper", false, loader);
        Field field = paramsClass.getField("genericRenderer");
        if (field.getType() != rendererClass || Modifier.isStatic(field.getModifiers())) {
            throw new NoSuchFieldException(paramsClass.getName() + ".genericRenderer is not an instance field of type " + rendererClass.getName());
        }
        Method method = rendererClass.getMethod("render", paramsClass, profilerClass, boolean.class);
        if (method.getReturnType() != void.class) {
            throw new NoSuchMethodException(rendererClass.getName() + ".render does not return void");
        }
        MethodHandles.Lookup lookup = MethodHandles.publicLookup();
        rendererOf = lookup.unreflectGetter(field).asType(java.lang.invoke.MethodType.methodType(Object.class, Object.class));
        render = lookup.unreflect(method).asType(java.lang.invoke.MethodType.methodType(void.class, Object.class, Object.class, Object.class,
            boolean.class));
    }

    /**
     * @param loader the class loader Distant Horizons' classes are visible from
     * @return the handles
     * @throws ReflectiveOperationException naming the first member that is missing or has another type
     */
    static DhGenericHandles resolve(ClassLoader loader) throws ReflectiveOperationException {
        return new DhGenericHandles(loader);
    }

    /**
     * @param params an object passed to the terrain renderer
     * @return whether it is Distant Horizons' {@code RenderParams}
     */
    boolean isParams(Object params) {
        return paramsClass.isInstance(params);
    }

    /**
     * @param params a {@code RenderParams}
     * @return its generic renderer, or null
     */
    Object renderer(Object params) {
        try {
            return (Object) rendererOf.invokeExact(params);
        } catch (Throwable t) {
            throw failure(t);
        }
    }

    /**
     * Calls {@code IDhGenericRenderer.render}.
     *
     * @param renderer the generic renderer
     * @param params   the frame's {@code RenderParams}
     * @param profiler the frame's {@code IProfilerWrapper}
     * @param ssao     the box groups rendered before Distant Horizons' SSAO pass, else the others
     */
    void render(Object renderer, Object params, Object profiler, boolean ssao) {
        try {
            render.invokeExact(renderer, params, profiler, ssao);
        } catch (Throwable t) {
            throw failure(t);
        }
    }

    private static RuntimeException failure(Throwable t) {
        if (t instanceof Error e) {
            throw e;
        }
        return t instanceof RuntimeException r ? r : new IllegalStateException("Distant Horizons generic renderer call failed: " + t, t);
    }
}
