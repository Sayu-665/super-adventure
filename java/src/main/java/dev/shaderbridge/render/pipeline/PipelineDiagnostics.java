package dev.shaderbridge.render.pipeline;

import java.util.List;
import java.util.Set;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.CopyOnWriteArrayList;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Collects the reasons programs could not run (skipped programs, fallbacks, failed compiles), each
 * logged once, for the log and the pack screen's error panel. Thread-safe.
 */
public final class PipelineDiagnostics {
    private static final Logger LOGGER = LoggerFactory.getLogger("ShaderBridge");

    private final Set<String> seen = ConcurrentHashMap.newKeySet();
    private final List<String> messages = new CopyOnWriteArrayList<>();

    /**
     * Records a message; repeated messages are ignored.
     *
     * @param message a self-contained sentence naming the program
     */
    public void report(String message) {
        if (seen.add(message)) {
            messages.add(message);
            LOGGER.warn(message);
        }
    }

    /** @return every distinct message, in report order */
    public List<String> messages() {
        return List.copyOf(messages);
    }
}
