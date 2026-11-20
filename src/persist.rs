use anyhow::{bail, ensure, Context, Result};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

pub const GHOST_DIR: &str = ".ghost";
pub const MANIFEST_PATH: &str = ".ghost/generated-manifest.json";
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Write a file through a temporary sibling and an atomic rename.
///
/// The temporary file is synced before the rename and the parent directory is
/// synced afterwards. A failed write never leaves a partially written target.
pub fn atomic_write(path: &Path, content: &[u8], executable: bool) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("could not create {}", parent.display()))?;

    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp = parent.join(format!(
        ".{name}.ghost-tmp-{}-{counter}",
        std::process::id()
    ));

    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .with_context(|| format!("could not create {}", temp.display()))?;
        file.write_all(content)
            .with_context(|| format!("could not write {}", temp.display()))?;
        file.sync_all()
            .with_context(|| format!("could not sync {}", temp.display()))?;
        drop(file);
        if executable {
            set_executable(&temp)?;
        }
        fs::rename(&temp, path).with_context(|| {
            format!(
                "could not move {} into place at {}",
                temp.display(),
                path.display()
            )
        })?;
        if let Ok(dir) = File::open(parent) {
            let _ = dir.sync_all();
        }
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<()> {
    Ok(())
}

/// Reject absolute paths and any component that could escape the project root.
pub fn validate_relative(relative: &Path) -> Result<()> {
    ensure!(
        !relative.as_os_str().is_empty(),
        "empty generated file path"
    );
    ensure!(
        !relative.is_absolute(),
        "generated file path must be relative: {}",
        relative.display()
    );
    for component in relative.components() {
        match component {
            Component::Normal(_) => {}
            _ => bail!(
                "generated file path must not contain '.' or '..': {}",
                relative.display()
            ),
        }
    }
    Ok(())
}

/// Resolve a generated path below `root`, refusing to follow symlinks.
pub fn safe_target(root: &Path, relative: &Path) -> Result<PathBuf> {
    validate_relative(relative)?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                bail!("refusing to write through symlink {}", current.display());
            }
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => break,
            Err(err) => {
                return Err(err).with_context(|| format!("could not inspect {}", current.display()))
            }
        }
    }
    Ok(root.join(relative))
}

/// A best-effort single-writer lock for `.ghost/`.
///
/// The lock is advisory: it protects ghost commands from each other, not other
/// programs. A lock left by a dead process is removed before retrying.
pub struct FileLock {
    path: PathBuf,
}

impl FileLock {
    pub fn acquire(root: &Path) -> Result<Self> {
        let dir = safe_target(root, Path::new(GHOST_DIR))?;
        fs::create_dir_all(&dir).with_context(|| format!("could not create {}", dir.display()))?;
        let path = dir.join(".lock");

        for attempt in 0..2 {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    writeln!(file, "{}", std::process::id())
                        .with_context(|| format!("could not write {}", path.display()))?;
                    return Ok(Self { path });
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                    let owner = fs::read_to_string(&path).unwrap_or_default();
                    let owner = owner.trim().to_string();
                    let alive = owner.parse::<u32>().map(process_alive).unwrap_or(false);
                    if !alive && attempt == 0 {
                        let _ = fs::remove_file(&path);
                        continue;
                    }
                    let detail = if owner.is_empty() {
                        String::new()
                    } else {
                        format!(", pid {owner}")
                    };
                    bail!(
                        "another ghost process is writing to .ghost (lock {}{detail})",
                        path.display()
                    );
                }
                Err(err) => {
                    return Err(err).with_context(|| format!("could not create {}", path.display()))
                }
            }
        }
        bail!("could not acquire .ghost lock")
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Best-effort credential filename detection. Used to keep likely secrets out
/// of checkpoints and context packets; it is not a secret scanner.
pub fn sensitive_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    if normalized
        .split('/')
        .any(|component| matches!(component, ".ssh" | ".aws" | ".gnupg" | ".docker"))
    {
        return true;
    }
    let name = normalized.rsplit('/').next().unwrap_or(&normalized);
    name.starts_with(".env")
        || name.starts_with("id_rsa")
        || name.starts_with("id_ed25519")
        || name.starts_with("id_ecdsa")
        || name.starts_with("id_dsa")
        || name.ends_with(".pem")
        || name.ends_with(".key")
        || name.ends_with(".p12")
        || name.ends_with(".pfx")
        || name.ends_with(".jks")
        || name.ends_with(".keystore")
        || name.ends_with(".kdbx")
        || matches!(
            name,
            ".netrc" | ".npmrc" | ".pypirc" | ".pgpass" | ".htpasswd" | "credentials"
        )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedManifest {
    pub schema_version: u32,
    pub generated_at: DateTime<Local>,
    pub files: BTreeMap<String, String>,
}

impl GeneratedManifest {
    pub fn build(files: &[(String, String)]) -> Self {
        Self {
            schema_version: MANIFEST_SCHEMA_VERSION,
            generated_at: Local::now(),
            files: files.iter().cloned().collect(),
        }
    }
}

pub fn manifest_path(root: &Path) -> PathBuf {
    root.join(MANIFEST_PATH)
}

pub fn load_manifest(root: &Path) -> Result<Option<GeneratedManifest>> {
    let path = manifest_path(root);
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("could not read {}", path.display())),
    };
    let manifest = serde_json::from_str(&raw)
        .with_context(|| format!("could not parse {}", path.display()))?;
    Ok(Some(manifest))
}

pub fn write_manifest(root: &Path, files: &[(String, String)], lock: &FileLock) -> Result<()> {
    let _ = lock;
    let manifest = GeneratedManifest::build(files);
    let raw = serde_json::to_string_pretty(&manifest)
        .context("could not serialize generated manifest")?;
    let path = manifest_path(root);
    let target = safe_target(root, Path::new(MANIFEST_PATH))?;
    atomic_write(&target, format!("{raw}\n").as_bytes(), false)
        .with_context(|| format!("could not write {}", path.display()))
}

/// Paths that exist but were not produced by this ghost run, or were edited
/// after generation. `--force` is required to replace them.
pub fn conflicts(
    root: &Path,
    files: &[(String, String)],
    manifest: Option<&GeneratedManifest>,
) -> Result<Vec<String>> {
    let mut found = Vec::new();
    for (relative, content) in files {
        let target = safe_target(root, Path::new(relative))?;
        let meta = match fs::symlink_metadata(&target) {
            Ok(meta) => meta,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(err).with_context(|| format!("could not inspect {}", target.display()))
            }
        };
        if meta.file_type().is_symlink() {
            found.push(format!("{relative} (symlink)"));
            continue;
        }
        let current =
            fs::read(&target).with_context(|| format!("could not read {}", target.display()))?;
        let current_hash = sha256_hex(&current);
        match manifest.and_then(|manifest| manifest.files.get(relative)) {
            Some(recorded) if *recorded != current_hash => {
                found.push(format!("{relative} (modified since generation)"));
            }
            Some(_) => {}
            None if current_hash != sha256_hex(content.as_bytes()) => {
                found.push(format!("{relative} (not generated by ghost)"));
            }
            None => {}
        }
    }
    found.sort();
    found.dedup();
    Ok(found)
}

/// SHA-256, implemented locally so the crate stays dependency-light.
pub fn sha256_hex(input: &[u8]) -> String {
    const H0: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let mut h = H0;
    let mut message = input.to_vec();
    let bit_len = (input.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    // `message` is padded above to a whole number of 64-byte blocks.
    for chunk in message.chunks(64) {
        let mut w = [0u32; 64];
        for (index, word) in chunk.chunks(4).enumerate() {
            w[index] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7)
                ^ w[index - 15].rotate_right(18)
                ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17)
                ^ w[index - 2].rotate_right(19)
                ^ (w[index - 2] >> 10);
            w[index] = w[index - 16]
                .wrapping_add(s0)
                .wrapping_add(w[index - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[index])
                .wrapping_add(w[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    h.iter().map(|word| format!("{word:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_common_credential_names() {
        for path in [
            ".env",
            "config/.env.local",
            ".ssh/id_rsa",
            "certs/server.pem",
            "tls/client.p12",
            "credentials",
            ".aws/config",
        ] {
            assert!(sensitive_path(path), "{path}");
        }
        for path in ["src/main.rs", "README.md", "docs/keyboard.md"] {
            assert!(!sensitive_path(path), "{path}");
        }
    }

    #[test]
    fn sha256_matches_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"The quick brown fox jumps over the lazy dog"),
            "d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592"
        );
    }

    #[test]
    fn atomic_write_replaces_content_without_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/file.txt");
        atomic_write(&path, b"first\n", false).unwrap();
        atomic_write(&path, b"second\n", false).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "second\n");
        let leftovers: Vec<_> = fs::read_dir(dir.path().join("nested"))
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().contains("ghost-tmp"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn relative_paths_stay_inside_the_project() {
        assert!(validate_relative(Path::new("specs/a.md")).is_ok());
        assert!(validate_relative(Path::new("/etc/passwd")).is_err());
        assert!(validate_relative(Path::new("../escape")).is_err());
        assert!(validate_relative(Path::new("a/../../b")).is_err());
        assert!(validate_relative(Path::new("./a")).is_err());
    }

    #[test]
    fn lock_is_exclusive_and_released_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let first = FileLock::acquire(dir.path()).unwrap();
        assert!(FileLock::acquire(dir.path()).is_err());
        drop(first);
        assert!(FileLock::acquire(dir.path()).is_ok());
    }

    #[test]
    fn stale_lock_is_reclaimed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".ghost/.lock");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "999999999\n").unwrap();
        assert!(FileLock::acquire(dir.path()).is_ok());
    }

    #[test]
    fn conflicts_detect_user_edits_and_unknown_files() {
        let dir = tempfile::tempdir().unwrap();
        let files = vec![("AGENTS.md".to_string(), "generated\n".to_string())];
        atomic_write(&dir.path().join("AGENTS.md"), b"generated\n", false).unwrap();
        assert!(conflicts(dir.path(), &files, None).unwrap().is_empty());
        let manifest =
            GeneratedManifest::build(&[("AGENTS.md".to_string(), sha256_hex(b"generated\n"))]);
        fs::write(dir.path().join("AGENTS.md"), "edited\n").unwrap();
        assert_eq!(
            conflicts(dir.path(), &files, Some(&manifest)).unwrap(),
            vec!["AGENTS.md (modified since generation)"]
        );
    }
}
