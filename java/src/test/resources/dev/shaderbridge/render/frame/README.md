# Frame sequencing fixtures

Reference data for `FrameSequencerTraceTest`.

## Pipeline structure (`<pack>.json`)

Each file is the `world0` pipeline of a corpus pack, compiled by ShaderBridge with
`shaderbridge compile <pack>/shaders --depth reversed --dim world0`. `strip.py` then reduced it to
the pipeline structure ShaderBridge derives: pass order, flip schedule, program kinds and names,
draw buffers, binding names and their `use_alt`, target formats and attachments.

Everything taken from the packs' files was removed. That covers stage modules and blobs, option
models and lang strings, id maps, custom uniform expressions, uniform layouts, custom texture paths
and diagnostics. No shader code or asset of the packs is contained.

- `ComplementaryReimagined.json`: Complementary Reimagined (Complementary License Agreement).
- `photon.json`: Photon (Photon Shaders License Agreement, see `photon.LICENSE`).
- `RethinkingVoxels.json`: Rethinking Voxels (Complementary Agreement). It exercises `shadowcomp`
  computes and a `prepare` chain.

## Reference traces (`<pack>.trace`)

These are the frame traces of the headless executor (`crates/sb-runtime`) for the same model, two
frames each. They were produced by `sb-reftrace` (a transcription of `Executor::record_frame`,
`geometry_pass`, `fullscreen` and `end_of_frame` without the GPU calls), which compiles
`crates/sb-runtime/src/flips.rs` itself. `glimmer.trace` and `tutorial4.trace` belong to the
`../glimmer` and `../tutorial4` fixtures.

Line format:

- `pass <group> <index>` or `pass <group> implicit`
- `computes <program>,...`
- `geometry <group> attachments=<target>:<main|alt>,...`
- `draw <program> writes=<location>=<target>:<main|alt>|sink,... reads=<binding>=<target>:<main|alt>,...`
- `flip <buffers>`
- `shadow-flip <buffers>`
- `copy ...`
- `copy-to-output colortex0:<main|alt>`
- `eof colortex<i>` and `eof shadowcolor<i>`
- `warn ...`
