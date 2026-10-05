package dev.shaderbridge.dh;

import dev.shaderbridge.gui.PackNotifier;
import dev.shaderbridge.render.targets.TextureBinding;
import java.lang.reflect.Field;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;
import java.util.function.Consumer;
import java.util.function.Supplier;
import net.fabricmc.loader.api.FabricLoader;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * ShaderBridge's hook into Distant Horizons (3.3.x on Minecraft 26.3), reflection-based and fail
 * soft. While a pack renders a frame, ShaderBridge draws the LODs itself with the pack's programs,
 * so Distant Horizons must hand them over and draw nothing on screen:
 *
 * <ul>
 *   <li>its terrain renderer ({@code LodRenderer.terrainRenderer}) is replaced by a stand-in that
 *   copies the render list of the opaque call ({@link #lods}) and draws nothing; the translucent
 *   LODs are taken from the same list;</li>
 *   <li>its far fade, temporal anti-aliasing and vanilla fade renderers (no API controls them)
 *   are replaced by stand-ins that draw nothing, and the apply pass, fog, SSAO and frustum culling
 *   are switched off through its API ({@code DhApiControl});</li>
 *   <li>its LOD block atlas keeps being updated, as its terrain renderer would;</li>
 *   <li>its generic objects (beacon beams, clouds, objects added through its API) are not drawn by
 *   Distant Horizons (its generic rendering is switched off through the API); ShaderBridge replays
 *   its generic renderer for the frame at the time the pack draws them ({@link #drawGeneric}).</li>
 * </ul>
 *
 * Everything is put back as soon as a frame renders without a pack. When the LODs cannot be
 * handed over (a Distant Horizons release whose internals differ, or a failure while taking over
 * or reading its render list), the reason is logged and shown to the player once, and Distant
 * Horizons' rendering is switched off through its API while a pack renders, so that its own
 * passes do not blend over the pack's image. Render thread only.
 */
public final class DistantHorizons {
    private static final Logger LOGGER = LoggerFactory.getLogger("ShaderBridge");
    private static DistantHorizons instance;

    private final boolean installed;
    private final DhInternals dh;
    /** Handles on the generic object renderer, or null when its members differ (generic objects are then not drawn). */
    private final DhGenericHandles generic;
    private boolean genericBroken;
    /** The generic renderer call Distant Horizons would have made this frame. */
    private GenericCall genericCall;
    private final List<RendererSwap> swaps = new ArrayList<>();
    private String unavailable;
    /** The stand-ins are in place and Distant Horizons hands its LODs over. */
    private boolean handedOver;
    /** Distant Horizons' rendering is switched off through its API. */
    private boolean switchedOff;
    private long frame;
    private LodFrame lods = LodFrame.EMPTY;

    /**
     * The generic renderer of one frame and the arguments Distant Horizons renders it with.
     *
     * @param frame    the frame ({@link #frame()})
     * @param renderer the level's {@code IDhGenericRenderer}
     * @param params   the frame's {@code RenderParams}
     * @param profiler the frame's {@code IProfilerWrapper}
     */
    private record GenericCall(long frame, Object renderer, Object params, Object profiler) {
    }

    private DistantHorizons(boolean installed, DhInternals dh, DhGenericHandles generic, String unavailable) {
        this.installed = installed;
        this.dh = dh;
        this.generic = generic;
        this.unavailable = unavailable;
        if (dh != null) {
            swaps.add(fieldSwap(dh.terrainRenderer, dh.terrainRendererInterface, this::capture));
            swaps.add(fieldSwap(dh.farFadeRenderer, dh.farFadeRendererInterface, RendererSwap.Render.NOTHING));
            swaps.add(fieldSwap(dh.antiAliasRenderer, dh.antiAliasRendererInterface, RendererSwap.Render.NOTHING));
            Class<?> fade = dh.vanillaFadeRendererInterface;
            swaps.add(new RendererSwap(fade, () -> dh.singleton(fade), r -> dh.replaceSingleton(fade, r), RendererSwap.Render.NOTHING));
        }
    }

    /** @return the integration; created on first use (render thread) */
    public static DistantHorizons get() {
        if (instance == null) {
            instance = create();
        }
        return instance;
    }

    private static DistantHorizons create() {
        if (!FabricLoader.getInstance().isModLoaded("distanthorizons")) {
            return new DistantHorizons(false, null, null, "Distant Horizons is not installed");
        }
        ClassLoader loader = DistantHorizons.class.getClassLoader();
        DhInternals internals;
        try {
            internals = DhInternals.resolve(loader);
        } catch (ReflectiveOperationException | LinkageError e) {
            String reason = "this Distant Horizons version is not supported (" + e.getMessage() + "); LODs are not drawn with shader packs";
            LOGGER.error("Distant Horizons integration unavailable: {}", reason, e);
            PackNotifier.warning(reason);
            return new DistantHorizons(true, null, null, reason);
        }
        DhGenericHandles generic = null;
        try {
            generic = DhGenericHandles.resolve(loader);
        } catch (ReflectiveOperationException | LinkageError e) {
            LOGGER.warn("Distant Horizons' generic objects (beacon beams, clouds) are not drawn with shader packs: {}", e.toString());
        }
        return new DistantHorizons(true, internals, generic, null);
    }

    private RendererSwap fieldSwap(Field field, Class<?> type, RendererSwap.Render render) {
        Supplier<Object> read = () -> {
            try {
                return field.get(dh.lodRenderer());
            } catch (IllegalAccessException e) {
                throw new IllegalStateException(e);
            }
        };
        Consumer<Object> write = value -> {
            try {
                field.set(dh.lodRenderer(), value);
            } catch (IllegalAccessException e) {
                throw new IllegalStateException(e);
            }
        };
        return new RendererSwap(type, read, write, render);
    }

    /**
     * @return Distant Horizons' settings while it renders LODs ShaderBridge can draw, else empty
     */
    public Optional<DhSettings> settings() {
        return unavailable == null ? DhApiControl.settings() : Optional.empty();
    }

    /**
     * Starts a frame: takes Distant Horizons' output over while a pack renders the frame, gives
     * it back otherwise. Call before Distant Horizons renders (start of the level frame).
     *
     * @param packRenders a pack renders this frame (with or without LODs)
     */
    public void beginFrame(boolean packRenders) {
        frame++;
        if (!installed) {
            return;
        }
        try {
            if (!packRenders) {
                giveBack();
            } else if (unavailable != null) {
                if (handedOver) {
                    giveBack();
                }
                switchedOff = DhApiControl.switchOff();
            } else if (DhApiControl.takeOver(generic != null && !genericBroken)) {
                handedOver = true;
                swaps.forEach(RendererSwap::install);
            }
        } catch (RuntimeException e) {
            fail(e);
        }
    }

    /** Gives Distant Horizons' output back (the pack stopped rendering or the world was left); never throws. */
    public void release() {
        try {
            giveBack();
        } catch (RuntimeException e) {
            fail(e);
        }
    }

    /** Restores Distant Horizons' renderers and API settings. */
    private void giveBack() {
        lods = LodFrame.EMPTY;
        genericCall = null;
        if (!handedOver && !switchedOff) {
            return;
        }
        RuntimeException failure = null;
        if (handedOver) {
            for (RendererSwap swap : swaps) {
                try {
                    swap.restore();
                } catch (RuntimeException e) {
                    failure = e;
                }
            }
        }
        handedOver = false;
        switchedOff = false;
        DhApiControl.release();
        if (failure != null) {
            throw failure;
        }
    }

    /**
     * @return the LOD buffers of the latest frame Distant Horizons rendered while taken over; its
     *     {@link LodFrame#frame()} tells whether that was this frame ({@link #frame()})
     */
    public LodFrame lods() {
        return lods;
    }

    /** @return the current frame number (counts {@link #beginFrame} calls) */
    public long frame() {
        return frame;
    }

    /** @return Distant Horizons' LOD block atlas ({@code dhBlockAtlas}), once it exists */
    public Optional<TextureBinding> blockAtlas() {
        if (unavailable != null) {
            return Optional.empty();
        }
        try {
            var view = dh.atlasView();
            var sampler = dh.atlasSampler();
            return view == null || sampler == null ? Optional.empty() : Optional.of(new TextureBinding(view, sampler));
        } catch (RuntimeException e) {
            fail(e);
            return Optional.empty();
        }
    }

    /** The terrain renderer stand-in: copies the render list of the opaque call. */
    private void capture(Object[] args) {
        if (unavailable != null || args.length < 3 || !(args[1] instanceof Boolean opaque) || !opaque) {
            return;
        }
        try {
            dh.uploadAtlasTiles();
            Object set = args[2];
            List<LodBuffer> solid = new ArrayList<>();
            List<LodBuffer> water = new ArrayList<>();
            int count = dh.size(set);
            for (int i = 0; i < count; i++) {
                Object container = dh.container(set, i);
                int[] s = dh.section(container);
                collect(container, true, s, solid);
                collect(container, false, s, water);
            }
            lods = new LodFrame(frame, solid, water);
            captureGeneric(args);
        } catch (RuntimeException e) {
            // Thrown into Distant Horizons, this would make it disable its renderer: stop the LODs instead
            // (from the next frame on, Distant Horizons is given back and switched off while packs render).
            lods = LodFrame.EMPTY;
            report("reading Distant Horizons' LOD buffers failed: " + e, e);
        }
    }

    /** Remembers the generic renderer and the arguments of this frame (Distant Horizons does not call it while ShaderBridge holds its output). */
    private void captureGeneric(Object[] args) {
        genericCall = null;
        if (generic == null || genericBroken || args.length < 4 || !generic.isParams(args[0])) {
            return;
        }
        Object renderer = generic.renderer(args[0]);
        if (renderer != null) {
            genericCall = new GenericCall(frame, renderer, args[0], args[3]);
        }
    }

    /**
     * Draws Distant Horizons' generic objects of this frame (beacon beams, clouds, objects added
     * through its API) by calling its generic renderer as Distant Horizons would: the box groups
     * shaded with SSAO, then the others. Each group opens a render pass of its own, which the
     * caller redirects to the pack's passes. Fails soft: a failure stops generic objects for the
     * rest of the session.
     *
     * @return whether the renderer ran (the player has generic objects on and Distant Horizons
     *     handed this frame's renderer over)
     */
    public boolean drawGeneric() {
        GenericCall call = genericCall;
        if (call == null || call.frame() != frame || genericBroken || unavailable != null || !DhApiControl.genericWanted()) {
            return false;
        }
        try {
            generic.render(call.renderer(), call.params(), call.profiler(), true);
            generic.render(call.renderer(), call.params(), call.profiler(), false);
            return true;
        } catch (RuntimeException e) {
            genericBroken = true;
            genericCall = null;
            LOGGER.error("Drawing Distant Horizons' generic objects failed; they are not drawn with shader packs from now on", e);
            PackNotifier.warning("Distant Horizons' generic objects (beacon beams, clouds) could not be drawn with the shader pack: " + e);
            return false;
        }
    }

    private void collect(Object container, boolean opaque, int[] section, List<LodBuffer> out) {
        for (Object wrapper : dh.wrappers(container, opaque)) {
            LodBuffer b = wrapper == null ? null : dh.buffer(wrapper, section[0], section[1], section[2], section[3]);
            if (b != null) {
                out.add(b);
            }
        }
    }

    /** Gives Distant Horizons back after a failure; it is switched off while packs render from now on. */
    private void fail(RuntimeException e) {
        report("the Distant Horizons integration failed: " + e, e);
        try {
            giveBack();
        } catch (RuntimeException suppressed) {
            LOGGER.error("Could not restore Distant Horizons' renderers", suppressed);
        }
    }

    private void report(String reason, RuntimeException e) {
        if (unavailable == null) {
            unavailable = reason + "; LODs are not drawn with shader packs";
            LOGGER.error("Distant Horizons integration stopped", e);
            PackNotifier.warning(unavailable);
        }
    }
}
