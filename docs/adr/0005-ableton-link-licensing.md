# ADR-0005: Ableton Link stays behind an off-by-default feature

Status: Accepted for the code layout. Whether any distributed build
includes Link is **the owner's decision**; until it is made, none does.

## Context

ADR-0001 lists Ableton Link among the clock sources, and CLAUDE.md says
`rusty_link` arrives in its own session. This is that session. The
licensing has to be settled before any Link code reaches a build that
leaves the developer's machine.

Facts (sources below):

- **Link is dual-licensed.** Its copyright holder, Ableton AG, offers it
  under GPLv2 or later, or under a proprietary license on request
  (`link-devs@ableton.com`). This is stated in the Link README and
  `LICENSE.md`, read on `master` and at tag `Link-3.1.5`; the two
  `LICENSE.md` files are identical. [1] [2]
- **`rusty_link` is GPL-2.0-or-later.** Its README says it has to be,
  because Link is. The published crate includes the Link C++ sources and
  compiles them into a static library linked into the Rust binary. [3]
  Link's bundled Asio is under the Boost Software License 1.0, which is
  permissive. [4]
- **GPLv2 restricts distribution, not use.** "The act of running the
  Program is not restricted" (§0). A distributed work that contains the
  Program must be licensed as a whole under the GPL (§2(b)). Object code
  must come with the complete corresponding source or a written offer for
  it (§3). [5]
- **player5 is not open source.** The workspace declares
  `license = "UNLICENSED"` (root `Cargo.toml`).
- **iOS.** The Link README sends iOS developers to LinkKit. The LinkKit
  README puts LinkKit under the Ableton Link SDK license and states that
  the GPL is not compatible with the iOS App Store. [6] [7]
- **Web.** Link discovers peers over UDP multicast (protocol notes,
  "Peers and discovery"), and browsers cannot open UDP sockets (ADR-0001,
  ADR-0007). The web app reaches Link only through the bridge (ADR-0007),
  so the bridge binary is where Link would be linked for the web path.

## Decision

1. **Feature-gated, off by default.** Link lives only in `sync::link`
   (`core/sync/src/link.rs`), compiled only with the `sync` crate's
   `ableton-link` cargo feature. `rusty_link` is an optional dependency of
   that feature. No workspace crate enables it by default. Default builds
   (CI, the WASM module, the Apple XCFramework, the bridge) neither build
   nor link any Link or `rusty_link` code. `Cargo.lock` lists `rusty_link`
   and its build dependencies, because lockfiles record optional
   dependencies, but Cargo downloads and compiles them only when the
   feature is on.
2. **Pinned.** `rusty_link = "=0.4.8"`, which bundles the Link 3.1.5
   release. 0.4.9 bundles a Link 4 beta. [3]
3. **Development use is fine.** A developer may build and run with
   `--features ableton-link` on their own machine. GPLv2 §0 does not
   restrict running.
4. **Distribution needs one of these, and the owner chooses:**
   - **(a) GPL distribution.** Ship the Link-enabled binary with the
     complete corresponding source of the whole binary (player5's crates
     included) under GPLv2-or-later-compatible terms. That means relicensing
     at least those parts of player5. It rules out the iOS App Store.
   - **(b) Ableton's proprietary license** for Link. `rusty_link` itself
     stays GPL-2.0-or-later whatever Link's license is, so this option also
     needs either `rusty_link`'s author's permission under other terms, or
     our own thin bindings over `abl_link` (a new dependency decision:
     bindgen/cc or hand-written `extern "C"`).
   - **(c) iOS:** LinkKit under Ableton's Link SDK license (with the
     multicast entitlement, `docs/ios-multicast-entitlement.md`). This is a
     separate integration from `rusty_link`.
   - **(d) Don't ship Link.** Keep it a developer-only source.
5. Until the owner decides, no distributed artefact enables the feature.
   That covers release binaries of the bridge, the macOS and iOS apps, and
   the published web app.

## Consequences

- Default builds are free of GPL code. Link support exists for developers
  and stays tested by `cargo test -p sync --features ableton-link`.
- A shell that offers Link needs a forwarding feature (for example
  `ableton-link = ["sync/ableton-link"]` in the bridge or `core/ffi`), and
  every build with it enabled is a GPL-encumbered build under point 4.
  Cargo unifies features across the workspace, so one crate enabling it
  enables it for every crate built with it.
- Enabling the feature needs CMake ≥ 3.14, a C++ compiler, libclang (for
  bindgen) and Rust ≥ 1.85, because `rusty_link` uses edition 2024; the
  workspace otherwise targets 1.80. Default CI does not build it. A CI job
  with the feature would catch breakage, and building in CI is not
  distribution.
- Tools that fetch everything in the lockfile (for example
  `cargo vendor`) may pull `rusty_link`'s GPL sources into a vendored
  tree. Keep such trees out of anything that ships.
- This is an engineering record, not legal advice. Option 4 should be
  confirmed with counsel before any Link-enabled release.

## Sources

1. Ableton Link README, "License":
   https://github.com/Ableton/link/blob/Link-3.1.5/README.md#license (and
   on `master`: https://github.com/Ableton/link/blob/master/README.md#license)
2. Ableton Link `LICENSE.md`:
   https://github.com/Ableton/link/blob/master/LICENSE.md (identical at
   https://github.com/Ableton/link/blob/Link-3.1.5/LICENSE.md)
3. `rusty_link` 0.4.8: manifest
   (https://docs.rs/crate/rusty_link/0.4.8/source/Cargo.toml.orig,
   `license = "GPL-2.0-or-later"`, edition 2024), README "License" and
   "Requirements" (https://docs.rs/crate/rusty_link/0.4.8/source/README.md),
   `build.rs` (https://docs.rs/crate/rusty_link/0.4.8/source/build.rs),
   changelog (https://github.com/anzbert/rusty_link/blob/master/CHANGELOG.md#048
   for 0.4.8 → Link 3.1.5; the 0.4.9 entry, Link 4.0.0b3, is only in the
   0.4.9 crate: https://docs.rs/crate/rusty_link/0.4.9/source/CHANGELOG.md).
   Read from the published `.crate` files on static.crates.io.
4. Asio licence files in the bundled Link tree:
   https://docs.rs/crate/rusty_link/0.4.8/source/link/modules/asio-standalone/asio/LICENSE_1_0.txt
5. GNU GPL v2 as shipped with Link,
   https://github.com/Ableton/link/blob/Link-3.1.5/GNU-GPL-v2.0.md (§0,
   §2(b), §3); also https://www.gnu.org/licenses/old-licenses/gpl-2.0.html
6. Link README, iOS note:
   https://github.com/Ableton/link/blob/Link-3.1.5/README.md?plain=1#L90-L91
7. LinkKit README, "Building" and the license line:
   https://github.com/Ableton/LinkKit/blob/master/README.md

Network note: github.com HTML pages are blocked from the development
container. The Link and LinkKit files were read through
raw.githubusercontent.com, and the crate through static.crates.io. The
links above are the canonical published locations.
