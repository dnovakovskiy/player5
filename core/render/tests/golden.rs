//! Golden-master tests: every pattern in `patterns/` renders to exactly the
//! stored PCM hash, and its fingerprint (levels, spectrum, onsets) stays
//! within tolerance.
//!
//! When a change to the sound is *intended*, regenerate the goldens and
//! commit them together with the change:
//!
//! ```text
//! UPDATE_GOLDEN=1 cargo test -p render --test golden
//! ```
//!
//! A hash mismatch with a matching fingerprint means the audio changed
//! imperceptibly (a reordered floating-point expression, say). That is still
//! a change and still needs a regenerated golden, but the message tells you
//! it is not a sound-design regression.

use std::fs;
use std::path::{Path, PathBuf};

use engine::PatternSpec;
use render::analysis::{hash_pcm16, Fingerprint, Tolerance};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Golden {
    hash_pcm16_fnv1a64: String,
    fingerprint: Fingerprint,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn pattern_files() -> Vec<PathBuf> {
    let dir = repo_root().join("patterns");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot list {}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no patterns found in {}", dir.display());
    files
}

fn golden_path(pattern: &Path) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(pattern.file_name().unwrap())
}

fn render(pattern: &Path) -> (PatternSpec, Vec<f32>) {
    let json = fs::read_to_string(pattern).unwrap();
    let spec =
        PatternSpec::from_json(&json).unwrap_or_else(|e| panic!("{}: {e}", pattern.display()));
    let audio = spec.render().unwrap();
    (spec, audio)
}

#[test]
fn renders_match_golden_masters() {
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    let mut failures = Vec::new();

    for pattern in pattern_files() {
        let name = pattern.file_name().unwrap().to_string_lossy().into_owned();
        let (spec, audio) = render(&pattern);
        let actual = Golden {
            hash_pcm16_fnv1a64: format!("{:#018x}", hash_pcm16(&audio)),
            fingerprint: Fingerprint::of(&audio, spec.render.sample_rate),
        };
        let golden = golden_path(&pattern);

        if update {
            let mut text = serde_json::to_string_pretty(&actual).unwrap();
            text.push('\n');
            fs::write(&golden, text).unwrap();
            eprintln!("updated {}", golden.display());
            continue;
        }

        let Ok(stored) = fs::read_to_string(&golden) else {
            failures.push(format!(
                "{name}: no golden at {} (run with UPDATE_GOLDEN=1)",
                golden.display()
            ));
            continue;
        };
        let expected: Golden = serde_json::from_str(&stored).unwrap();

        let diffs = actual
            .fingerprint
            .differences(&expected.fingerprint, Tolerance::default());
        if actual.hash_pcm16_fnv1a64 != expected.hash_pcm16_fnv1a64 {
            if diffs.is_empty() {
                failures.push(format!(
                    "{name}: PCM hash changed ({} -> {}) but the fingerprint still matches: \
                     an inaudible numeric change. Regenerate with UPDATE_GOLDEN=1 if intended.",
                    expected.hash_pcm16_fnv1a64, actual.hash_pcm16_fnv1a64
                ));
            } else {
                failures.push(format!(
                    "{name}: the sound changed:\n    {}",
                    diffs.join("\n    ")
                ));
            }
        } else if !diffs.is_empty() {
            failures.push(format!(
                "{name}: hash matches but fingerprint differs (stale golden?):\n    {}",
                diffs.join("\n    ")
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "golden-master mismatch:\n  {}",
        failures.join("\n  ")
    );
}

#[test]
fn renders_are_deterministic_across_block_sizes() {
    // Block boundaries must not leak into the audio: the same pattern
    // rendered with a different block size is bit-identical.
    for pattern in pattern_files() {
        let json = fs::read_to_string(&pattern).unwrap();
        let mut spec = PatternSpec::from_json(&json).unwrap();
        spec.render.block_size = 256;
        let a = spec.render().unwrap();
        spec.render.block_size = 97;
        let b = spec.render().unwrap();
        spec.render.block_size = 4_096;
        let c = spec.render().unwrap();
        assert_eq!(hash_pcm16(&a), hash_pcm16(&b), "{}", pattern.display());
        assert_eq!(hash_pcm16(&a), hash_pcm16(&c), "{}", pattern.display());
        assert!(
            a == b && a == c,
            "{}: float output differs",
            pattern.display()
        );
    }
}

#[test]
fn every_pattern_stays_within_headroom() {
    for pattern in pattern_files() {
        let (spec, audio) = render(&pattern);
        let fp = Fingerprint::of(&audio, spec.render.sample_rate);
        assert!(audio.iter().all(|s| s.is_finite()), "{}", pattern.display());
        assert!(
            fp.peak_dbfs <= 0.0,
            "{}: peak {} dBFS",
            pattern.display(),
            fp.peak_dbfs
        );
        if spec.render.output_gain == 1.0 && !spec.render.limiter {
            // Default headroom: nothing louder than -5 dBFS.
            assert!(
                fp.peak_dbfs <= -5.0,
                "{}: peak {} dBFS exceeds default headroom",
                pattern.display(),
                fp.peak_dbfs
            );
        }
    }
}
