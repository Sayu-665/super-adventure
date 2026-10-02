package dev.shaderbridge;

import dev.shaderbridge.config.ConfigStore;
import dev.shaderbridge.config.PackOptionValues;
import dev.shaderbridge.config.ShaderBridgeConfig;
import dev.shaderbridge.gui.PackNotifier;
import dev.shaderbridge.model.CompileEnvironment;
import dev.shaderbridge.natives.NativeLibrary;
import dev.shaderbridge.pack.CompileEnvironmentFactory;
import dev.shaderbridge.pack.CompileRequest;
import dev.shaderbridge.pack.CompileSettings;
import dev.shaderbridge.pack.LoadedPack;
import dev.shaderbridge.pack.PackCompiler;
import dev.shaderbridge.pack.PackEntry;
import dev.shaderbridge.pack.PackRepository;
import dev.shaderbridge.uniforms.BiomeIds;
import dev.shaderbridge.uniforms.GameStateCapture;
import dev.shaderbridge.uniforms.IdMapLookup;
import java.io.IOException;
import java.nio.file.Path;
import java.util.Map;
import java.util.Optional;
import net.minecraft.client.Minecraft;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * The mod's state: configuration, packs, the compiler and the active compiled pack. All methods
 * run on the client main thread; {@link #get()} is the entry point for the render integration.
 */
public final class ShaderBridge {
    private static final Logger LOGGER = LoggerFactory.getLogger("ShaderBridge");
    private static ShaderBridge instance;

    private final Path gameDir;
    private final ConfigStore config;
    private final PackRepository packs;
    private final PackCompiler compiler;
    private final GameStateCapture gameState = new GameStateCapture();
    private LoadedPack activePack;
    private Status status = new Status.Idle();
    /** Biome ids of the most recent compile request; a world with other biomes needs a recompile. */
    private Map<String, Integer> requestedBiomes = Map.of();

    /** What the compiler is doing, for the GUI. */
    public sealed interface Status {
        /** Nothing is selected, shaders are disabled, or the last pack was unloaded. */
        record Idle() implements Status {
        }

        /** @param pack the pack being compiled */
        record Compiling(String pack) implements Status {
        }

        /** @param pack the active pack */
        record Ready(String pack) implements Status {
        }

        /**
         * @param pack   the pack that failed
         * @param reason why
         */
        record Failed(String pack, String reason) implements Status {
        }
    }

    private ShaderBridge(Path gameDir, ConfigStore config) {
        this.gameDir = gameDir;
        this.config = config;
        this.packs = new PackRepository(gameDir.resolve("shaderpacks"));
        this.compiler = new PackCompiler(task -> Minecraft.getInstance().execute(task), new PackCompiler.Listener() {
            @Override
            public void onCompiled(LoadedPack pack) {
                activate(pack);
            }

            @Override
            public void onFailed(PackEntry pack, String reason) {
                fail(pack.name(), reason);
            }
        });
    }

    /**
     * Creates the instance; called once by the client entrypoint.
     *
     * @param gameDir the game directory
     * @param config  the loaded configuration
     * @return the instance
     */
    static ShaderBridge initialize(Path gameDir, ConfigStore config) {
        if (instance != null) {
            throw new IllegalStateException("ShaderBridge is already initialized");
        }
        instance = new ShaderBridge(gameDir, config);
        return instance;
    }

    /**
     * @return the mod's state
     * @throws IllegalStateException before the client entrypoint ran
     */
    public static ShaderBridge get() {
        if (instance == null) {
            throw new IllegalStateException("ShaderBridge is not initialized");
        }
        return instance;
    }

    /** @return the configuration */
    public ConfigStore config() {
        return config;
    }

    /** @return the packs in the {@code shaderpacks} directory */
    public PackRepository packs() {
        return packs;
    }

    /** @return the compiled pack shaders currently render with, if any */
    public Optional<LoadedPack> activePack() {
        return Optional.ofNullable(activePack);
    }

    /** @return the compiler state */
    public Status status() {
        return status;
    }

    /** @return the per-frame game state reader for builtin uniforms */
    public GameStateCapture gameState() {
        return gameState;
    }

    /**
     * Selects a pack (or none), enables shader packs when one is selected, and compiles it.
     *
     * @param pack a pack file name, or null for none
     */
    public void selectPack(String pack) {
        config.update(c -> c.withSelectedPack(pack).withEnabled(pack != null || c.enabled()));
        refresh();
    }

    /**
     * Enables or disables shader packs.
     *
     * @param enabled the new state
     */
    public void setEnabled(boolean enabled) {
        config.update(c -> c.withEnabled(enabled));
        refresh();
    }

    /** Recompiles the selected pack, e.g. after its files changed. */
    public void reload() {
        refresh();
    }

    /**
     * Stores a pack's option values and recompiles it if it is the selected pack.
     *
     * @param pack   pack file name
     * @param values the changed values
     */
    public void saveOptionValues(String pack, PackOptionValues values) {
        try {
            values.write(PackOptionValues.fileFor(packs.directory(), pack));
        } catch (IOException e) {
            LOGGER.error("Cannot save the settings of {}", pack, e);
            PackNotifier.error(pack, "cannot save settings: " + e.getMessage());
            return;
        }
        if (pack.equals(config.get().selectedPack())) {
            refresh();
        }
    }

    /**
     * Called when a world is joined: compiles the selected pack unless it is active (or being
     * compiled) for the biomes of this world.
     */
    public void onWorldJoin() {
        if (!config.get().isActive()) {
            return;
        }
        boolean upToDate = (status instanceof Status.Compiling || status instanceof Status.Ready)
            && requestedBiomes.equals(BiomeIds.current(Minecraft.getInstance().level));
        if (!upToDate) {
            refresh();
        }
    }

    /** Brings the active pack in line with the configuration. */
    private void refresh() {
        ShaderBridgeConfig cfg = config.get();
        if (!cfg.isActive()) {
            compiler.cancel();
            deactivate();
            status = new Status.Idle();
            return;
        }
        String name = cfg.selectedPack();
        if (!NativeLibrary.isLoaded()) {
            fail(name, NativeLibrary.status() instanceof NativeLibrary.Status.Failed failed ? failed.reason() : "the native library is not loaded");
            return;
        }
        Optional<PackEntry> entry = packs.find(name);
        if (entry.isEmpty()) {
            fail(name, "the pack is not in " + packs.directory());
            return;
        }
        if (!entry.get().valid()) {
            fail(name, entry.get().error());
            return;
        }
        PackOptionValues values;
        try {
            values = PackOptionValues.read(PackOptionValues.fileFor(packs.directory(), name));
        } catch (IOException e) {
            LOGGER.warn("Cannot read the settings of {}, using the defaults: {}", name, e.getMessage());
            values = PackOptionValues.empty();
        }
        requestedBiomes = BiomeIds.current(Minecraft.getInstance().level);
        CompileEnvironment environment = CompileEnvironmentFactory.create(cfg.depthMode(), requestedBiomes);
        CompileSettings settings = new CompileSettings(null, cfg.validate(), gameDir.resolve("shaderbridge").resolve("cache"), cfg.compileThreads());
        Path dumpDir = cfg.debugDumpGlsl() ? gameDir.resolve("shaderbridge").resolve("debug").resolve(name.replaceAll("[^A-Za-z0-9_.-]", "_")) : null;
        compiler.submit(new CompileRequest(entry.get(), environment, values, settings, dumpDir));
        status = new Status.Compiling(name);
    }

    private void activate(LoadedPack pack) {
        deactivate();
        activePack = pack;
        status = new Status.Ready(pack.name());
        gameState.usePack(IdMapLookup.of(pack.model().idMaps()), BiomeIds.fromMacros(pack.model().info().environment().extraMacros()));
        if (!pack.model().info().featuresUnsupported().isEmpty()) {
            LOGGER.warn("Shader pack {} requires unsupported features: {}", pack.name(), pack.model().info().featuresUnsupported());
        }
        PackNotifier.compiled(pack, config.get().showDiagnosticsInChat());
    }

    private void fail(String pack, String reason) {
        deactivate();
        status = new Status.Failed(pack, reason);
        LOGGER.error("Shader pack {} is not active: {}", pack, reason);
        PackNotifier.error(pack, reason);
    }

    private void deactivate() {
        if (activePack != null) {
            activePack.close();
            activePack = null;
        }
    }

    /** Stops the compiler and releases the active pack (client shutdown). */
    void shutdown() {
        compiler.close();
        deactivate();
    }
}
