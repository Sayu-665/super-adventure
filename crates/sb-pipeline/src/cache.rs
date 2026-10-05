//! Spec step 16: the compile cache (`<dir>/<key>.json` + `<key>.bin`).

use crate::CompileSettings;
use sb_core::model::{BlobTable, CompiledPack};
use sb_pack::ShaderPack;
use std::path::Path;

/// Revision of the translator: a blake3 hash (32 hex digits) of the sources of every crate
/// whose code determines a compile's output (`sb-core`, `sb-pack`, `sb-preprocess`,
/// `sb-expr`, `sb-uniforms`, `sb-transform` with its draw profiles, `sb-compile`,
/// `sb-pipeline`) and of the workspace `Cargo.lock`, computed by `build.rs`.
///
/// It is part of [`cache_key`], so a cache written by a build whose translator differs in
/// any way (not only in its version number) is never reused.
pub const TRANSLATOR_REVISION: &str = env!("SB_TRANSLATOR_REVISION");

/// The cache key of a compile: blake3 over the pack contents, the normalized option
/// values, the environment (JSON), the ShaderBridge version, the [`TRANSLATOR_REVISION`]
/// and every other setting that changes the output (dimension filter, language,
/// validation, extra profiles and variants).
pub fn cache_key(pack: &ShaderPack, settings: &CompileSettings) -> String {
    cache_key_with_hash(&pack.content_hash(), settings)
}

/// [`cache_key`] with a precomputed pack content hash.
pub fn cache_key_with_hash(content_hash: &str, settings: &CompileSettings) -> String {
    key_for_revision(content_hash, settings, TRANSLATOR_REVISION)
}

/// [`cache_key_with_hash`] for an explicit translator revision.
fn key_for_revision(content_hash: &str, settings: &CompileSettings, revision: &str) -> String {
    let mut h = blake3::Hasher::new();
    let mut part = |b: &[u8]| {
        h.update(&(b.len() as u64).to_le_bytes());
        h.update(b);
    };
    part(b"sb-pipeline-cache-v1");
    part(sb_core::SHADERBRIDGE_VERSION.as_bytes());
    part(revision.as_bytes());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translator_revision_is_part_of_the_key() {
        // build.rs exports 32 hex digits.
        assert_eq!(TRANSLATOR_REVISION.len(), 32);
        assert!(TRANSLATOR_REVISION.bytes().all(|b| b.is_ascii_hexdigit()), "{TRANSLATOR_REVISION}");
        let settings = CompileSettings::default();
        let current = cache_key_with_hash("pack", &settings);
        assert_eq!(current, key_for_revision("pack", &settings, TRANSLATOR_REVISION));
        // A translator change (same version, same pack and settings) is a different key.
        let other = key_for_revision("pack", &settings, "00000000000000000000000000000000");
        assert_ne!(current, other);
        assert_ne!(other, key_for_revision("pack", &settings, "00000000000000000000000000000001"));
        // Stable for identical inputs.
        assert_eq!(current, cache_key_with_hash("pack", &settings));
    }
}
