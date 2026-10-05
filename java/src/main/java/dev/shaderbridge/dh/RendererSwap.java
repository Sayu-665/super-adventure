package dev.shaderbridge.dh;

import java.lang.reflect.InvocationHandler;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.util.function.Consumer;
import java.util.function.Supplier;

/**
 * Stands in for one of Distant Horizons' internal renderers while a pack renders: a
 * {@link Proxy} of the renderer interface is put where Distant Horizons keeps the renderer (a
 * field of {@code LodRenderer} or a {@code SingletonInjector} binding) and the original is put
 * back afterwards. The proxy runs {@link Render} instead of the interface's {@code render} method
 * and forwards every other method (Distant Horizons' {@code IBindable} setup hooks) to the
 * original renderer.
 */
final class RendererSwap implements InvocationHandler {
    /** What the stand-in does instead of rendering. */
    @FunctionalInterface
    interface Render {
        /** Does nothing (the renderer's output is not wanted while a pack renders). */
        Render NOTHING = args -> { };

        /**
         * @param args the arguments Distant Horizons passed to {@code render}
         */
        void render(Object[] args);
    }

    private final Class<?> type;
    private final Supplier<Object> read;
    private final Consumer<Object> write;
    private final Render render;
    private final Object proxy;
    private Object original;

    /**
     * @param type   the renderer interface
     * @param read   reads where Distant Horizons keeps the renderer
     * @param write  writes it
     * @param render what the stand-in does instead of rendering
     */
    RendererSwap(Class<?> type, Supplier<Object> read, Consumer<Object> write, Render render) {
        this.type = type;
        this.read = read;
        this.write = write;
        this.render = render;
        this.proxy = Proxy.newProxyInstance(type.getClassLoader(), new Class<?>[] {type}, this);
    }

    /**
     * Puts the stand-in in place (idempotent; call every frame). Nothing happens while Distant
     * Horizons has not created the renderer yet. A renderer Distant Horizons (or another mod) put
     * there since becomes the new original.
     */
    void install() {
        Object current = read.get();
        if (current == null || current == proxy) {
            return;
        }
        original = current;
        write.accept(proxy);
    }

    /** Puts the original renderer back, unless something else replaced the stand-in meanwhile. */
    void restore() {
        if (original != null && read.get() == proxy) {
            write.accept(original);
        }
        original = null;
    }

    @Override
    public Object invoke(Object self, Method method, Object[] args) throws Throwable {
        if (method.getDeclaringClass() == Object.class) {
            return switch (method.getName()) {
                case "equals" -> self == args[0];
                case "hashCode" -> System.identityHashCode(self);
                default -> "ShaderBridge stand-in for " + type.getSimpleName();
            };
        }
        if (method.getName().equals("render")) {
            render.render(args == null ? new Object[0] : args);
            return null;
        }
        Object target = original;
        if (target == null) {
            if (method.isDefault()) {
                return InvocationHandler.invokeDefault(self, method, args);
            }
            throw new IllegalStateException(type.getSimpleName() + "." + method.getName() + " called without an original renderer");
        }
        try {
            return method.invoke(target, args);
        } catch (InvocationTargetException e) {
            throw e.getCause();
        }
    }
}
