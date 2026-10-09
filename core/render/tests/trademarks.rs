//! Trademark sweep (CLAUDE.md, "No Roland trademarks or trade dress"):
//! no source, doc, UI string or fixture in the repository names the
//! trademark owner outside the policy statement itself, or one of its drum
//! machine and bass-synth model numbers.

use std::fs;
use std::path::{Path, PathBuf};

/// Directories never scanned: build output (including the gitignored,
/// machine-generated JS core), dependencies, VCS, tooling.
const SKIP_DIRS: [&str; 12] = [
    "target",
    "node_modules",
    ".git",
    ".claude",
    "dist",
    "dist-single",
    "generated",
    "Frameworks",
    "build",
    "DerivedData",
    "test-results",
    "playwright-report",
];

/// Text files the repository is made of.
const EXTENSIONS: [&str; 21] = [
    "rs",
    "md",
    "ts",
    "js",
    "mjs",
    "swift",
    "json",
    "toml",
    "yml",
    "yaml",
    "html",
    "css",
    "sh",
    "txt",
    "hex",
    "webmanifest",
    "svg",
    "plist",
    "entitlements",
    "h",
    "bs",
];

/// Lockfiles are third-party integrity hashes, not our text.
const SKIP_FILES: [&str; 2] = ["package-lock.json", "Cargo.lock"];

/// The owner's name may appear only in lines stating the policy.
const POLICY_PHRASES: [&str; 2] = ["Roland trademarks", "Roland model"];

/// Model numbers, written in hex so this file does not spell them.
const MODEL_NUMBERS: [u32; 7] = [0x12F, 0x25E, 0x272, 0x2C3, 0x2D7, 0x328, 0x38D];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) {
                collect(&path, out);
            }
        } else if !SKIP_FILES.contains(&name.as_str())
            && path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| EXTENSIONS.contains(&e))
        {
            out.push(path);
        }
    }
}

/// Finds `needle` (a digit string) standing as its own number: not part of
/// a longer number or identifier, not a decimal fraction (`0.707`), and
/// not a hex byte run. A trailing `s` ("…s") still counts.
fn names_model_number(line: &str, needle: &str) -> bool {
    let bytes = line.as_bytes();
    let mut from = 0;
    while let Some(i) = line[from..].find(needle) {
        let start = from + i;
        let end = start + needle.len();
        let before = start.checked_sub(1).map(|j| bytes[j]);
        let after = bytes.get(end).copied();
        let before_ok =
            !matches!(before, Some(b) if b.is_ascii_alphanumeric() || b == b'.' || b == b',');
        let after_ok = !matches!(after, Some(a) if a.is_ascii_alphanumeric() && a != b's');
        let after_not_decimal =
            !(after == Some(b'.') && bytes.get(end + 1).is_some_and(u8::is_ascii_digit));
        if before_ok && after_ok && after_not_decimal {
            return true;
        }
        from = end;
    }
    false
}

fn violations(text: &str, owner: &str) -> Vec<String> {
    let numbers: Vec<String> = MODEL_NUMBERS.iter().map(u32::to_string).collect();
    let mut found = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let lower = line.to_lowercase();
        if lower.contains(owner) && !POLICY_PHRASES.iter().any(|p| line.contains(p)) {
            found.push(format!("line {}: owner's name: {line}", n + 1));
        }
        for number in &numbers {
            if names_model_number(line, number) {
                found.push(format!("line {}: model number: {line}", n + 1));
            }
        }
    }
    found
}

fn owner() -> String {
    // Lowercase, compared against lowercased lines.
    POLICY_PHRASES[0]
        .split_whitespace()
        .next()
        .unwrap()
        .to_lowercase()
}

#[test]
fn no_trademarks_anywhere_in_the_repository() {
    let root = repo_root();
    let mut files = Vec::new();
    collect(&root, &mut files);
    assert!(
        files.len() > 100,
        "the sweep found only {} files under {}",
        files.len(),
        root.display()
    );
    let owner = owner();
    let mut report = Vec::new();
    for path in files {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        for v in violations(&text, &owner) {
            let rel = path.strip_prefix(&root).unwrap_or(&path);
            report.push(format!("{}: {v}", rel.display()));
        }
    }
    assert!(report.is_empty(), "trademark sweep:\n{}", report.join("\n"));
}

#[test]
fn the_sweep_recognises_model_numbers_and_spares_ordinary_numbers() {
    let owner = owner();
    let n = MODEL_NUMBERS[5].to_string();
    for bad in [
        format!("an {n}-style kick"),
        format!("TR-{n}"),
        format!("classic {n}s"),
        format!("({n})"),
    ] {
        assert!(!violations(&bad, &owner).is_empty(), "missed {bad:?}");
    }
    let q = MODEL_NUMBERS[3].to_string();
    for fine in [
        format!("Q = 0.{q}"),
        format!("port 20{n}"),
        format!("-4.{n}"),
        format!("0x{n}"),
        format!("{n}.5 Hz"),
        "TR-inspired, TR-style accent".to_string(),
    ] {
        assert!(violations(&fine, &owner).is_empty(), "flagged {fine:?}");
    }
    let shouted = format!("a {} drum", owner.to_uppercase());
    assert!(!violations(&shouted, &owner).is_empty());
    assert!(violations("- **No Roland trademarks** anywhere", &owner).is_empty());
}
