package dev.shaderbridge.pack;

import dev.shaderbridge.model.Diagnostic;
import java.util.List;

/**
 * Counts of a compile's diagnostics.
 *
 * @param errors   number of errors
 * @param warnings number of warnings
 * @param infos    number of informational notes
 */
public record DiagnosticSummary(int errors, int warnings, int infos) {
    /**
     * @param diagnostics a compile's diagnostics
     * @return their counts
     */
    public static DiagnosticSummary of(List<Diagnostic> diagnostics) {
        int errors = 0;
        int warnings = 0;
        int infos = 0;
        for (Diagnostic d : diagnostics) {
            switch (d.severity()) {
                case ERROR -> errors++;
                case WARNING -> warnings++;
                case INFO -> infos++;
            }
        }
        return new DiagnosticSummary(errors, warnings, infos);
    }

    /** @return e.g. {@code 2 errors, 1 warning} or {@code no problems} */
    public String describe() {
        if (errors == 0 && warnings == 0) {
            return "no problems";
        }
        StringBuilder out = new StringBuilder();
        if (errors > 0) {
            out.append(errors).append(errors == 1 ? " error" : " errors");
        }
        if (warnings > 0) {
            if (!out.isEmpty()) {
                out.append(", ");
            }
            out.append(warnings).append(warnings == 1 ? " warning" : " warnings");
        }
        return out.toString();
    }
}
