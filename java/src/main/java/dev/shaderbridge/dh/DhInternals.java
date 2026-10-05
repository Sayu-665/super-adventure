package dev.shaderbridge.dh;

import com.mojang.renderpearl.api.buffers.GpuBuffer;
import com.mojang.renderpearl.api.textures.GpuSampler;
import com.mojang.renderpearl.api.textures.GpuTextureView;
import java.lang.invoke.MethodHandle;
import java.lang.invoke.MethodHandles;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;

/**
 * Reflective handles on the Distant Horizons internals ShaderBridge relies on. None of this is
 * Distant Horizons API, so everything is resolved by name at runtime and checked once: the same
 * members exist in every Distant Horizons release from 3.1.0 to 3.3.4
 * ({@code research-Distant-Horizons-hook-for-drawing-DH-s-B.md}), and a release that changed them
 * makes {@link #resolve} fail with the missing member's name instead of crashing later. Resolution
 * does not initialize any Distant Horizons class.
 *
 * <ul>
 *   <li>{@code LodRenderer.INSTANCE} and its private {@code terrainRenderer},
 *   {@code farFadeRenderer} and {@code antiAliasRenderer} fields (filled on Distant Horizons'
 *   first frame);</li>
 *   <li>the renderer interfaces those fields hold, and {@code IDhVanillaFadeRenderer}, which
 *   Distant Horizons looks up in {@code SingletonInjector} on every use;</li>
 *   <li>the render list: {@code SortedArraySet.size()/get(int)}, {@code LodBufferContainer}'s
 *   public {@code minCornerBlockPos}, {@code pos}, {@code vboOpaqueWrappers} and
 *   {@code vboTransparentWrappers}, {@code DhBlockPos.getX/Y/Z()},
 *   {@code DhSectionPos.getBlockWidth(long)} and {@code BlazeVertexBufferWrapper}'s public
 *   {@code vertexGpuBuffer}, {@code vertexCount}, {@code indexCount}, {@code uploaded} and
 *   {@code getIndexGpuBuffer()};</li>
 *   <li>the LOD block atlas: {@code BlazeBlockTextureAtlas.INSTANCE}, {@code uploadPendingTiles()},
 *   {@code getTextureWrapper()} and the wrapper's {@code getTextureView()} /
 *   {@code getTextureSampler()}.</li>
 * </ul>
 */
final class DhInternals {
    private static final String CORE = "com.seibel.distanthorizons.core.";
    private static final String BLAZE = "com.seibel.distanthorizons.common.render.blaze.";
    /** Vertex size of Distant Horizons' LOD buffers. */
    static final int VERTEX_STRIDE = 16;

    final Class<?> terrainRendererInterface;
    final Class<?> farFadeRendererInterface;
    final Class<?> antiAliasRendererInterface;
    final Class<?> vanillaFadeRendererInterface;
    private final MethodHandle lodRenderer;
    final Field terrainRenderer;
    final Field farFadeRenderer;
    final Field antiAliasRenderer;
    private final MethodHandle singletons;
    private final MethodHandle singletonGet;
    private final MethodHandle singletonReplace;
    private final MethodHandle setSize;
    private final MethodHandle setGet;
    private final MethodHandle minCorner;
    private final MethodHandle sectionPos;
    private final MethodHandle opaqueWrappers;
    private final MethodHandle transparentWrappers;
    private final MethodHandle blockX;
    private final MethodHandle blockY;
    private final MethodHandle blockZ;
    private final MethodHandle blockWidth;
    private final Class<?> vertexBufferClass;
    private final MethodHandle vertexBuffer;
    private final MethodHandle vertexCount;
    private final MethodHandle indexCount;
    private final MethodHandle uploaded;
    private final MethodHandle indexBuffer;
    private final MethodHandle atlas;
    private final MethodHandle uploadPendingTiles;
    private final MethodHandle atlasTexture;
    private final MethodHandle textureView;
    private final MethodHandle textureSampler;

    private DhInternals(ClassLoader loader) throws ReflectiveOperationException {
        MethodHandles.Lookup lookup = MethodHandles.lookup();
        Class<?> lodRendererClass = type(loader, CORE + "render.renderer.LodRenderer");
        terrainRendererInterface = type(loader, CORE + "wrapperInterfaces.render.renderPass.IDhTerrainRenderer");
        farFadeRendererInterface = type(loader, CORE + "wrapperInterfaces.render.renderPass.IDhFarFadeRenderer");
        antiAliasRendererInterface = type(loader, CORE + "wrapperInterfaces.render.renderPass.IDhAntiAliasRenderer");
        vanillaFadeRendererInterface = type(loader, CORE + "wrapperInterfaces.render.renderPass.IDhVanillaFadeRenderer");
        lodRenderer = generic(lookup.unreflectGetter(field(lodRendererClass, "INSTANCE", lodRendererClass, true)));
        terrainRenderer = field(lodRendererClass, "terrainRenderer", terrainRendererInterface, false);
        farFadeRenderer = field(lodRendererClass, "farFadeRenderer", farFadeRendererInterface, false);
        antiAliasRenderer = field(lodRendererClass, "antiAliasRenderer", antiAliasRendererInterface, false);
        Class<?> injectorClass = type(loader, CORE + "dependencyInjection.SingletonInjector");
        Class<?> bindable = type(loader, "com.seibel.distanthorizons.coreapi.interfaces.dependencyInjection.IBindable");
        singletons = generic(lookup.unreflectGetter(field(injectorClass, "INSTANCE", injectorClass, true)));
        singletonGet = generic(lookup.unreflect(method(injectorClass, "get", bindable, Class.class)));
        singletonReplace = generic(lookup.unreflect(method(injectorClass, "replaceBinding", void.class, Class.class, bindable)));
        Method render = method(terrainRendererInterface, "render", void.class, type(loader, CORE + "render.RenderParams"), boolean.class,
            type(loader, CORE + "util.objects.SortedArraySet"), type(loader, CORE + "wrapperInterfaces.minecraft.IProfilerWrapper"));
        Class<?> setClass = render.getParameterTypes()[2];
        setSize = generic(lookup.unreflect(method(setClass, "size", int.class)));
        setGet = generic(lookup.unreflect(method(setClass, "get", Object.class, int.class)));
        Class<?> containerClass = type(loader, CORE + "dataObjects.render.bufferBuilding.LodBufferContainer");
        Class<?> blockPosClass = type(loader, CORE + "pos.blockPos.DhBlockPos");
        Class<?> wrapperArray = type(loader, CORE + "wrapperInterfaces.render.objects.IVertexBufferWrapper").arrayType();
        minCorner = generic(lookup.unreflectGetter(field(containerClass, "minCornerBlockPos", blockPosClass, true)));
        sectionPos = generic(lookup.unreflectGetter(field(containerClass, "pos", long.class, true)));
        opaqueWrappers = generic(lookup.unreflectGetter(field(containerClass, "vboOpaqueWrappers", wrapperArray, true)));
        transparentWrappers = generic(lookup.unreflectGetter(field(containerClass, "vboTransparentWrappers", wrapperArray, true)));
        blockX = generic(lookup.unreflect(method(blockPosClass, "getX", int.class)));
        blockY = generic(lookup.unreflect(method(blockPosClass, "getY", int.class)));
        blockZ = generic(lookup.unreflect(method(blockPosClass, "getZ", int.class)));
        blockWidth = generic(lookup.unreflect(method(type(loader, CORE + "pos.DhSectionPos"), "getBlockWidth", int.class, long.class)));
        vertexBufferClass = type(loader, BLAZE + "wrappers.buffer.BlazeVertexBufferWrapper");
        vertexBuffer = generic(lookup.unreflectGetter(field(vertexBufferClass, "vertexGpuBuffer", GpuBuffer.class, true)));
        vertexCount = generic(lookup.unreflectGetter(field(vertexBufferClass, "vertexCount", int.class, true)));
        indexCount = generic(lookup.unreflectGetter(field(vertexBufferClass, "indexCount", int.class, true)));
        uploaded = generic(lookup.unreflectGetter(field(vertexBufferClass, "uploaded", boolean.class, true)));
        indexBuffer = generic(lookup.unreflect(method(vertexBufferClass, "getIndexGpuBuffer", GpuBuffer.class)));
        Class<?> atlasClass = type(loader, BLAZE + "wrappers.texture.BlazeBlockTextureAtlas");
        Class<?> textureClass = type(loader, BLAZE + "wrappers.texture.BlazeTextureWrapper");
        atlas = generic(lookup.unreflectGetter(field(atlasClass, "INSTANCE", atlasClass, true)));
        uploadPendingTiles = generic(lookup.unreflect(method(atlasClass, "uploadPendingTiles", void.class)));
        atlasTexture = generic(lookup.unreflect(method(atlasClass, "getTextureWrapper", textureClass)));
        textureView = generic(lookup.unreflect(method(textureClass, "getTextureView", GpuTextureView.class)));
        textureSampler = generic(lookup.unreflect(method(textureClass, "getTextureSampler", GpuSampler.class)));
    }

    /**
     * @param loader the class loader Distant Horizons' classes are visible from
     * @return the handles
     * @throws ReflectiveOperationException naming the first member that is missing or has another type
     */
    static DhInternals resolve(ClassLoader loader) throws ReflectiveOperationException {
        return new DhInternals(loader);
    }

    // ------------------------------------------------------------------ renderers

    /** @return {@code LodRenderer.INSTANCE} (initializes Distant Horizons' renderer class) */
    Object lodRenderer() {
        return call(lodRenderer);
    }

    /**
     * @param type a renderer interface Distant Horizons binds as a singleton
     * @return the bound renderer, or null
     */
    Object singleton(Class<?> type) {
        return call(singletonGet, call(singletons), type);
    }

    /**
     * @param type     a renderer interface Distant Horizons binds as a singleton
     * @param renderer the renderer to bind instead of the current one
     */
    void replaceSingleton(Class<?> type, Object renderer) {
        call(singletonReplace, call(singletons), type, renderer);
    }

    // ------------------------------------------------------------------ render list

    /**
     * @param set a {@code SortedArraySet<LodBufferContainer>}
     * @return its size
     */
    int size(Object set) {
        return (int) call(setSize, set);
    }

    /**
     * @param set   a {@code SortedArraySet<LodBufferContainer>}
     * @param index an index below {@link #size}
     * @return the container
     */
    Object container(Object set, int index) {
        return call(setGet, set, index);
    }

    /**
     * @param container a {@code LodBufferContainer}
     * @return its minimum corner: x, y, z and the section width in blocks
     */
    int[] section(Object container) {
        Object corner = call(minCorner, container);
        int width = (int) call(blockWidth, call(sectionPos, container));
        return new int[] {(int) call(blockX, corner), (int) call(blockY, corner), (int) call(blockZ, corner), width};
    }

    /**
     * @param container a {@code LodBufferContainer}
     * @param opaque    the opaque buffers, else the translucent ones
     * @return its vertex buffer wrappers (entries may be null)
     */
    Object[] wrappers(Object container, boolean opaque) {
        Object[] wrappers = (Object[]) call(opaque ? opaqueWrappers : transparentWrappers, container);
        return wrappers == null ? new Object[0] : wrappers;
    }

    /**
     * Reads a vertex buffer wrapper.
     *
     * @param wrapper a vertex buffer wrapper of the Blaze3D renderer
     * @param x       section minimum X
     * @param y       section minimum Y
     * @param z       section minimum Z
     * @param width   section width
     * @return the buffer, or null if it is empty, not uploaded or not a Blaze3D buffer
     * @throws IllegalStateException if its vertices are not {@value #VERTEX_STRIDE} bytes apart
     */
    LodBuffer buffer(Object wrapper, int x, int y, int z, int width) {
        if (!vertexBufferClass.isInstance(wrapper) || !(boolean) call(uploaded, wrapper)) {
            return null;
        }
        int vertices = (int) call(vertexCount, wrapper);
        GpuBuffer vbo = (GpuBuffer) call(vertexBuffer, wrapper);
        GpuBuffer ibo = (GpuBuffer) call(indexBuffer, wrapper);
        if (vertices <= 0 || vbo == null || ibo == null) {
            return null;
        }
        if (vbo.size() / vertices != VERTEX_STRIDE) {
            throw new IllegalStateException("Distant Horizons LOD vertices are " + vbo.size() / vertices + " bytes, " + VERTEX_STRIDE + " expected");
        }
        return new LodBuffer(x, y, z, width, vbo, ibo, (int) call(indexCount, wrapper));
    }

    // ------------------------------------------------------------------ block atlas

    /** Uploads the block textures Distant Horizons queued (its terrain renderer would). */
    void uploadAtlasTiles() {
        call(uploadPendingTiles, call(atlas));
    }

    /** @return the LOD block atlas view, or null before it exists */
    GpuTextureView atlasView() {
        return (GpuTextureView) call(textureView, call(atlasTexture, call(atlas)));
    }

    /** @return the LOD block atlas sampler, or null before it exists */
    GpuSampler atlasSampler() {
        return (GpuSampler) call(textureSampler, call(atlasTexture, call(atlas)));
    }

    // ------------------------------------------------------------------ helpers

    /** Adapts a handle to all-{@code Object} parameters and result, so that the calls below are exact invocations. */
    private static MethodHandle generic(MethodHandle handle) {
        return handle.asType(handle.type().generic());
    }

    private static Object call(MethodHandle handle) {
        try {
            return (Object) handle.invokeExact();
        } catch (Throwable t) {
            throw failure(t);
        }
    }

    private static Object call(MethodHandle handle, Object a) {
        try {
            return (Object) handle.invokeExact(a);
        } catch (Throwable t) {
            throw failure(t);
        }
    }

    private static Object call(MethodHandle handle, Object a, Object b) {
        try {
            return (Object) handle.invokeExact(a, b);
        } catch (Throwable t) {
            throw failure(t);
        }
    }

    private static Object call(MethodHandle handle, Object a, Object b, Object c) {
        try {
            return (Object) handle.invokeExact(a, b, c);
        } catch (Throwable t) {
            throw failure(t);
        }
    }

    private static RuntimeException failure(Throwable t) {
        if (t instanceof Error e) {
            throw e;
        }
        return t instanceof RuntimeException r ? r : new IllegalStateException("Distant Horizons call failed: " + t, t);
    }

    private static Class<?> type(ClassLoader loader, String name) throws ClassNotFoundException {
        return Class.forName(name, false, loader);
    }

    private static Field field(Class<?> owner, String name, Class<?> type, boolean isPublic) throws NoSuchFieldException {
        Field f = owner.getDeclaredField(name);
        if (f.getType() != type || Modifier.isPublic(f.getModifiers()) != isPublic) {
            throw new NoSuchFieldException(owner.getName() + "." + name + " is not a " + (isPublic ? "public " : "private ") + type.getName());
        }
        f.setAccessible(true);
        return f;
    }

    private static Method method(Class<?> owner, String name, Class<?> returnType, Class<?>... parameters) throws NoSuchMethodException {
        Method m = owner.getMethod(name, parameters);
        if (m.getReturnType() != returnType) {
            throw new NoSuchMethodException(owner.getName() + "." + name + " returns " + m.getReturnType().getName() + ", not " + returnType.getName());
        }
        return m;
    }
}
