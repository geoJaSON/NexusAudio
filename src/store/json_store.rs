//! Atomic JSON persistence for small mutable state (playlists, resume, queue,
//! settings). Writes go to a temp file then rename — a crash mid-write never
//! corrupts the existing file. This is what protects audiobook resume data.

use anyhow::{Context, Result};
use serde::{de::DeserializeOwned, Serialize};
use std::path::Path;

/// Load `T` from `path`, or `T::default()` if the file is absent/unreadable.
pub fn load_or_default<T: DeserializeOwned + Default>(path: &Path) -> T {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => T::default(),
    }
}

/// Serialize `value` to `path` atomically (temp file + rename).
pub fn save<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("mkdir {parent:?}"))?;
    }
    let json = serde_json::to_vec_pretty(value).context("serialize")?;

    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &json).with_context(|| format!("write {tmp:?}"))?;
    // rename is atomic on the same filesystem on both Windows and Unix.
    std::fs::rename(&tmp, path).with_context(|| format!("rename into {path:?}"))?;
    Ok(())
}
