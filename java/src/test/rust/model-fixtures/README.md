# Serde fixtures for the Java model tests

`dev.shaderbridge.model.SerdeFixtureTest` parses JSON that **serde itself** produced from
`sb_core::model` values, so the Gson adapters are checked against the real wire format rather than
against a hand-written reading of the serde attributes. This standalone package (not a workspace
member) builds those values and writes the fixtures.

Regenerate after changing `crates/sb-core/src/model.rs` (run from the repository root):

```sh
cargo run --manifest-path java/src/test/rust/model-fixtures/Cargo.toml --target-dir target/model-fixtures \
  -- java/src/test/resources/dev/shaderbridge/model
```

Check the opposite direction (Java-written JSON parsed by serde) after running the Java tests:

```sh
cargo run --manifest-path java/src/test/rust/model-fixtures/Cargo.toml --target-dir target/model-fixtures \
  -- --check java/build/test-run/model-fixture-roundtrip.json
```

`--parse <file>` checks that any CompiledPack JSON file, such as the hand-written
`compiled_pack_sample.json`, is accepted by serde.
