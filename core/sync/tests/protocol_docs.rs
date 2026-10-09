//! The protocol notes in `docs/protocols/` against the sources they cite
//! and the code that relies on them.
//!
//! What the sources say is fixed here as data read from them (each list
//! names where it came from); the tests check that the notes, the Rust
//! constants and the macOS shell's MIDI walker agree with it.

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    let path = root().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn notes() -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(root().join("docs/protocols"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".md") && n != "README.md")
        .collect();
    names.sort();
    names
}

#[test]
fn readme_indexes_every_note_with_a_current_status() {
    let readme = read("docs/protocols/README.md");
    let names = notes();
    assert!(names.len() >= 6, "notes: {names:?}");
    for name in &names {
        let row = readme
            .lines()
            .find(|l| l.starts_with(&format!("| [{name}]")))
            .unwrap_or_else(|| panic!("docs/protocols/README.md has no row for {name}"));
        for stale in ["Sources listed", "digest in session", "to be digested"] {
            assert!(!row.contains(stale), "stale status for {name}: {row}");
        }
    }
}

/// The headings of Chris Wilson's "A tale of two clocks", read from the
/// article source (`src/site/content/en/blog/audio-scheduling/index.md`
/// in https://github.com/GoogleChrome/web.dev, `main`).
const TWO_CLOCKS_SECTIONS: [&str; 7] = [
    "Introduction",
    "The Best of Times - the Web Audio Clock",
    "The Worst of Times - the JavaScript Clock",
    "Using JavaScript setTimeout() in Audio Apps",
    "Obtaining Rock-Solid Timing By Looking Ahead",
    "Yet Another Timing System",
    "Conclusion",
];

#[test]
fn lookahead_note_cites_only_sections_the_article_has() {
    // Citations wrap across lines; join them first.
    let note = read("docs/protocols/lookahead-scheduling.md").replace('\n', " ");
    let mut cited = 0;
    for part in note.split("§\"").skip(1) {
        let title = part
            .split('"')
            .next()
            .unwrap()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            TWO_CLOCKS_SECTIONS.contains(&title.as_str()),
            "lookahead-scheduling.md cites a section the article does not have: {title:?}"
        );
        cited += 1;
    }
    assert!(cited >= 4, "only {cited} section citations");
}

/// Section anchors of the DJ Link packet analysis, read from the dysentery
/// AsciiDoc sources (`doc/modules/ROOT/pages/<page>.adoc` on `main` of
/// https://github.com/Deep-Symmetry/dysentery), page by page.
const DJL_ANCHORS: [(&str, &[&str]); 6] = [
    ("packets", &["packet-types"]),
    (
        "startup",
        &[
            "mixer-startup",
            "mixer-initial-announcement",
            "mixer-assign-stage-1",
            "mixer-assign-stage-2",
            "mixer-assign-final",
            "mixer-keep-alive",
            "cdj-startup",
            "cdj-initial-announcement",
            "cdj-3000-foreshadowing",
            "cdj-assign-stage-1",
            "cdj-assign-stage-2",
            "cdj-assign-final",
            "cdj-keep-alive",
            "assignment-intention-packet",
            "assignment-packet",
            "assignment-finished-packet",
            "assignment-finished-from-player",
            "startup-3000",
            "cdj-3000-initial-announcement",
            "channel-conflict-packet",
            "xdj-xz-limitations",
            "xdj-xz-usb-network",
        ],
    ),
    (
        "beats",
        &[
            "beat-packets",
            "status-beat-offsets",
            "absolute-position-packets",
        ],
    ),
    (
        "vcdj",
        &[
            "creating-vcdj",
            "mixer-status-packets",
            "mixer-status-packet",
            "cdj-status-packets",
            "cdj-status-packet",
            "known-p1-values",
            "cdj-status-flag-bits",
            "current-track-bpm",
            "known-p3-values",
            "rekordbox-status-packets",
        ],
    ),
    (
        "sync",
        &[
            "sync-control",
            "tempo-master-handoff",
            "master-takeover-request-packet",
            "master-takeover-response-packet",
        ],
    ),
    (
        "mixer_integration",
        &["fader-start", "channels-on-air", "on-air-packet"],
    ),
];

fn files_under(rel: &str, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(root().join(rel)).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            let sub = path
                .strip_prefix(root())
                .unwrap()
                .to_string_lossy()
                .into_owned();
            files_under(&sub, out);
        } else if path
            .extension()
            .is_some_and(|e| e == "md" || e == "rs" || e == "hex")
        {
            out.push(path);
        }
    }
}

#[test]
fn every_djl_analysis_link_points_at_an_existing_section() {
    let mut files = Vec::new();
    files_under("docs", &mut files);
    files_under("core/sync/src", &mut files);
    let marker = "deepsymmetry.org/djl-analysis/";
    let mut links = 0;
    for path in files {
        let text = fs::read_to_string(&path).unwrap();
        for part in text.split(marker).skip(1) {
            let target: String = part
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || "_-.#".contains(*c))
                .collect();
            let (page, anchor) = target.split_once('#').unwrap_or((&target, ""));
            let page = page.trim_end_matches(".html");
            if page.is_empty() {
                continue; // the site's front page
            }
            let known = DJL_ANCHORS
                .iter()
                .find(|(p, _)| *p == page)
                .unwrap_or_else(|| panic!("{}: unknown page {page:?}", path.display()));
            assert!(
                anchor.is_empty() || known.1.contains(&anchor),
                "{}: no section #{anchor} on {page}",
                path.display()
            );
            links += 1;
        }
    }
    assert!(links > 50, "only {links} analysis links found");
}

/// UMP message sizes in 32-bit words by message type, from CoreMIDI's
/// `MIDIMessages.h` (macOS 11.3 SDK, `MIDIMessageType` and its comment on
/// the undefined types) and AM MIDI 2.0 Lib's `umpProcessor.cpp`; see
/// docs/protocols/midi-clock.md.
const UMP_WORDS: [usize; 16] = [1, 1, 1, 2, 2, 4, 1, 1, 2, 2, 2, 3, 3, 4, 4, 4];

#[test]
fn midi_note_states_the_ump_sizes_the_sources_give() {
    let note = read("docs/protocols/midi-clock.md");
    let table = note
        .split("| Type | Words |")
        .nth(1)
        .expect("midi-clock.md has no UMP size table");
    let mut seen = [None; 16];
    for line in table.lines().skip(2) {
        let line = line.trim();
        if !line.starts_with('|') {
            break;
        }
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        let words: usize = cells[2].parse().unwrap();
        for ty in cells[1].split(',') {
            let ty = ty.trim().trim_matches('`').trim_start_matches("0x");
            let ty = usize::from_str_radix(ty, 16).unwrap();
            seen[ty] = Some(words);
        }
    }
    for (ty, words) in UMP_WORDS.iter().enumerate() {
        assert_eq!(seen[ty], Some(*words), "midi-clock.md, type {ty:#x}");
    }
}

const MIDI_INPUT: &str = "apps/mac/Sources/Player5Kit/Clock/MIDIClockInput.swift";

#[test]
fn mac_midi_walker_steps_by_the_documented_sizes() {
    let swift = read(MIDI_INPUT);
    let body = swift
        .split("func wordCount(ofMessageType")
        .nth(1)
        .expect("wordCount(ofMessageType:) not found")
        .split("\n        }")
        .next()
        .unwrap();
    let mut sizes: [Option<usize>; 16] = [None; 16];
    let mut default = None;
    for line in body.lines() {
        let line = line.trim();
        let Some((cases, ret)) = line.split_once(": return ") else {
            continue;
        };
        let words: usize = ret.trim().parse().unwrap();
        if cases == "default" {
            default = Some(words);
            continue;
        }
        for ty in cases.trim_start_matches("case ").split(',') {
            let ty = usize::from_str_radix(ty.trim().trim_start_matches("0x"), 16).unwrap();
            sizes[ty] = Some(words);
        }
    }
    for (ty, words) in UMP_WORDS.iter().enumerate() {
        assert_eq!(
            sizes[ty].or(default),
            Some(*words),
            "{MIDI_INPUT}: type {ty:#x}"
        );
    }
    // Type 0x1, status byte in bits 16-23.
    assert!(swift.contains("messageType == 0x1"), "{MIDI_INPUT}");
    assert!(swift.contains("(word >> 16) & 0xFF"), "{MIDI_INPUT}");
    assert!(swift.contains("word >> 28"), "{MIDI_INPUT}");
}

#[test]
fn mac_midi_walker_advances_by_the_full_word_count() {
    // MIDIEventPacketNext is &pkt->words[pkt->wordCount], and wordCount may
    // exceed the declared 64 words (CoreMIDI MIDIServices.h).
    let swift = read(MIDI_INPUT);
    let count_line = swift
        .lines()
        .find(|l| l.contains("fromByteOffset: countOffset"))
        .expect("word count read not found");
    assert!(
        !count_line.contains("min("),
        "{MIDI_INPUT} caps the word count: {}",
        count_line.trim()
    );
    assert!(
        swift.contains("packet = wordBase + words * 4"),
        "{MIDI_INPUT}"
    );
}

#[test]
fn prolink_constants_match_the_sources() {
    use sync::prolink::packets as p;
    // PA "Packet Types": magic, kind at 0a, ports 50000-50002.
    assert_eq!(p::MAGIC, *b"Qspt1WmJOL");
    assert_eq!(p::KIND_OFFSET, 0x0a);
    assert_eq!(
        (p::ANNOUNCE_PORT, p::BEAT_PORT, p::STATUS_PORT),
        (50000, 50001, 50002)
    );
    // PA keep-alive 0x36, beat 0x60, precise position 0x3c, mixer status 0x38.
    assert_eq!(p::KEEP_ALIVE_LEN, 0x36);
    assert_eq!(p::BEAT_LEN, 0x60);
    assert_eq!(p::PRECISE_POSITION_LEN, 0x3c);
    assert_eq!(p::MIXER_STATUS_LEN, 0x38);
    // beat-link CdjStatus.MINIMUM_PACKET_SIZE; PA nexus d4, CDJ-3000 200.
    assert_eq!(p::CDJ_STATUS_MIN_LEN, 0xcc);
    assert_eq!(p::CDJ_STATUS_NEXUS_LEN, 0xd4);
    assert_eq!(p::CDJ_STATUS_CDJ3000_LEN, 0x200);
    // beat-link Util.NEUTRAL_PITCH = 1048576.
    assert_eq!(p::NEUTRAL_PITCH, 1_048_576);
    // PA status flag bits: 6 play, 5 master, 4 sync, 3 on air, 1 BPM sync.
    assert_eq!(
        [
            p::FLAG_PLAYING,
            p::FLAG_MASTER,
            p::FLAG_SYNCED,
            p::FLAG_ON_AIR,
            p::FLAG_BPM_SYNC
        ],
        [1 << 6, 1 << 5, 1 << 4, 1 << 3, 1 << 1]
    );
    // beat-link OpusProvider.OPUS_NAME.
    assert_eq!(p::OPUS_QUAD_NAME, "OPUS-QUAD");
}

#[test]
fn opus_constants_match_the_sources() {
    use std::time::Duration;
    use sync::opus;
    use sync::opus::packets as p;
    // beat-link VirtualRekordbox: device 0x17, fallback 0x13..=0x27,
    // announce interval 1500 ms within 200..=2000 ms.
    assert_eq!(p::DEFAULT_DEVICE_NUMBER, 0x17);
    assert_eq!(p::FALLBACK_DEVICE_NUMBERS, 0x13..=0x27);
    assert_eq!(opus::DEFAULT_ANNOUNCE_INTERVAL, Duration::from_millis(1500));
    assert_eq!(opus::MIN_ANNOUNCE_INTERVAL, Duration::from_millis(200));
    assert_eq!(opus::MAX_ANNOUNCE_INTERVAL, Duration::from_millis(2000));
    // Both sources: 54-byte keep-alive, 296-byte lighting request.
    assert_eq!(p::KEEP_ALIVE_LEN, 54);
    assert_eq!(p::LIGHTING_REQUEST_LEN, 296);
    assert_eq!(p::OPUS_NAME, "OPUS-QUAD");
    assert_eq!(p::REKORDBOX_NAME, "rekordbox");
    // beat-link DeviceFinder.MAXIMUM_AGE = 10000 ms.
    assert_eq!(opus::session::EXPIRY_NS, 10_000_000_000);
}
