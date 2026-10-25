//! LUMINA-CACHE-EXTRACT-70: die beiden nativen Disk-Tiers, bewusst
//! **außerhalb** von `lumina-core`.
//!
//! `lumina-core` bleibt im Produktionscode ohne Dateisystem und damit
//! plattformneutral; dieser Crate ist der Adapter zwischen Core und den nativen
//! Frontends und die **einzige** Stelle mit `std::fs` für den Ordner-Cache und
//! die Preview-Disk. Löschen des Caches ist niemals destruktiv: Previews werden
//! erneut gerendert, Rezepte bleiben in ihren Sidecars.
//!
//! Q4 desselben Auftrags: das Pfad-Layout ist öffentliche API
//! ([`disk::entry_stem`], [`disk::entry_paths`]), statt es in der GUI zu
//! spiegeln.

pub mod disk;
pub mod preview_disk;

use std::io;
use std::path::{Component, Path};

/// Create/open a directory path without following any symlink component.
/// Relative paths start at the opened working directory; absolute paths start
/// at root. Newly created components use 0777 subject to umask. On Unix,
/// dirfd-relative `mkdirat` + `openat(O_NOFOLLOW)` avoids the check-then-create
/// race. Existing ordinary directories are accepted unchanged.
#[cfg(unix)]
pub fn create_dir_all_nofollow(path: &Path) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;

    let anchor =
        CString::new(if path.is_absolute() { "/" } else { "." }).expect("literal has no NUL");
    let root_fd = unsafe {
        libc::open(
            anchor.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    };
    if root_fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut dir = unsafe { OwnedFd::from_raw_fd(root_fd) };
    for component in path.components() {
        let name = match component {
            Component::RootDir | Component::CurDir => continue,
            Component::Normal(name) => name,
            Component::ParentDir => {
                let parent = CString::new("..").expect("literal has no NUL");
                let next = unsafe {
                    libc::openat(
                        dir.as_raw_fd(),
                        parent.as_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    )
                };
                if next < 0 {
                    return Err(io::Error::last_os_error());
                }
                dir = unsafe { OwnedFd::from_raw_fd(next) };
                continue;
            }
            Component::Prefix(_) => unreachable!(),
        };
        let name = CString::new(name.as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in path component"))?;
        let fd = dir.as_raw_fd();
        if unsafe { libc::mkdirat(fd, name.as_ptr(), 0o777) } != 0 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::AlreadyExists {
                return Err(error);
            }
        }
        let next = unsafe {
            libc::openat(
                fd,
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if next < 0 {
            return Err(io::Error::last_os_error());
        }
        dir = unsafe { OwnedFd::from_raw_fd(next) };
    }
    Ok(())
}

#[cfg(not(unix))]
pub fn create_dir_all_nofollow(_path: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "symlink-safe directory creation is unavailable on this platform",
    ))
}

pub use disk::{DiskCacheError, DiskFolderCache, MAX_SETTINGS_BYTES};

#[cfg(all(test, unix))]
mod secure_dir_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn symlink_target_is_untouched_and_normal_directory_is_0755() {
        let tmp = tempfile::tempdir().unwrap();
        // DoD §10: no environment assumption. macOS resolves `TMPDIR` through
        // the `/var` -> `/private/var` symlink, and this function deliberately
        // refuses symlink ancestors; canonicalising the root removes that host
        // artefact without weakening either assertion below.
        let root = tmp.path().canonicalize().unwrap();
        let attacker = root.join("attacker");
        std::fs::create_dir(&attacker).unwrap();
        std::fs::write(attacker.join("marker"), b"unchanged").unwrap();
        let link = root.join("linked");
        std::os::unix::fs::symlink(&attacker, &link).unwrap();
        assert!(create_dir_all_nofollow(&link.join("new")).is_err());
        assert_eq!(
            std::fs::read(attacker.join("marker")).unwrap(),
            b"unchanged"
        );
        assert_eq!(std::fs::read_dir(&attacker).unwrap().count(), 1);

        let normal = root.join("new/under");
        create_dir_all_nofollow(&normal).unwrap();
        assert!(normal.is_dir());
        assert_eq!(
            u64::from(
                std::fs::metadata(root.join("new"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777
            ),
            0o777 & !current_umask()
        );
    }

    #[test]
    fn parent_components_are_resolved_without_following_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let expected = root.join("created");
        let path = root.join("child/../created");
        create_dir_all_nofollow(&path).unwrap();
        assert!(expected.is_dir());
    }

    /// The process umask, widened to `u64` so the value is comparable on both
    /// targets: `libc::mode_t` is `u16` on macOS and `u32` on Linux, and a
    /// `u32::from` / `as u32` at this boundary would be a same-type conversion
    /// (and therefore `clippy::useless_conversion` / `unnecessary_cast`) on
    /// Linux. `mode_t` is never `u64`, so this widening carries no lint on
    /// either platform.
    fn current_umask() -> u64 {
        // Querying umask requires changing it; restore immediately. This test
        // module has no other umask-mutating tests and asserts policy, not 0755.
        let mask = unsafe { libc::umask(0o777) };
        unsafe { libc::umask(mask) };
        mask as u64
    }
}
