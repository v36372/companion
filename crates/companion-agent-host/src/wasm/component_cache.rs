//! Serialized components contain native code, not untrusted Wasm. Only load
//! artifacts in a private, user-owned directory. On platforms without Unix mode
//! checks we deliberately compile instead of trusting an unverified cache.
use sha2::{Digest, Sha256};
use std::{
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};
use wasmtime::{Engine, component::Component};

// Keep synchronized with the exact workspace pin. The engine fingerprint also
// includes Wasmtime's version and all code-generation settings (including our
// epoch instrumentation and Cranelift optimization level).
const WASMTIME_VERSION: &str = "36.0.19";

struct Fingerprint(Sha256);
impl Hasher for Fingerprint {
    fn write(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
    fn finish(&self) -> u64 {
        u64::from_le_bytes(self.0.clone().finalize()[..8].try_into().unwrap())
    }
}
fn key(engine: &Engine, bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(bytes);
    digest.update(WASMTIME_VERSION);
    let mut fingerprint = Fingerprint(Sha256::new());
    engine
        .precompile_compatibility_hash()
        .hash(&mut fingerprint);
    digest.update(fingerprint.0.finalize());
    hex::encode(digest.finalize())
}

pub(super) fn configured_dir() -> Option<PathBuf> {
    if std::env::var_os("COMPANION_COMPONENT_CACHE").is_some_and(|v| v == "off") {
        return None;
    }
    std::env::var_os("COMPANION_CACHE_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            directories::ProjectDirs::from("", "LearningCompanion", "learning-companion")
                .map(|dirs| dirs.cache_dir().to_owned())
        })
}

pub(super) fn load(
    engine: &Engine,
    bytes: &[u8],
    root: Option<&Path>,
) -> anyhow::Result<Component> {
    load_with(engine, bytes, root, || {
        Component::from_binary(engine, bytes)
    })
}

fn load_with(
    engine: &Engine,
    bytes: &[u8],
    root: Option<&Path>,
    compile: impl FnOnce() -> anyhow::Result<Component>,
) -> anyhow::Result<Component> {
    let dir = root.map(|root| root.join("components"));
    let dir = dir.filter(|dir| match private_dir(dir) {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(%error, path = %dir.display(), "component cache unavailable; compiling");
            false
        }
    });
    let path = dir
        .as_ref()
        .map(|dir| dir.join(format!("agent-core-{}.cwasm", key(engine, bytes))));
    if let Some(path) = &path {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) => {
                let cached = private_file(&metadata).and_then(|()| {
                    // SAFETY: only we write these serialized components. The name
                    // includes the component digest, Wasmtime version and engine
                    // compatibility fingerprint. We verify a regular 0600 file
                    // inside a 0700 directory owned by this user before loading.
                    // Other processes running as this user are trusted, just as
                    // they are trusted not to replace the application executable.
                    unsafe { Component::deserialize_file(engine, path) }
                });
                match cached {
                    Ok(component) => return Ok(component),
                    Err(error) => {
                        tracing::warn!(%error, path = %path.display(), "invalid component cache; recompiling");
                        let _ = std::fs::remove_file(path);
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => tracing::warn!(%error, "cannot read component cache; compiling"),
        }
    }
    let component = compile()?;
    if let Some(path) = &path
        && let Err(error) = save(&component, path)
    {
        tracing::warn!(%error, path = %path.display(), "cannot write component cache; continuing");
    }
    Ok(component)
}

#[cfg(unix)]
fn private_dir(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true).mode(0o700);
    builder.create(path)?;
    // The root may be an existing OS cache directory (0755 is fine), but
    // another user must not be able to replace its private components child.
    let root = std::fs::symlink_metadata(path.parent().unwrap())?;
    anyhow::ensure!(
        root.is_dir()
            && root.uid() == unsafe { libc::geteuid() }
            && root.permissions().mode() & 0o022 == 0,
        "cache root must be user-owned and not writable by other users"
    );
    let metadata = std::fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_dir()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.permissions().mode() & 0o777 == 0o700,
        "cache directory must be user-owned and mode 0700 (not a symlink)"
    );
    Ok(())
}
#[cfg(unix)]
fn private_file(metadata: &std::fs::Metadata) -> anyhow::Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    anyhow::ensure!(
        metadata.is_file()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.permissions().mode() & 0o777 == 0o600,
        "cache file must be user-owned and mode 0600 (not a symlink)"
    );
    Ok(())
}
#[cfg(not(unix))]
fn private_dir(_: &Path) -> anyhow::Result<()> {
    anyhow::bail!("secure component cache is only supported on Unix")
}
#[cfg(not(unix))]
fn private_file(_: &std::fs::Metadata) -> anyhow::Result<()> {
    anyhow::bail!("unverified cache permissions")
}

fn save(component: &Component, path: &Path) -> anyhow::Result<()> {
    use std::io::Write;
    let dir = path.parent().unwrap();
    // NamedTempFile uses create_new and mode 0600 on Unix. Rename within the
    // same directory exposes either the complete old file or complete new one.
    let mut temp = tempfile::NamedTempFile::new_in(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    temp.write_all(&component.serialize()?)?;
    temp.as_file().sync_all()?;
    temp.persist(path)?;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("agent-core-") && name.ends_with(".cwasm") && entry.path() != path {
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    const SMALL: &[u8] = b"\0asm\x0d\0\x01\0";
    #[test]
    fn miss_hit_corrupt_and_permissions() {
        use std::{cell::Cell, os::unix::fs::PermissionsExt};
        let engine = super::super::engine().unwrap();
        let root = tempfile::tempdir().unwrap();
        let count = Cell::new(0);
        let compile = || {
            count.set(count.get() + 1);
            Component::from_binary(&engine, SMALL)
        };
        load_with(&engine, SMALL, Some(root.path()), compile).unwrap();
        let path = root
            .path()
            .join("components")
            .join(format!("agent-core-{}.cwasm", key(&engine, SMALL)));
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        load_with(&engine, SMALL, Some(root.path()), compile).unwrap();
        assert_eq!(count.get(), 1, "hit must not compile");
        std::fs::write(&path, b"corrupt").unwrap();
        load_with(&engine, SMALL, Some(root.path()), compile).unwrap();
        assert_eq!(count.get(), 2);
        load_with(&engine, SMALL, Some(root.path()), || {
            panic!("replacement must deserialize")
        })
        .unwrap();
        assert_ne!(key(&engine, SMALL), key(&engine, b"different"));
        let mut config = wasmtime::Config::new();
        config.epoch_interruption(false);
        assert_ne!(
            key(&engine, SMALL),
            key(&Engine::new(&config).unwrap(), SMALL)
        );
        std::fs::write(
            path.parent().unwrap().join("agent-core-stale.cwasm"),
            b"old",
        )
        .unwrap();
        std::fs::remove_file(&path).unwrap();
        load_with(&engine, SMALL, Some(root.path()), compile).unwrap();
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
    }
    #[test]
    fn uncached_and_unwritable_still_compile() {
        let engine = super::super::engine().unwrap();
        load(&engine, SMALL, None).unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        load(&engine, SMALL, Some(file.path())).unwrap();
    }
    #[test]
    fn rejects_untrusted_paths_and_write_failure_is_nonfatal() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let engine = super::super::engine().unwrap();
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("components");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        load(&engine, SMALL, Some(root.path())).unwrap();
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = dir.join(format!("agent-core-{}.cwasm", key(&engine, SMALL)));
        let victim = root.path().join("victim");
        std::fs::write(&victim, b"unchanged").unwrap();
        symlink(&victim, &path).unwrap();
        load(&engine, SMALL, Some(root.path())).unwrap();
        assert_eq!(std::fs::read(&victim).unwrap(), b"unchanged");
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap(); // persist cannot overwrite a directory
        load(&engine, SMALL, Some(root.path())).unwrap();
    }
    #[test]
    fn version_matches_workspace_pin() {
        assert!(
            include_str!("../../../../Cargo.toml")
                .contains(&format!("wasmtime = {{ version = \"={WASMTIME_VERSION}\""))
        );
    }
    #[test]
    fn kill_switch_subprocess() {
        const CHILD: &str = "COMPANION_CACHE_KILL_SWITCH_CHILD";
        if std::env::var_os(CHILD).is_some() {
            assert!(configured_dir().is_none());
            let engine = super::super::engine().unwrap();
            load(&engine, SMALL, configured_dir().as_deref()).unwrap();
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        assert!(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["kill_switch_subprocess", "--nocapture"])
                .env(CHILD, "1")
                .env("COMPANION_COMPONENT_CACHE", "off")
                .env("COMPANION_CACHE_DIR", dir.path())
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
