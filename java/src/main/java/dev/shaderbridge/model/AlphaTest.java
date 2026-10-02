package dev.shaderbridge.model;

/**
 * Alpha test of a program.
 *
 * @param func      comparison
 * @param reference reference value
 */
public record AlphaTest(AlphaFunc func, float reference) {
}
