//! LUMINA-CACHE-EXTRACT-70: die native Ordner-Cache-Disk-Schicht, bewusst
//! **außerhalb** von `lumina-core`.
//!
//! `lumina-core` bleibt im Produktionscode ohne Dateisystem und damit
//! plattformneutral; dieser Crate ist der Adapter zwischen Core und den nativen
//! Frontends und die **einzige** Stelle mit `std::fs` für den Ordner-Cache.
//! Löschen des Caches ist niemals destruktiv: Previews werden erneut gerendert,
//! Rezepte bleiben in ihren Sidecars.
//!
//! Q4 desselben Auftrags: das Pfad-Layout ist öffentliche API
//! ([`disk::entry_stem`], [`disk::entry_paths`]), statt es in der GUI zu
//! spiegeln.

pub mod disk;

pub use disk::{DiskCacheError, DiskFolderCache};
