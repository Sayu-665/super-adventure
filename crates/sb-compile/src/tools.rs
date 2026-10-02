//! External SPIRV-Tools binaries: `spirv-val` (validation), `spirv-dis`
//! (disassembly) and `spirv-opt` (optional optimisation).
//!
//! glslang-sys is built without SPIRV-Tools, so these are run as processes when
//! they are installed. A tool is looked up, once per process, in:
//!
//! 1. the environment variable `SB_SPIRV_VAL` / `SB_SPIRV_DIS` / `SB_SPIRV_OPT`
//!    (a full path);
//! 2. every directory of `PATH`;
//! 3. `/usr/bin` and `/usr/local/bin`.
//!
//! Modules are handed over through uniquely named temporary files that are
//! removed afterwards. A tool that runs longer than 60 seconds is killed
//! (SPIRV-Tools take exponential time on some crafted modules); its run then
//! counts as not having happened ([`ValidationResult::Skipped`], `None`, or an
//! unoptimised module).

use crate::module;
use crate::target::{SpirvTarget, VulkanTarget};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Longest a SPIRV-Tools process may run before it is killed.
const TOOL_TIMEOUT: Duration = Duration::from_secs(60);

/// Outcome of [`validate`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "result", content = "message")]
pub enum ValidationResult {
    /// `spirv-val` accepted the module.
    Valid,
    /// The module is malformed or `spirv-val` rejected it; the validator's message.
    Invalid(String),
    /// Validation could not run (e.g. `spirv-val` is not installed); the reason.
    Skipped(String),
}

impl ValidationResult {
    /// `spirv-val` accepted the module.
    pub fn is_valid(&self) -> bool {
        matches!(self, Self::Valid)
    }
    /// The module was rejected.
    pub fn is_invalid(&self) -> bool {
        matches!(self, Self::Invalid(_))
    }
    /// Validation did not run.
    pub fn is_skipped(&self) -> bool {
        matches!(self, Self::Skipped(_))
    }
}

/// Locate a SPIRV-Tools binary (`spirv-val`, `spirv-dis`, `spirv-opt`, ...).
/// The result is cached per process.
pub fn find_tool(name: &str) -> Option<PathBuf> {
    static CACHE: Mutex<Option<HashMap<String, Option<PathBuf>>>> = Mutex::new(None);
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    guard.get_or_insert_with(HashMap::new).entry(name.to_string()).or_insert_with(|| search_tool(name)).clone()
}

fn search_tool(name: &str) -> Option<PathBuf> {
    let env_name = format!("SB_{}", name.to_ascii_uppercase().replace('-', "_"));
    if let Some(p) = std::env::var_os(&env_name).filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    let file = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    let path = std::env::var_os("PATH").unwrap_or_default();
    let fallback: [OsString; 2] = ["/usr/bin".into(), "/usr/local/bin".into()];
    std::env::split_paths(&path)
        .chain(fallback.iter().map(PathBuf::from))
        .map(|dir| dir.join(&file))
        .find(|p| p.is_file())
}

/// A temporary file removed on drop.
struct TempFile(PathBuf);

impl TempFile {
    fn new(ext: &str) -> std::io::Result<Self> {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir();
        loop {
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let path = dir.join(format!("sb-compile-{}-{n}.{ext}", std::process::id()));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => return Ok(Self(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
    }

    fn with_words(words: &[u32]) -> std::io::Result<Self> {
        let f = Self::new("spv")?;
        let mut file = OpenOptions::new().write(true).truncate(true).open(&f.0)?;
        file.write_all(&module::words_to_bytes(words))?;
        file.flush()?;
        Ok(f)
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Run `cmd` to completion, capturing its output in temporary files (so a
/// chatty tool cannot block on a full pipe), and kill it after `timeout`.
fn run_with_timeout(mut cmd: Command, timeout: Duration) -> Result<Output, String> {
    let stdout = TempFile::new("out").map_err(|e| format!("cannot create temporary file: {e}"))?;
    let stderr = TempFile::new("err").map_err(|e| format!("cannot create temporary file: {e}"))?;
    let open = |f: &TempFile| OpenOptions::new().write(true).truncate(true).open(&f.0);
    let (out_file, err_file) = match (open(&stdout), open(&stderr)) {
        (Ok(o), Ok(e)) => (o, e),
        (Err(e), _) | (_, Err(e)) => return Err(format!("cannot open temporary file: {e}")),
    };
    let program = cmd.get_program().to_string_lossy().into_owned();
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::from(out_file))
        .stderr(Stdio::from(err_file))
        .spawn()
        .map_err(|e| format!("cannot run {program}: {e}"))?;
    let start = Instant::now();
    let mut pause = Duration::from_millis(1);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{program} timed out after {} s", timeout.as_secs_f32()));
            }
            Ok(None) => {
                std::thread::sleep(pause);
                pause = (pause * 2).min(Duration::from_millis(20));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("cannot wait for {program}: {e}"));
            }
        }
    };
    let read = |f: &TempFile| std::fs::read(&f.0).unwrap_or_default();
    Ok(Output { status, stdout: read(&stdout), stderr: read(&stderr) })
}

fn target_env_for(spirv: &[u32], vulkan: VulkanTarget) -> &'static str {
    let version = module::header(spirv).ok().and_then(|h| SpirvTarget::from_header_word(h.version));
    vulkan.target_env(version.unwrap_or_else(|| vulkan.max_spirv()))
}

fn process_output(out: &std::process::Output) -> String {
    let mut s = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&out.stdout);
    if !stdout.trim().is_empty() {
        if !s.is_empty() {
            s.push('\n');
        }
        s.push_str(stdout.trim());
    }
    s
}

/// Validate a module with `spirv-val --target-env vulkan1.x` (the SPIR-V
/// version is taken from the module header, e.g. `vulkan1.1spv1.4`).
///
/// Returns [`ValidationResult::Skipped`] when `spirv-val` is not installed, and
/// [`ValidationResult::Invalid`] without running it when the header is malformed.
pub fn validate(spirv: &[u32], vulkan: VulkanTarget) -> ValidationResult {
    match find_tool("spirv-val") {
        Some(tool) => validate_with(&tool, spirv, vulkan),
        None => ValidationResult::Skipped("spirv-val not found (set SB_SPIRV_VAL or install SPIRV-Tools)".into()),
    }
}

/// [`validate`] with an explicit `spirv-val` binary.
pub fn validate_with(spirv_val: &Path, spirv: &[u32], vulkan: VulkanTarget) -> ValidationResult {
    validate_impl(spirv_val, spirv, vulkan, TOOL_TIMEOUT)
}

fn validate_impl(spirv_val: &Path, spirv: &[u32], vulkan: VulkanTarget, timeout: Duration) -> ValidationResult {
    if let Err(e) = module::header(spirv) {
        return ValidationResult::Invalid(e);
    }
    let file = match TempFile::with_words(spirv) {
        Ok(f) => f,
        Err(e) => return ValidationResult::Skipped(format!("cannot write temporary file: {e}")),
    };
    let mut cmd = Command::new(spirv_val);
    cmd.arg("--target-env").arg(target_env_for(spirv, vulkan)).arg(&file.0);
    match run_with_timeout(cmd, timeout) {
        Err(e) => ValidationResult::Skipped(e),
        Ok(out) if out.status.success() => ValidationResult::Valid,
        // Killed by a signal (crash, OOM killer): no verdict about the module.
        Ok(out) if out.status.code().is_none() => {
            ValidationResult::Skipped(format!("{} terminated abnormally ({})", spirv_val.display(), out.status))
        }
        Ok(out) => {
            let msg = process_output(&out);
            ValidationResult::Invalid(if msg.is_empty() { format!("spirv-val failed ({})", out.status) } else { msg })
        }
    }
}

/// Disassemble a module with `spirv-dis` (debugging aid). `None` if the tool is
/// missing or fails.
pub fn disassemble(spirv: &[u32]) -> Option<String> {
    let tool = find_tool("spirv-dis")?;
    let file = TempFile::with_words(spirv).ok()?;
    let mut cmd = Command::new(tool);
    cmd.arg("--no-color").arg(&file.0);
    let out = run_with_timeout(cmd, TOOL_TIMEOUT).ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Optimise a module with `spirv-opt -O`, preserving bindings and the stage
/// interface (so reflection and pipeline layouts are unaffected).
pub(crate) fn optimize(spirv: &[u32], vulkan: VulkanTarget) -> Result<Vec<u32>, String> {
    let tool = find_tool("spirv-opt").ok_or("spirv-opt not found (set SB_SPIRV_OPT or install SPIRV-Tools)")?;
    let input = TempFile::with_words(spirv).map_err(|e| format!("cannot write temporary file: {e}"))?;
    let output = TempFile::new("opt.spv").map_err(|e| format!("cannot create temporary file: {e}"))?;
    let mut cmd = Command::new(&tool);
    cmd.arg("-O")
        .arg("--preserve-bindings")
        .arg("--preserve-interface")
        .arg(format!("--target-env={}", target_env_for(spirv, vulkan)))
        .arg(&input.0)
        .arg("-o")
        .arg(&output.0);
    let out = run_with_timeout(cmd, TOOL_TIMEOUT)?;
    if !out.status.success() {
        return Err(format!("spirv-opt failed: {}", process_output(&out)));
    }
    let bytes = std::fs::read(&output.0).map_err(|e| format!("cannot read spirv-opt output: {e}"))?;
    module::words_from_bytes(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_tool_is_skipped() {
        let module = [module::MAGIC, 0x0001_0500, 0, 1, 0];
        let r = validate_with(Path::new("/nonexistent/spirv-val"), &module, VulkanTarget::Vulkan1_2);
        assert!(r.is_skipped(), "{r:?}");
    }

    #[test]
    fn malformed_header_is_invalid_without_tool() {
        let r = validate_with(Path::new("/nonexistent/spirv-val"), &[1, 2, 3], VulkanTarget::Vulkan1_2);
        assert!(r.is_invalid(), "{r:?}");
    }

    #[test]
    fn temp_files_are_unique_and_removed() {
        let a = TempFile::new("spv").unwrap();
        let b = TempFile::new("spv").unwrap();
        assert_ne!(a.0, b.0);
        let pa = a.0.clone();
        assert!(pa.exists());
        drop(a);
        assert!(!pa.exists());
    }

    /// A fake `spirv-val` shell script in a fresh temporary directory.
    #[cfg(unix)]
    fn fake_tool(body: &str) -> (TempFile, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let marker = TempFile::new("sh").unwrap();
        let path = marker.0.with_extension("fake-spirv-val");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        (marker, path)
    }

    #[cfg(unix)]
    #[test]
    fn validator_verdicts_follow_the_exit_status() {
        let module = [module::MAGIC, 0x0001_0500, 0, 1, 0];
        let (_m1, ok) = fake_tool("exit 0");
        assert_eq!(validate_with(&ok, &module, VulkanTarget::Vulkan1_2), ValidationResult::Valid);
        let (_m2, bad) = fake_tool("echo 'error: line 3: bad id' >&2; exit 1");
        assert_eq!(
            validate_with(&bad, &module, VulkanTarget::Vulkan1_2),
            ValidationResult::Invalid("error: line 3: bad id".into())
        );
        // A crashed validator says nothing about the module.
        let (_m3, crash) = fake_tool("kill -9 $$");
        let r = validate_with(&crash, &module, VulkanTarget::Vulkan1_2);
        assert!(r.is_skipped(), "{r:?}");
        // Neither does one that hangs: it is killed after the timeout.
        let (_m4, hang) = fake_tool("exec sleep 30");
        let start = Instant::now();
        let r = validate_impl(&hang, &module, VulkanTarget::Vulkan1_2, Duration::from_millis(200));
        assert!(matches!(&r, ValidationResult::Skipped(m) if m.contains("timed out")), "{r:?}");
        assert!(start.elapsed() < Duration::from_secs(10));
        // Lots of output does not block the tool on a full pipe.
        let (_m5, chatty) = fake_tool("head -c 2000000 /dev/zero | tr '\\0' x >&2; exit 1");
        match validate_impl(&chatty, &module, VulkanTarget::Vulkan1_2, Duration::from_secs(30)) {
            ValidationResult::Invalid(m) => assert_eq!(m.len(), 2_000_000),
            other => panic!("{other:?}"),
        }
        for p in [ok, bad, crash, hang, chatty] {
            let _ = std::fs::remove_file(p);
        }
    }

    #[test]
    fn target_env_follows_header() {
        let m14 = [module::MAGIC, 0x0001_0400, 0, 1, 0];
        assert_eq!(target_env_for(&m14, VulkanTarget::Vulkan1_1), "vulkan1.1spv1.4");
        assert_eq!(target_env_for(&[], VulkanTarget::Vulkan1_2), "vulkan1.2");
    }
}
