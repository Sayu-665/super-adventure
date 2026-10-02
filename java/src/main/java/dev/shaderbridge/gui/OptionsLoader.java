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
 * Loads a pack's options for the GUI: from the active pack when it is the one asked for,
 * otherwise by opening the pack natively on a background thread.
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
        if (active != null) {
            return CompletableFuture.completedFuture(new OptionsEditor(active.model().options(), saved));
        }
        return CompletableFuture.supplyAsync(() -> {
            try (PackSession session = PackSession.open(pack.file())) {
                OptionsModel model = session.options(language);
                return new OptionsEditor(model, saved);
            } catch (PackException e) {
                throw new CompletionException(e);
            }
        }, WORKER);
    }
}
