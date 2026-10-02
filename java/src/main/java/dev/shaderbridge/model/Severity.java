package dev.shaderbridge.model;

import dev.shaderbridge.model.json.WireEnum;

/** Diagnostic severity, ordered from least to most severe. */
public enum Severity implements WireEnum {
    /** Informational note. */
    INFO,
    /** Something was worked around; output may differ from Iris. */
    WARNING,
    /** A program or feature could not be translated. */
    ERROR
}
