//! Refuses to build when the embedded glslang would be compiled with its `assert()`s
//! enabled.
//!
//! `glslang-sys` builds glslang with the `cc` crate in the same environment as this build
//! script, and neither defines `NDEBUG`. An internal glslang `assert()` would then
//! `abort()` the host process (Minecraft) instead of reporting a compile failure. The
//! workspace's `.cargo/config.toml` forces `CXXFLAGS=-DNDEBUG`; this script checks the
//! flags `cc` will actually use (it concatenates `CXXFLAGS`, `HOST_`/`TARGET_CXXFLAGS` and
//! `CXXFLAGS_<target>`, the most specific last), so a build outside the workspace or with
//! an overriding environment fails loudly instead of shipping asserts. Set
//! `SB_ALLOW_GLSLANG_ASSERTS=1` to build anyway (debugging glslang itself).

use std::env;

fn main() {
    let target = env::var("TARGET").unwrap_or_default();
    let host = env::var("HOST").unwrap_or_default();
    let kind = if target == host { "HOST" } else { "TARGET" };
    // `cc`'s precedence order, least specific first (the order it concatenates them in).
    let vars = ["CXXFLAGS".to_string(), format!("{kind}_CXXFLAGS"), format!("CXXFLAGS_{}", target.replace('-', "_")), format!("CXXFLAGS_{target}")];
    for v in &vars {
        println!("cargo:rerun-if-env-changed={v}");
    }
    println!("cargo:rerun-if-env-changed=SB_ALLOW_GLSLANG_ASSERTS");
    let flags: Vec<String> = vars.iter().filter_map(|v| env::var(v).ok()).flat_map(|s| s.split_ascii_whitespace().map(str::to_string).collect::<Vec<_>>()).collect();
    if ndebug_defined(&flags) || env::var("SB_ALLOW_GLSLANG_ASSERTS").is_ok_and(|v| v == "1") {
        return;
    }
    panic!(
        "glslang would be built with its assert()s enabled (the C++ flags cc will use do not define NDEBUG: {flags:?}). \
         Build inside the ShaderBridge workspace (its .cargo/config.toml sets CXXFLAGS=-DNDEBUG), \
         add -DNDEBUG to CXXFLAGS, or set SB_ALLOW_GLSLANG_ASSERTS=1 to build anyway."
    );
}

/// Whether the last `NDEBUG` define or undefine in `flags` defines it.
fn ndebug_defined(flags: &[String]) -> bool {
    let mut defined = false;
    let mut i = 0;
    while i < flags.len() {
        let f = flags[i].as_str();
        let (op, name) = match f {
            "-D" | "-U" if i + 1 < flags.len() => {
                i += 1;
                (f, flags[i].as_str())
            }
            _ if f.starts_with("-D") || f.starts_with("-U") => (&f[..2], &f[2..]),
            _ => ("", ""),
        };
        let name = name.split('=').next().unwrap_or_default();
        if name == "NDEBUG" {
            defined = op == "-D";
        }
        i += 1;
    }
    defined
}
