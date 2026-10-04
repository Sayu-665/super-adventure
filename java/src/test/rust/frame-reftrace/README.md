# Reference frame traces for the Java frame sequencing tests

`dev.shaderbridge.render.frame.FrameSequencerTraceTest` checks the Java frame orchestration
against the headless executor (`crates/sb-runtime`). Both implement the same host contract. This
standalone package (not a workspace member) prints the executor's view of two frames of a
CompiledPack: pass order, implicit geometry passes, computes, attachments and their main/alt
textures, fullscreen outputs and inputs, flips, the copy to the output and the end-of-frame
copies.

The loop is a line-by-line transcription of `Executor::record_frame`, `geometry_pass`,
`fullscreen` and `end_of_frame` (`crates/sb-runtime/src/frame.rs`) without the GPU calls. The
target set follows `Executor::build`. The flip bookkeeping is not copied: the tool compiles
`crates/sb-runtime/src/flips.rs` itself. Update the transcription when `frame.rs` changes.

Regenerate the traces (run from the repository root):

```sh
R=java/src/test/resources/dev/shaderbridge/render
cargo build --release --manifest-path java/src/test/rust/frame-reftrace/Cargo.toml --target-dir target/frame-reftrace
for p in ComplementaryReimagined photon RethinkingVoxels; do
  target/frame-reftrace/release/sb-frame-reftrace $R/frame/$p.json world0 > $R/frame/$p.trace
done
target/frame-reftrace/release/sb-frame-reftrace $R/glimmer/pack.json world0 > $R/frame/glimmer.trace
target/frame-reftrace/release/sb-frame-reftrace $R/tutorial4/pack.json "" > $R/frame/tutorial4.trace
```

The structural pack fixtures come from `shaderbridge compile <corpus pack>/shaders --depth reversed
--dim world0`, reduced with `strip.py <pack.json> <out.json>`. The script removes everything taken
from the pack's files (see `../../resources/dev/shaderbridge/render/frame/README.md`).
