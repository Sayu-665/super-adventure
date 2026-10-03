# Render test fixtures

Real `CompiledPack` models for the `dev.shaderbridge.render` tests (see `RenderFixture`), compiled
from MIT-licensed packs of the shader corpus with the release CLI and reduced to their SPIR-V
blobs to stay small:

```sh
shaderbridge compile "<corpus>/MinecraftShaderProgramming/Tutorial 4 - Advanced Shadow Mapping/shaders" \
  -o /tmp/tutorial4 --depth reversed --target vulkan
shaderbridge compile <corpus>/glimmer-shaders/shaders -o /tmp/glimmer --depth reversed --target vulkan --dim world0
python3 spirv_only.py /tmp/tutorial4 tutorial4
python3 spirv_only.py /tmp/glimmer glimmer
```

Each directory keeps the license of its pack. Regenerate after a change to the CompiledPack
format or to the translator that the tests depend on.
