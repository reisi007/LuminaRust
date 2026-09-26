//! LENSFUN-DB-33: what a `timestamp.txt` is **made of** (review finding M2).
//!
//! Split out of `tests/fs_probe/mod.rs` (file-size ratchet, User-Vorgabe 2026-09-17).
//! The parent module asks the real filesystem about a `timestamp.txt` whose
//! *content* varies; this one asks it about a `timestamp.txt` whose *kind*
//! varies, which is a different axis and needs different fixtures (a directory, a
//! socket, a dangling symlink, a device node).
//!
//! Both need the real `FsProbe` and the real upstream symbol behind it, which is
//! why they live under one module rather than in two unrelated files.

use super::{running_as_root, Tree};
use crate::db_path::{resolve_with, FsProbe, LayerOrigin, Probe, SCHEMA_SUBDIR};
use crate::db_timestamp::DatabaseTimestamp;
use std::path::PathBuf;

/// What a `timestamp.txt` is *made of* — the part of the upstream value a byte
/// string cannot express, and the part where `std::fs::read` used to be too
/// coarse (finding M2).
///
/// `std::ifstream` *opens* first and only then *extracts*; upstream scores the
/// two failures differently (`0` vs `-1`). `fs::read` fuses them into one `Err`,
/// so a `timestamp.txt` that is a directory used to score `0` here and `-1`
/// upstream — and `0` beats `-1`, which is a mis-ordering and not cosmetics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StampKind {
    /// A normal file with these bytes.
    Regular(&'static [u8]),
    /// No `timestamp.txt` at all (the directory holds `a.xml` instead).
    Missing,
    /// A **directory** named `timestamp.txt`.
    Directory,
    /// A symlink to a directory.
    SymlinkToDirectory,
    /// A symlink whose target does not exist.
    SymlinkToMissing,
    /// A symlink to `/dev/null`: opens, then reads end-of-file.
    SymlinkToDevNull,
    /// A bound `AF_UNIX` socket: the OS refuses to `open(2)` it.
    Socket,
    /// A mode-`000` file.
    Unreadable,
}

/// The measured upstream value per file kind.
///
/// Measured with the same harness as the byte-string matrix (the real exported
/// symbol over a real filesystem), not read off the C++ source:
///
/// | `timestamp.txt` is … | upstream | here |
/// | --- | --- | --- |
/// | a regular file with a number | that number | that number |
/// | missing | `0` | `0` |
/// | **a directory** (or a symlink to one) | `-1` | `-1` |
/// | a **socket** | `0` | `0` |
/// | a symlink to a missing file | `0` | `0` |
/// | a symlink to `/dev/null` | `-1` | `-1` |
/// | mode `000` | `0` | `0` |
///
/// The socket row is worth stating explicitly, because "it is not a regular
/// file, so it must behave like the other odd ones" is **wrong**: `open(2)`
/// answers `ENXIO` for a socket, `fail()` is therefore already true *before* the
/// extraction, and upstream takes the `timestamp = 0` branch. A directory, by
/// contrast, opens fine and only fails on the read.
///
/// **A FIFO is deliberately not a row.** Measured: upstream and this crate
/// behave *identically* — both block in `open(2)` while no writer is attached,
/// and both block in the read while a writer is attached but has not closed
/// (libc++'s `filebuf` fills its whole buffer before the sentry sees a byte). A
/// test could therefore only assert a deadlock, so the row is documented here
/// instead.
const MEASURED_FILE_KINDS: &[(StampKind, &str, DatabaseTimestamp)] = &[
    (
        StampKind::Regular(b"1700000000\n"),
        "a regular file",
        DatabaseTimestamp::At(1_700_000_000),
    ),
    (
        StampKind::Missing,
        "missing",
        DatabaseTimestamp::NoTimestampFile,
    ),
    (
        StampKind::Directory,
        "a directory",
        DatabaseTimestamp::TimestampUnreadable,
    ),
    (
        StampKind::SymlinkToDirectory,
        "a symlink to a directory",
        DatabaseTimestamp::TimestampUnreadable,
    ),
    (
        StampKind::SymlinkToMissing,
        "a symlink to a missing file",
        DatabaseTimestamp::NoTimestampFile,
    ),
    (
        StampKind::SymlinkToDevNull,
        "a symlink to /dev/null",
        DatabaseTimestamp::BlankTimestampFile,
    ),
    (
        StampKind::Socket,
        "a socket",
        DatabaseTimestamp::NoTimestampFile,
    ),
    (
        StampKind::Unreadable,
        "mode 000",
        DatabaseTimestamp::NoTimestampFile,
    ),
];

/// Build `timestamp.txt` in `schema` as the given kind.
fn make_stamp(schema: &std::path::Path, kind: StampKind) {
    use std::os::unix::fs::symlink;
    let stamp = schema.join(crate::db_timestamp::TIMESTAMP_FILE);
    match kind {
        StampKind::Regular(bytes) => std::fs::write(&stamp, bytes).expect("write stamp"),
        // `a.xml` already makes the directory non-empty, which is what upstream
        // needs before it opens the file at all.
        StampKind::Missing => {}
        StampKind::Directory => std::fs::create_dir(&stamp).expect("mkdir timestamp.txt"),
        StampKind::SymlinkToDirectory => {
            let target = schema.join("elsewhere");
            std::fs::create_dir(&target).expect("mkdir target");
            symlink(&target, &stamp).expect("symlink to directory");
        }
        StampKind::SymlinkToMissing => {
            symlink(schema.join("nope"), &stamp).expect("dangling symlink")
        }
        StampKind::SymlinkToDevNull => symlink("/dev/null", &stamp).expect("symlink to /dev/null"),
        // Built by `socket_row`, which needs a shorter path than `Tree` gives.
        StampKind::Socket => unreachable!("the socket row has its own fixture"),
        StampKind::Unreadable => {
            use std::os::unix::fs::PermissionsExt;
            std::fs::write(&stamp, b"1700000000\n").expect("write stamp");
            std::fs::set_permissions(&stamp, std::fs::Permissions::from_mode(0o000))
                .expect("chmod 000");
        }
    }
}

/// **M2: what kind of file `timestamp.txt` is decides the value, and the two
/// failures are not the same failure.** The rows are measurements against the
/// real symbol (see [`MEASURED_FILE_KINDS`]); a mutation that collapses
/// "opened but unreadable" into "could not be opened" fails the directory rows.
#[test]
fn the_kind_of_timestamp_txt_decides_the_measured_value() {
    for (index, (kind, label, expected)) in MEASURED_FILE_KINDS.iter().enumerate() {
        if *kind == StampKind::Socket {
            socket_row(index, *expected);
            continue;
        }
        let tree = Tree::new(&label.replace(' ', "-"));
        let schema = tree.0.join(SCHEMA_SUBDIR);
        if *kind == StampKind::Unreadable {
            // The same honest gap the `MissReason::Unreadable` case documents:
            // root ignores the permission bits, so the row would be vacuous.
            if running_as_root() || {
                make_stamp(&schema, *kind);
                std::fs::File::open(schema.join(crate::db_timestamp::TIMESTAMP_FILE)).is_ok()
            } {
                eprintln!(
                    "M2: skipping the mode-000 row — this process can read the file \
                     anyway (root or an ACL), so the assertion would be vacuous"
                );
                continue;
            }
        } else {
            make_stamp(&schema, *kind);
        }
        assert_eq!(
            FsProbe.database_timestamp(&schema),
            *expected,
            "measured value for a timestamp.txt that is {label}"
        );
    }
}

/// The socket row, on a path short enough for `sun_path`.
///
/// `sockaddr_un::sun_path` is 104 bytes on macOS and the per-thread temp path the
/// other rows use is longer than that, so this row cannot share their fixture. It
/// gets a short root of its own, which is also where such a leftover realistically
/// sits (`/run`, `/var/run`).
fn socket_row(index: usize, expected: DatabaseTimestamp) {
    let root = PathBuf::from(format!("/tmp/lfs-sock-{}-{index}", std::process::id()));
    let schema = root.join(SCHEMA_SUBDIR);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&schema).expect("create socket fixture");
    std::fs::write(schema.join("a.xml"), "<lensdatabase/>").expect("write fixture xml");
    // Binding leaves the socket node in place, and dropping the listener does not
    // unlink it — the state a leftover socket leaves behind.
    let stamp = schema.join(crate::db_timestamp::TIMESTAMP_FILE);
    drop(std::os::unix::net::UnixListener::bind(&stamp).expect("bind socket"));
    assert_eq!(
        FsProbe.database_timestamp(&schema),
        expected,
        "measured value for a timestamp.txt that is a socket: `open(2)` answers \
         ENXIO, so upstream's `fail()` is already true and it stores 0"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// **The consequence of M2 at the algorithm level, on real files.**
///
/// A system database whose `timestamp.txt` is *blank* legitimately scores `-1`,
/// and a user update package whose `timestamp.txt` is a **directory** must score
/// `-1` too — upstream's measured value. Both `-1` keeps the system database
/// (the documented all-`-1` deviation). Had the directory scored `0`, the
/// `0 > -1` comparison would have handed the system layer to the update package:
/// the same class of silent displacement as the measured F1 defect, reached
/// through a different file kind.
#[test]
fn a_directory_as_timestamp_txt_never_displaces_the_system_database() {
    let tree = Tree::new("stampdir");
    // A blank timestamp.txt: the only reachable `-1` for a resolved database.
    tree.stamp(b"   \n");
    // A user update package that *holds XML* — so it is eligible and would win
    // the competition on merit — and whose `timestamp.txt` is a directory.
    let updates = tree.0.join("lensfun/updates/version_1");
    std::fs::create_dir_all(&updates).expect("create updates dir");
    std::fs::write(updates.join("u.xml"), "<lensdatabase/>").expect("write update xml");
    std::fs::create_dir(updates.join(crate::db_timestamp::TIMESTAMP_FILE)).expect("mkdir stamp");

    let resolved = resolve_with(
        Some(tree.dir().as_os_str()),
        None,
        Some(tree.dir().as_os_str()),
        Some(tree.dir().as_os_str()),
        &FsProbe,
    )
    .expect("the system database must resolve");
    assert_eq!(
        FsProbe.database_timestamp(&updates),
        DatabaseTimestamp::TimestampUnreadable,
        "the premise: a directory as timestamp.txt is -1 upstream, not 0"
    );
    assert_eq!(
        resolved.primary,
        LayerOrigin::SystemSchema,
        "the system database must keep the layer: {resolved:?}"
    );
    assert_eq!(resolved.layers[0].files.len(), 1);
    assert!(
        resolved
            .layers
            .iter()
            .all(|l| l.origin != LayerOrigin::UserUpdates),
        "the update package must not be loaded at all: {resolved:?}"
    );
}
