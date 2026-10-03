package dev.shaderbridge.gui;

import dev.shaderbridge.ShaderBridge;
import dev.shaderbridge.config.PackOptionValues;
import dev.shaderbridge.model.OptionsModel;
import dev.shaderbridge.pack.LoadedPack;
import dev.shaderbridge.pack.PackEntry;
import dev.shaderbridge.pack.PackException;
import dev.shaderbridge.pack.PackSession;
import java.io.IOException;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.CompletionException;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/**
 * Loads a pack's options for the GUI on a background thread, with the lang strings of the
 * player's language: through the active pack's native session when it is the pack asked for,
 * otherwise by opening the pack natively.
 */
final class OptionsLoader {
    private static final ExecutorService WORKER = Executors.newSingleThreadExecutor(runnable -> {
        Thread thread = new Thread(runnable, "ShaderBridge Options");
        thread.setDaemon(true);
        return thread;
    });

    private OptionsLoader() {
    }

    /**
     * @param pack     the pack
     * @param language Minecraft language code for the pack's lang strings
     * @return the options editor, completed on a background thread
     */
    static CompletableFuture<OptionsEditor> load(PackEntry pack, String language) {
        ShaderBridge bridge = ShaderBridge.get();
        PackOptionValues saved;
        try {
            saved = PackOptionValues.read(PackOptionValues.fileFor(bridge.packs().directory(), pack.name()));
        } catch (IOException e) {
            return CompletableFuture.failedFuture(e);
        }
        LoadedPack active = bridge.activePack().filter(p -> p.name().equals(pack.name())).orElse(null);
        return CompletableFuture.supplyAsync(() -> new OptionsEditor(active != null ? activeOptions(active, language) : options(pack, language), saved), WORKER);
    }

    /**
     * The active pack's options in the requested language; the compiled model (whose lang strings
     * are {@code en_us}) if the session was closed in the meantime.
     */
    private static OptionsModel activeOptions(LoadedPack active, String language) {
        try {
            return active.session().options(language);
        } catch (PackException e) {
            return active.model().options();
        }
    }

    private static OptionsModel options(PackEntry pack, String language) {
        try (PackSession session = PackSession.open(pack.file())) {
            return session.options(language);
        } catch (PackException e) {
            throw new CompletionException(e);
        }
    }
}
