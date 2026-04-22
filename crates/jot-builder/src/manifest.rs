//! Build manifest for incremental compilation.
//!
//! The manifest is stored at `target/.build-manifest.json` (main sources),
//! `target/.build-manifest-test.json` (test sources), or
//! `target/.build-manifest-bench.json` (bench sources).
//!
//! It tracks source-file fingerprints (mtime, size, SHA-256) together with a
//! classpath hash and toolchain hash.  On subsequent builds, `jot` uses the
//! manifest to skip recompiling unchanged sources and to detect when a full
//! rebuild is required (e.g. after a dependency upgrade or toolchain change).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const MANIFEST_VERSION: u32 = 1;
pub const MANIFEST_FILENAME: &str = ".build-manifest.json";
pub const TEST_MANIFEST_FILENAME: &str = ".build-manifest-test.json";
pub const BENCH_MANIFEST_FILENAME: &str = ".build-manifest-bench.json";

// ── Data types ───────────────────────────────────────────────────────────────

/// Fingerprint for a single source file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceEntry {
    /// Modification time: seconds since the Unix epoch.
    pub mtime_secs: u64,
    /// Sub-second part of the modification time (nanoseconds).
    pub mtime_nanos: u32,
    /// File size in bytes.
    pub size: u64,
    /// Hex-encoded SHA-256 of the file contents.
    pub hash: String,
}

/// Per-output-directory build manifest persisted to disk as JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildManifest {
    pub manifest_version: u32,
    /// SHA-256 of the toolchain identifier (JDK version + optional Kotlin version).
    pub toolchain_hash: String,
    /// SHA-256 of the sorted classpath + processor-path entries.
    pub classpath_hash: String,
    /// Map from absolute source-file path string → fingerprint.
    pub sources: HashMap<String, SourceEntry>,
}

impl BuildManifest {
    /// Load a manifest from disk; returns `None` if the file is absent or
    /// cannot be parsed.
    pub fn load(path: &Path) -> Option<Self> {
        let content = std::fs::read(path).ok()?;
        serde_json::from_slice(&content).ok()
    }

    /// Atomically write the manifest to disk.
    pub fn save(&self, path: &Path) -> Result<(), std::io::Error> {
        let content = serde_json::to_vec_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        jot_common::atomic_write(path, &content)
    }
}

// ── Fingerprinting helpers ────────────────────────────────────────────────────

/// Compute a deterministic SHA-256 over a sorted list of classpath path strings.
///
/// **Known limitation:** the hash covers JAR file _paths_, not their contents.
/// A JAR replaced in-place at the same cached path (e.g., a local SNAPSHOT
/// overwriting the cached file) will not invalidate the manifest.  This is an
/// accepted trade-off for the current implementation; hashing every JAR on
/// every build would itself be expensive.
pub fn compute_classpath_hash(classpath: &[PathBuf]) -> String {
    let mut sorted: Vec<&PathBuf> = classpath.iter().collect();
    sorted.sort();
    let combined = sorted
        .iter()
        .map(|p| p.to_string_lossy())
        .collect::<Vec<_>>()
        .join("\n");
    jot_common::sha256_bytes(combined.as_bytes())
}

/// Compute a deterministic SHA-256 identifying the toolchain.
pub fn compute_toolchain_hash(jdk_version: &str, kotlin_version: Option<&str>) -> String {
    let combined = match kotlin_version {
        Some(kv) => format!("jdk:{jdk_version}|kotlin:{kv}"),
        None => format!("jdk:{jdk_version}"),
    };
    jot_common::sha256_bytes(combined.as_bytes())
}

/// Read a source file's mtime, size, and SHA-256 hash.
pub fn fingerprint_source(path: &Path) -> Result<SourceEntry, std::io::Error> {
    let metadata = std::fs::metadata(path)?;
    let mtime = metadata.modified()?;
    let duration = mtime
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let hash = jot_common::sha256_file(path)?;
    Ok(SourceEntry {
        mtime_secs: duration.as_secs(),
        mtime_nanos: duration.subsec_nanos(),
        size: metadata.len(),
        hash,
    })
}

// ── Incremental classification ────────────────────────────────────────────────

/// The result of comparing the current source set against a prior manifest.
pub enum IncrementalStatus {
    /// Wipe the output directory and recompile everything.
    FullRebuild {
        #[allow(dead_code)]
        reason: &'static str,
    },
    /// Only these source files changed; recompile them in place.
    Incremental { dirty: Vec<PathBuf> },
    /// Nothing changed; skip compilation entirely.
    UpToDate,
}

/// Classify sources as requiring a full rebuild, partial recompile, or no-op.
///
/// A **full rebuild** is triggered when:
/// - No prior manifest exists.
/// - The manifest schema version changed.
/// - The toolchain changed.
/// - The classpath (or processor paths) changed.
/// - A previously tracked source file was deleted.
///
/// An **incremental** rebuild is triggered when one or more sources are new or
/// have a different hash.
///
/// Otherwise returns `UpToDate`.
pub fn classify_sources(
    manifest: Option<&BuildManifest>,
    current_toolchain_hash: &str,
    current_classpath_hash: &str,
    all_sources: &[PathBuf],
) -> Result<IncrementalStatus, std::io::Error> {
    let manifest = match manifest {
        None => {
            return Ok(IncrementalStatus::FullRebuild {
                reason: "no prior build manifest",
            });
        }
        Some(m) => m,
    };

    if manifest.manifest_version != MANIFEST_VERSION {
        return Ok(IncrementalStatus::FullRebuild {
            reason: "manifest version changed",
        });
    }

    if manifest.toolchain_hash != current_toolchain_hash {
        return Ok(IncrementalStatus::FullRebuild {
            reason: "toolchain changed",
        });
    }

    if manifest.classpath_hash != current_classpath_hash {
        return Ok(IncrementalStatus::FullRebuild {
            reason: "classpath changed",
        });
    }

    // Check for sources that were previously tracked but are no longer in the
    // source set (e.g., deleted from disk, or moved outside of source_dirs).
    // Stale .class pruning requires a full rebuild in either case.
    let current_paths: std::collections::HashSet<String> = all_sources
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    for prev_path in manifest.sources.keys() {
        if !current_paths.contains(prev_path) {
            return Ok(IncrementalStatus::FullRebuild {
                reason: "source file no longer in the source set",
            });
        }
    }

    // Identify dirty (new or modified) source files.
    let mut dirty = Vec::new();
    for source_path in all_sources {
        let path_str = source_path.to_string_lossy().into_owned();
        match manifest.sources.get(&path_str) {
            None => {
                // Brand-new file not yet in the manifest.
                dirty.push(source_path.clone());
            }
            Some(prev) => {
                let metadata = std::fs::metadata(source_path)?;
                let mtime = metadata
                    .modified()?
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default();
                let mtime_secs = mtime.as_secs();
                let mtime_nanos = mtime.subsec_nanos();
                let size = metadata.len();

                // Fast path: mtime + size match → assume clean.
                if mtime_secs == prev.mtime_secs
                    && mtime_nanos == prev.mtime_nanos
                    && size == prev.size
                {
                    continue;
                }

                // Slow path: hash the file for a definitive answer (handles
                // `touch` without content change).
                if jot_common::sha256_file(source_path)? != prev.hash {
                    dirty.push(source_path.clone());
                }
            }
        }
    }

    if dirty.is_empty() {
        Ok(IncrementalStatus::UpToDate)
    } else {
        Ok(IncrementalStatus::Incremental { dirty })
    }
}

/// Build a fresh manifest by fingerprinting all `sources`.
pub fn build_updated_manifest(
    toolchain_hash: String,
    classpath_hash: String,
    sources: &[PathBuf],
) -> Result<BuildManifest, std::io::Error> {
    let mut source_entries = HashMap::new();
    for path in sources {
        let entry = fingerprint_source(path)?;
        source_entries.insert(path.to_string_lossy().into_owned(), entry);
    }
    Ok(BuildManifest {
        manifest_version: MANIFEST_VERSION,
        toolchain_hash,
        classpath_hash,
        sources: source_entries,
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn make_manifest(sources: &[(&Path, &str)], toolchain: &str, classpath: &str) -> BuildManifest {
        let mut map = HashMap::new();
        for (path, hash) in sources {
            map.insert(
                path.to_string_lossy().into_owned(),
                SourceEntry {
                    mtime_secs: 1_000_000,
                    mtime_nanos: 0,
                    size: 42,
                    hash: hash.to_string(),
                },
            );
        }
        BuildManifest {
            manifest_version: MANIFEST_VERSION,
            toolchain_hash: toolchain.to_string(),
            classpath_hash: classpath.to_string(),
            sources: map,
        }
    }

    #[test]
    fn full_rebuild_when_no_manifest() {
        let result = classify_sources(None, "tool-hash", "cp-hash", &[]).unwrap();
        assert!(matches!(result, IncrementalStatus::FullRebuild { .. }));
    }

    #[test]
    fn full_rebuild_on_toolchain_change() {
        let m = make_manifest(&[], "old-tool", "cp-hash");
        let result = classify_sources(Some(&m), "new-tool", "cp-hash", &[]).unwrap();
        assert!(matches!(result, IncrementalStatus::FullRebuild { reason } if reason.contains("toolchain")));
    }

    #[test]
    fn full_rebuild_on_classpath_change() {
        let m = make_manifest(&[], "tool-hash", "old-cp");
        let result = classify_sources(Some(&m), "tool-hash", "new-cp", &[]).unwrap();
        assert!(matches!(result, IncrementalStatus::FullRebuild { reason } if reason.contains("classpath")));
    }

    #[test]
    fn full_rebuild_on_manifest_version_change() {
        let mut m = make_manifest(&[], "tool-hash", "cp-hash");
        m.manifest_version = 999;
        let result = classify_sources(Some(&m), "tool-hash", "cp-hash", &[]).unwrap();
        assert!(matches!(result, IncrementalStatus::FullRebuild { reason } if reason.contains("version")));
    }

    #[test]
    fn full_rebuild_on_deleted_source() {
        let tmp = tempdir().unwrap();
        let gone = tmp.path().join("Gone.java");
        // `gone` is NOT written to disk — it represents a deleted file.
        let m = make_manifest(&[(&gone, "deadbeef")], "tool-hash", "cp-hash");
        let result = classify_sources(Some(&m), "tool-hash", "cp-hash", &[]).unwrap();
        assert!(matches!(result, IncrementalStatus::FullRebuild { reason } if reason.contains("no longer")));
    }

    #[test]
    fn up_to_date_when_fingerprints_match() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("Main.java");
        fs::write(&src, b"class Main {}").unwrap();

        let entry = fingerprint_source(&src).unwrap();
        let m = BuildManifest {
            manifest_version: MANIFEST_VERSION,
            toolchain_hash: "tool-hash".to_string(),
            classpath_hash: "cp-hash".to_string(),
            sources: HashMap::from([(src.to_string_lossy().into_owned(), entry)]),
        };

        let result = classify_sources(Some(&m), "tool-hash", "cp-hash", &[src]).unwrap();
        assert!(matches!(result, IncrementalStatus::UpToDate));
    }

    #[test]
    fn incremental_on_modified_source() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("Main.java");
        fs::write(&src, b"class Main {}").unwrap();

        // Record fingerprint, then modify the file.
        let entry = fingerprint_source(&src).unwrap();
        let m = BuildManifest {
            manifest_version: MANIFEST_VERSION,
            toolchain_hash: "tool-hash".to_string(),
            classpath_hash: "cp-hash".to_string(),
            sources: HashMap::from([(src.to_string_lossy().into_owned(), entry)]),
        };

        // Write different content (hash will differ even if mtime is same).
        fs::write(&src, b"class Main { /* changed */ }").unwrap();

        let result = classify_sources(Some(&m), "tool-hash", "cp-hash", &[src.clone()]).unwrap();
        assert!(matches!(&result, IncrementalStatus::Incremental { dirty } if dirty.contains(&src)));
    }

    #[test]
    fn incremental_on_new_source() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("New.java");
        fs::write(&src, b"class New {}").unwrap();

        // Manifest has no sources → src is new.
        let m = make_manifest(&[], "tool-hash", "cp-hash");
        let result = classify_sources(Some(&m), "tool-hash", "cp-hash", &[src.clone()]).unwrap();
        assert!(matches!(&result, IncrementalStatus::Incremental { dirty } if dirty.contains(&src)));
    }

    #[test]
    fn compute_classpath_hash_is_order_independent() {
        let a = PathBuf::from("/repo/a.jar");
        let b = PathBuf::from("/repo/b.jar");
        assert_eq!(
            compute_classpath_hash(&[a.clone(), b.clone()]),
            compute_classpath_hash(&[b, a]),
        );
    }

    #[test]
    fn manifest_save_and_load_roundtrip() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("manifest.json");
        let m = make_manifest(&[], "tool-hash", "cp-hash");
        m.save(&path).unwrap();

        let loaded = BuildManifest::load(&path).expect("should load");
        assert_eq!(loaded.toolchain_hash, "tool-hash");
        assert_eq!(loaded.classpath_hash, "cp-hash");
        assert_eq!(loaded.manifest_version, MANIFEST_VERSION);
    }

    #[test]
    fn classify_sources_up_to_date_overhead_is_negligible() {
        // Create 200 real Java source files, fingerprint them, then measure how
        // fast classify_sources runs on an already-up-to-date project.
        //
        // The goal: prove that the incremental check overhead is far below the
        // time saved by skipping javac (javac startup alone is typically >500ms).
        let tmp = tempdir().unwrap();
        let n = 200_usize;
        let mut sources = Vec::with_capacity(n);
        for i in 0..n {
            let path = tmp.path().join(format!("Class{i}.java"));
            fs::write(&path, format!("class Class{i} {{ int x = {i}; }}").as_bytes()).unwrap();
            sources.push(path);
        }

        let m = build_updated_manifest("tool-hash".to_string(), "cp-hash".to_string(), &sources)
            .expect("manifest build should succeed");
        let path = tmp.path().join("manifest.json");
        m.save(&path).unwrap();

        let loaded = BuildManifest::load(&path).expect("should load");

        let start = std::time::Instant::now();
        let status = classify_sources(Some(&loaded), "tool-hash", "cp-hash", &sources).unwrap();
        let elapsed = start.elapsed();

        // Must return UpToDate since nothing changed.
        assert!(matches!(status, IncrementalStatus::UpToDate));

        // The check overhead for 200 files must be under 200ms (javac startup
        // alone typically exceeds 500ms, so any value here represents a net win).
        assert!(
            elapsed.as_millis() < 200,
            "classify_sources took {elapsed:?} for {n} files — unexpectedly slow",
        );
    }

    #[test]
    fn build_updated_manifest_fingerprints_all_sources() {
        let tmp = tempdir().unwrap();
        let src1 = tmp.path().join("A.java");
        let src2 = tmp.path().join("B.java");
        fs::write(&src1, b"class A {}").unwrap();
        fs::write(&src2, b"class B {}").unwrap();

        let m = build_updated_manifest(
            "tool".to_string(),
            "cp".to_string(),
            &[src1.clone(), src2.clone()],
        )
        .unwrap();
        assert_eq!(m.sources.len(), 2);
        assert!(m.sources.contains_key(&src1.to_string_lossy().into_owned()));
        assert!(m.sources.contains_key(&src2.to_string_lossy().into_owned()));
    }
}
