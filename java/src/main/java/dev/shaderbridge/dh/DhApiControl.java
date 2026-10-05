package dev.shaderbridge.dh;

import com.seibel.distanthorizons.api.DhApi;
import com.seibel.distanthorizons.api.interfaces.config.IDhApiConfigValue;
import com.seibel.distanthorizons.api.interfaces.config.client.IDhApiGraphicsConfig;
import com.seibel.distanthorizons.api.methods.events.abstractEvents.DhApiBeforeApplyShaderRenderEvent;
import com.seibel.distanthorizons.api.methods.events.abstractEvents.DhApiBeforeFogRenderEvent;
import com.seibel.distanthorizons.api.methods.events.sharedParameterObjects.DhApiCancelableEventParam;
import com.seibel.distanthorizons.api.methods.events.sharedParameterObjects.DhApiRenderParam;
import java.util.Optional;

/**
 * Everything ShaderBridge does through Distant Horizons' public API (a compile-only dependency:
 * only touch this class when the {@code distanthorizons} mod is loaded). While a pack renders,
 * Distant Horizons' own output must not reach the screen, and its render list must hold every
 * section (the shadow pass needs sections outside the camera's view):
 *
 * <ul>
 *   <li>its apply pass (the copy of its color texture over Minecraft's) and its fog are
 *   cancelled through {@code DhApiBeforeApplyShaderRenderEvent} and
 *   {@code DhApiBeforeFogRenderEvent};</li>
 *   <li>its SSAO is turned off and its frustum culling disabled through API config overrides;</li>
 *   <li>its translucent LOD pass is deferred ({@code setDeferTransparentRendering}), so its first
 *   pass hands over the opaque half only;</li>
 *   <li>when ShaderBridge draws Distant Horizons' generic objects itself, its own generic rendering
 *   is turned off through the API config ({@code genericRendering().renderingEnabled()}), so the
 *   objects' per-frame callbacks run once, in ShaderBridge's replay.</li>
 * </ul>
 *
 * When the LODs cannot be handed over, Distant Horizons' rendering is switched off instead
 * ({@link #switchOff}). The previous API values are restored when the pack stops rendering.
 * Render thread only.
 */
final class DhApiControl {
    /** The name Distant Horizons shows for ShaderBridge's config overrides. */
    private static final String CALLER = "ShaderBridge";

    private static volatile boolean suppressing;
    private static boolean eventsBound;
    private static boolean holding;
    private static boolean off;
    private static Boolean previousSsao;
    private static Boolean previousCulling;
    private static boolean previousDefer;
    private static Boolean previousRendering;
    private static boolean holdingGeneric;
    private static Boolean previousGeneric;
    private static boolean genericWanted;

    private DhApiControl() {
    }

    /** Cancels Distant Horizons' apply pass while a pack renders. */
    private static final class CancelApply extends DhApiBeforeApplyShaderRenderEvent {
        @Override
        public void beforeRender(DhApiCancelableEventParam<DhApiRenderParam> event) {
            if (suppressing) {
                event.cancelEvent();
            }
        }
    }

    /** Cancels Distant Horizons' fog while a pack renders (packs fog LODs themselves). */
    private static final class CancelFog extends DhApiBeforeFogRenderEvent {
        @Override
        public void beforeRender(DhApiCancelableEventParam<DhApiBeforeFogRenderEvent.EventParam> event) {
            if (suppressing) {
                event.cancelEvent();
            }
        }
    }

    /**
     * @return Distant Horizons' settings, if it has finished starting up and renders LODs
     */
    static Optional<DhSettings> settings() {
        if (DhApi.Delayed.configs == null) {
            return Optional.empty();
        }
        IDhApiGraphicsConfig g = DhApi.Delayed.configs.graphics();
        if (!Boolean.TRUE.equals(g.renderingEnabled().getValue())) {
            return Optional.empty();
        }
        return Optional.of(new DhSettings(orElse(g.chunkRenderDistance(), 0), orElse(g.overdrawPreventionRadius(), -1.0f),
            orElse(g.lodOnlyMode(), false), orElse(g.earthCurvatureRatio(), 0)));
    }

    /**
     * Takes Distant Horizons' output over (idempotent).
     *
     * @param replayGeneric ShaderBridge draws the generic objects itself: switch Distant Horizons'
     *                      own generic rendering off (remembering whether the player had it on)
     * @return whether the API was ready (Distant Horizons has finished starting up)
     */
    static boolean takeOver(boolean replayGeneric) {
        if (holding) {
            return true;
        }
        if (DhApi.Delayed.configs == null || DhApi.Delayed.renderProxy == null) {
            return false;
        }
        if (!eventsBound) {
            DhApi.events.bind(DhApiBeforeApplyShaderRenderEvent.class, new CancelApply());
            DhApi.events.bind(DhApiBeforeFogRenderEvent.class, new CancelFog());
            eventsBound = true;
        }
        IDhApiGraphicsConfig g = DhApi.Delayed.configs.graphics();
        previousSsao = g.ambientOcclusion().enabled().getApiValue();
        previousCulling = g.disableFrustumCulling().getApiValue();
        previousDefer = DhApi.Delayed.renderProxy.getDeferTransparentRendering();
        g.ambientOcclusion().enabled().setValue(false, CALLER);
        g.disableFrustumCulling().setValue(true, CALLER);
        DhApi.Delayed.renderProxy.setDeferTransparentRendering(true);
        if (replayGeneric) {
            IDhApiConfigValue<Boolean> generic = g.genericRendering().renderingEnabled();
            genericWanted = Boolean.TRUE.equals(generic.getValue());
            previousGeneric = generic.getApiValue();
            generic.setValue(false, CALLER);
            holdingGeneric = true;
        } else {
            genericWanted = false;
        }
        suppressing = true;
        holding = true;
        return true;
    }

    /**
     * @return whether the player has Distant Horizons' generic objects on, while ShaderBridge holds
     *     its output and draws them itself
     */
    static boolean genericWanted() {
        return holding && holdingGeneric && genericWanted;
    }

    /**
     * Switches Distant Horizons' rendering off (idempotent), for when its LODs cannot be handed
     * over: its own passes (apply, far fade, vanilla fade) would otherwise blend its image over
     * the pack's.
     *
     * @return whether the API was ready
     */
    static boolean switchOff() {
        if (off) {
            return true;
        }
        if (DhApi.Delayed.configs == null) {
            return false;
        }
        IDhApiConfigValue<Boolean> rendering = DhApi.Delayed.configs.graphics().renderingEnabled();
        previousRendering = rendering.getApiValue();
        rendering.setValue(false, CALLER);
        off = true;
        return true;
    }

    /** Gives Distant Horizons' output back (idempotent). */
    static void release() {
        suppressing = false;
        if (off) {
            off = false;
            restore(DhApi.Delayed.configs.graphics().renderingEnabled(), previousRendering);
        }
        if (!holding) {
            return;
        }
        holding = false;
        IDhApiGraphicsConfig g = DhApi.Delayed.configs.graphics();
        restore(g.ambientOcclusion().enabled(), previousSsao);
        restore(g.disableFrustumCulling(), previousCulling);
        if (holdingGeneric) {
            holdingGeneric = false;
            restore(g.genericRendering().renderingEnabled(), previousGeneric);
        }
        DhApi.Delayed.renderProxy.setDeferTransparentRendering(previousDefer);
    }

    private static void restore(IDhApiConfigValue<Boolean> value, Boolean previous) {
        if (previous == null) {
            value.clearValue();
        } else {
            value.setValue(previous, CALLER);
        }
    }

    private static <T> T orElse(IDhApiConfigValue<T> value, T fallback) {
        T v = value.getValue();
        return v != null ? v : fallback;
    }
}
