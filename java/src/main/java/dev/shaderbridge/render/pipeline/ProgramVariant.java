package dev.shaderbridge.render.pipeline;

import dev.shaderbridge.model.Blobs;
import dev.shaderbridge.model.Program;

/**
 * A program together with the blob table its stage modules point into: the pack's main blob
 * buffer, or the buffer of a variant compiled later for another draw profile.
 *
 * @param folder  world folder of the program's dimension pipeline
 * @param program the program
 * @param blobs   its blob table
 */
public record ProgramVariant(String folder, Program program, Blobs blobs) {
    /** @return the draw profile the program was translated for ({@code ""} for compute programs) */
    public String profile() {
        return program.drawProfile() == null ? "" : program.drawProfile();
    }
}
