//! Draw profiles registered by the host (`registerProfile`). They are process-wide: every
//! compile of every session started after a registration includes them.

use crate::error::{Error, Result};
use sb_transform::DrawProfile;
use std::sync::{LazyLock, RwLock};

static PROFILES: LazyLock<RwLock<Vec<DrawProfile>>> = LazyLock::new(Default::default);

/// Parse and validate a profile (TOML, `sb-transform/profiles/README.md`) and register it,
/// replacing an earlier registration of the same name. A registered profile takes
/// precedence over a built-in profile of the same name. Returns the profile name.
pub(crate) fn register(toml: &str) -> Result<String> {
    let profile = sb_transform::parse_profile(toml).map_err(|e| Error::invalid(format!("invalid draw profile: {e}")))?;
    let name = profile.name.clone();
    let mut all = PROFILES.write().unwrap_or_else(|e| e.into_inner());
    all.retain(|p| p.name != name);
    all.push(profile);
    Ok(name)
}

/// Every registered profile, in registration order.
pub(crate) fn snapshot() -> Vec<DrawProfile> {
    PROFILES.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Whether a profile named `name` is registered.
pub(crate) fn is_registered(name: &str) -> bool {
    PROFILES.read().unwrap_or_else(|e| e.into_inner()).iter().any(|p| p.name == name)
}
