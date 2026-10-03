//! Spec step 16: the compile cache (`<dir>/<key>.json` + `<key>.bin`).

use crate::CompileSettings;
use sb_core::model::{BlobTable, CompiledPack};
use sb_pack::ShaderPack;
use std::path::Path;

/// The cache key of a compile: blake3 over the pack contents, the normalized option
/// values, the environment (JSON), the ShaderBridge version and every other setting that
/// changes the output (dimension filter, language, validation, extra profiles and
/// variants).
pub fn cache_key(pack: &ShaderPack, settings: &CompileSettings) -> String {
    cache_key_with_hash(&pack.content_hash(), settings)
}

/// [`cache_key`] with a precomputed pack content hash.
pub fn cache_key_with_hash(content_hash: &str, settings: &CompileSettings) -> String {
    let mut h = blake3::Hasher::new();
    let mut part = |b: &[u8]| {
        h.update(&(b.len() as u64).to_le_bytes());
        h.update(b);
    };
    part(b"sb-pipeline-cache-v1");
    part(sb_core::SHADERBRIDGE_VERSION.as_bytes());
    part(&sb_core::MODEL_FORMAT_VERSION.to_le_bytes());
    part(content_hash.as_bytes());
    part(settings.option_values.to_settings_file().as_bytes());
    part(serde_json::to_string(&settings.env).unwrap_or_default().as_bytes());
    part(format!("{:?}", settings.dimension_filter).as_bytes());
    part(settings.language.as_bytes());
    part(&[u8::from(settings.validate_spirv)]);
    part(serde_json::to_string(&settings.extra_profiles).unwrap_or_default().as_bytes());
    part(serde_json::to_string(&settings.profile_overrides).unwrap_or_default().as_bytes());
    h.finalize().to_hex().to_string()
}

/// Load a cached compile, if present and readable.
pub fn load(dir: &Path, key: &str) -> Option<(CompiledPack, BlobTable)> {
    let json = std::fs::read_to_string(dir.join(format!("{key}.json"))).ok()?;
    let bin = std::fs::read(dir.join(format!("{key}.bin"))).ok()?;
    let pack = CompiledPack::from_json(&json).ok()?;
    if pack.format_version != sb_core::MODEL_FORMAT_VERSION {
        return None;
    }
    let blobs = BlobTable::from_concat(&pack.blobs, &bin)?;
    Some((pack, blobs))
}

/// Store a compile (written to temporary files first, then renamed).
pub fn store(dir: &Path, key: &str, pack: &CompiledPack, blobs: &BlobTable) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let (infos, buf) = blobs.concat();
    let mut pack = pack.clone();
    pack.blobs = infos;
    let json = dir.join(format!("{key}.json"));
    let bin = dir.join(format!("{key}.bin"));
    let tmp_json = dir.join(format!("{key}.json.tmp"));
    let tmp_bin = dir.join(format!("{key}.bin.tmp"));
    std::fs::write(&tmp_bin, buf)?;
    std::fs::write(&tmp_json, pack.to_json())?;
    std::fs::rename(&tmp_bin, bin)?;
    std::fs::rename(&tmp_json, json)?;
    Ok(())
}
