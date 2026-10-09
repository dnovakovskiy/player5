# Pro DJ Link (CDJ-3000 / XDJ players, DJM mixers)

Status: digested; implemented in `core/sync/src/prolink/` (parsing and
builders in `packets.rs`, joining in `join.rs`, the device table in
`devices.rs`, the network source in `source.rs`). Fixtures:
[`fixtures/prolink/`](fixtures/prolink/).

What player5 needs from the booth network: which device is the tempo
master, when its beats happen, where the bar starts, and its tempo. This
note digests only the parts of the protocol that serve that, plus what it
takes to join the network politely as a fifth device.

## Sources and how they were read

One fact, one link. Abbreviations used below:

| Tag | Source |
|-----|--------|
| **PA** | Deep Symmetry, *DJ Link Packet Analysis* (dysentery), <https://djl-analysis.deepsymmetry.org/djl-analysis/>. Read from the AsciiDoc sources in the dysentery repository (`doc/modules/ROOT/pages/<page>.adoc` on `main`, fetched via raw.githubusercontent.com in October 2026) because the published site was not reachable from the build container; links point at the published pages and their section anchors. |
| **BL** | Deep Symmetry, *beat-link* (Java reference implementation), <https://github.com/Deep-Symmetry/beat-link>, `main`, files under `src/main/java/org/deepsymmetry/beatlink/`. |
| **CAP** | Hardware captures published with dysentery: `to-virtual.pcapng`, `powerup.pcapng`, `LinkInfo.pcapng` in <https://github.com/Deep-Symmetry/dysentery/tree/main/doc/assets>, and the CDJ-2000nexus session captures with their README in <https://github.com/Deep-Symmetry/dysentery/tree/main/doc/assets/captures>. |

Byte offsets and values are hexadecimal, as in PA.

## Header

- Every packet starts with the ten bytes `51 73 70 74 31 57 6d 4a 4f 4c`,
  followed by a kind byte at `0a` that, together with the port, identifies
  the packet — [PA, Packet Types](https://djl-analysis.deepsymmetry.org/djl-analysis/packets.html#packet-types).
- On port 50000 the byte after the kind (`0b`) is a subtype, the 20-byte
  device name follows at `0c`, then `01` at `20`, a structure byte at `21`
  and the total packet length `lenp` at `22`–`23` — [PA, Mixer Startup](https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#mixer-startup).
- On ports 50001 and 50002 the name starts one byte earlier at `0b`, the
  byte after it is `01`, a subtype sits at `20`, the device number `D` at
  `21` and the length of the rest of the packet `lenr` at `22`–`23` — [PA,
  Beat Packets](https://djl-analysis.deepsymmetry.org/djl-analysis/beats.html#beat-packets).
- The device name is at most 20 bytes of plain ASCII, padded with `00` —
  [BL, `VirtualCdj.setDeviceName`](https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/VirtualCdj.java).
- The name does not identify a device: two CDJ-2000nexus players both
  report `CDJ-2000nexus` — [CAP, captures README](https://github.com/Deep-Symmetry/dysentery/tree/main/doc/assets/captures).
  We key devices by device number.

## Ports

| Port | Carries | Source |
|------|---------|--------|
| 50000 | Announcements and device-number negotiation | [PA, Port 50000](https://djl-analysis.deepsymmetry.org/djl-analysis/packets.html#packet-types) |
| 50001 | Beats, precise position, on-air flags, fader start, sync and master handoff commands | [PA, Port 50001](https://djl-analysis.deepsymmetry.org/djl-analysis/packets.html#packet-types) |
| 50002 | Device status (and media/load commands) | [PA, Port 50002](https://djl-analysis.deepsymmetry.org/djl-analysis/packets.html#packet-types) |

The reference implementation listens on all three:

- 50000 on the wildcard address — [BL, `DeviceFinder.start`](https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/DeviceFinder.java);
- 50001 on the wildcard address — [BL, `BeatFinder.start`](https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/BeatFinder.java);
- 50002 on the interface address it announces — [BL, `VirtualCdj.createVirtualCdj`](https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/VirtualCdj.java).

## Packet types

The kinds player5 parses (`PacketKind` in `packets.rs` has the full table
from [PA, Packet Types](https://djl-analysis.deepsymmetry.org/djl-analysis/packets.html#packet-types)):

| Port | Kind | Packet | We |
|------|------|--------|----|
| 50000 | `0a` | Initial announcement (hello) | send |
| 50000 | `00` | First-stage number claim | send |
| 50000 | `01` | Mixer assignment intention | answer |
| 50000 | `02` | Second-stage claim (`0b` = `01`: request to a mixer) | send, watch for our number |
| 50000 | `03` | Mixer number assignment | obey |
| 50000 | `04` | Final-stage claim | send, watch for our number |
| 50000 | `05` | Assignment finished | obey |
| 50000 | `06` | Keep-alive | send, track devices |
| 50000 | `08` | Channel conflict ("number in use") | yield |
| 50001 | `03` | Channels on air | on-air flags |
| 50001 | `0b` | Precise (absolute) position, CDJ-3000 | tempo |
| 50001 | `28` | Beat | **phase and tempo** |
| 50002 | `0a` | CDJ status | master, playing, tempo |
| 50002 | `29` | Mixer status | master, tempo |

Kind `0a` means "hello" on 50000 and "CDJ status" on 50002 —
[BL, `Util.PacketType.CDJ_STATUS`](https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/Util.java),
so classification always takes the port.

## Keep-alive

Port 50000, kind `06`, `36` bytes — [PA, CDJ keep-alive](https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#cdj-keep-alive):

| Offset | Field |
|--------|-------|
| `24` | Device number `D` |
| `25` | `02` if the device was first on the network when it booted, else `01`; latched at boot ([PA, CDJ keep-alive](https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#cdj-keep-alive)) |
| `26`–`2b` | MAC address |
| `2c`–`2f` | IP address |
| `30` | Peer count `p`, the sender included |
| `34` | `01` on a player, `02` on a mixer ([PA, CDJ keep-alive](https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#cdj-keep-alive)) |
| `35` | `64` in the CDJ-3000-compatible variant ([PA, Startup with CDJ-3000s](https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#startup-3000)) |

- Peers drop out of `p` about ten seconds after they vanish —
  [PA, Mixer Startup](https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#mixer-keep-alive);
  the reference implementation forgets a device after 10 000 ms without an
  announcement — [BL, `DeviceFinder.MAXIMUM_AGE`](https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/DeviceFinder.java).
  We expire a device 10 s after its last packet of any kind.
- A CDJ sends keep-alives every 2.0026 s (measured on CDJ-2000nexus, firmware
  1.44); a mixer about every 1.5 s — [PA, CDJ keep-alive](https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#cdj-keep-alive),
  [PA, mixer keep-alive](https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#mixer-keep-alive).
- Keep-alives are broadcast — [PA, CDJ keep-alive](https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#cdj-keep-alive).

## Device kinds

`classify_device` in `packets.rs`:

- Byte `34` of the keep-alive: `01` player, `02` mixer (above).
- A lone mixer uses device number `21` (33) —
  [PA, Mixer Status Packets](https://djl-analysis.deepsymmetry.org/djl-analysis/vcdj.html#mixer-status-packets);
  without other information the reference implementation treats numbers
  from 33 up as mixers — [BL, `Beat.isBeatWithinBarMeaningful`](https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/Beat.java).
- rekordbox announces the name `rekordbox` and usually number `11`;
  rekordbox mobile starts at `29` — [PA, Rekordbox Status packets](https://djl-analysis.deepsymmetry.org/djl-analysis/vcdj.html#rekordbox-status-packets).
- CDJ-3000s can use player numbers 5 and 6 —
  [PA, CDJ Startup](https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#cdj-3000-foreshadowing).
  We treat player-type devices numbered 1–6 as players; higher player-type
  numbers are other virtual devices.

## Opus Quad

- The Opus Quad announces the name `OPUS-QUAD` —
  [BL, `OpusProvider.OPUS_NAME`](https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/data/OpusProvider.java)
  — and exposes device numbers 1, 2 and 33 —
  [PA, XDJ-XZ Limitations](https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#xdj-xz-limitations).
- It cannot take part in a DJ Link network —
  [PA, Background](https://djl-analysis.deepsymmetry.org/djl-analysis/packets.html)
  — and does not send beat or precise position packets in the rekordbox
  lighting mode — [BL, `VirtualCdj.start` javadoc](https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/VirtualCdj.java).
  The source warns and points to the separate `opus` source
  ([opus-quad.md](opus-quad.md)).

## Joining

To receive status, a device must announce itself: bind 50002 on the
interface that sees DJ Link traffic and send keep-alives to 50000 on the
broadcast address, as a CDJ would, with the interface's real MAC and IP —
[PA, Creating a Virtual CDJ](https://djl-analysis.deepsymmetry.org/djl-analysis/vcdj.html#creating-vcdj).
The other devices then send status packets straight to that socket
(same source).

The full startup sequence, which `join.rs` follows:

1. Three hellos (kind `0a`) 300 ms apart —
   [PA, CDJ initial announcement](https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#cdj-initial-announcement).
   Layout: `lenp` = `25`, payload `01` for a CDJ.
2. Three first-stage claims (kind `00`), 300 ms apart: `24` packet counter
   `N` (1–3), `25` = `01`, `26`–`2b` MAC, length `2c` —
   [PA, First-stage CDJ claim](https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#cdj-assign-stage-1).
3. Three second-stage claims (kind `02`), 300 ms apart: `24`–`27` IP,
   `28`–`2d` MAC, `2e` the number claimed `D`, `2f` counter `N`, `30` =
   `01`, `31` = `a` (`01` auto-assign, `02` a specific number), length
   `32` — [PA, Second-stage CDJ claim](https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#cdj-assign-stage-2).
   Byte `31` was confirmed on hardware — [CAP, captures README](https://github.com/Deep-Symmetry/dysentery/tree/main/doc/assets/captures).
4. Three final-stage claims (kind `04`), 300 ms apart: `24` `D`, `25` `N` —
   [PA, Final-stage CDJ claim](https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#cdj-assign-final).
   The diagram, the reference implementation's template
   ([BL, `VirtualCdj.claimStage3bytes`](https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/VirtualCdj.java))
   and the hardware capture (fixture `claim-stage3-cdj-2000nexus`) all
   have length `26`; the prose of the mixer section says `2a`, which we
   treat as a typo.
5. Then keep-alives. All of the above are broadcast to port 50000 (same
   sections).

Details we rely on:

- **CDJ-3000 variants.** To coexist with CDJ-3000s on numbers 5 and 6, use
  the CDJ-3000 templates: the hello has structure byte `04`, length `26`
  and payload `01 40`; the three claims have structure byte `03`; the
  keep-alive has `64` at `35`. The wrong keep-alive value can make CDJ-3000s
  on 5 or 6 drop off the network repeatedly —
  [PA, Startup with CDJ-3000s](https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#startup-3000).
  The hello payload `40` is decimal 64 in the diagram's notation; the
  reference template spells it `0x40` —
  [BL, `VirtualCdj.helloBytes`](https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/VirtualCdj.java).
  We always send these variants (we claim 5 by default).
- **Cutting the final stage short.** Any device already on the network may
  answer a final-stage claim with "assignment finished" (kind `05`, `24` =
  its own number), unicast; the claimant then goes straight to keep-alives —
  [PA, assignment finished from a player](https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#assignment-finished-from-player),
  real capture in fixture `assignment-finished-cdj-2000nexus`.
- **Mixer-assigned numbers.** On a channel-specific mixer port the mixer
  sends kind `01` ("will assign", `2f` bytes) straight to the claimant; the
  claimant answers with kind `02` and `0b` = `01`, `D` = `00`; the mixer
  replies with kind `03` (`24` = the assigned number) and the claimant
  accepts it, sends one final-stage claim, and receives kind `05` —
  [PA, Startup in a Channel-Specific Port](https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#assignment-intention-packet),
  real exchange in the four `assignment-*` fixtures. The reference
  implementation answers kind `01` only while claiming —
  [BL, `VirtualCdj.handleSpecialAnnouncementPacket`](https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/VirtualCdj.java).
- **Conflicts.** A device that already holds a number defends it by sending
  kind `08` (`24` = `D`, `25`–`28` its IP, `29` bytes) to port 50000 of the
  claimant, which then gives up —
  [PA, Channel Conflicts](https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#channel-conflict-packet).
- **Watch before claiming.** An XDJ-XZ does not defend its numbers and,
  through its laptop port, approves any claim —
  [PA, XDJ-XZ Limitations](https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#xdj-xz-limitations);
  the reference implementation therefore watches the network for 4 s before
  self-assigning — [BL, `VirtualCdj.selfAssignDeviceNumber`](https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/VirtualCdj.java).
- **Cadence.** "Roughly every 1.5 seconds" keeps a virtual CDJ on the
  network — [PA, Creating a Virtual CDJ](https://djl-analysis.deepsymmetry.org/djl-analysis/vcdj.html#creating-vcdj);
  the reference default is 1500 ms within an allowed 200–2000 ms —
  [BL, `VirtualCdj.announceInterval`](https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/VirtualCdj.java).
- **Source port.** The reference implementation sends its announcements
  from the socket bound to 50002 —
  [BL, `VirtualCdj.sendAnnouncement`](https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/VirtualCdj.java);
  a captured virtual CDJ keep-alive came from port 50002 (fixture
  `keep-alive-virtual-cdj`).
- **Device numbers.** Numbers outside 1–4 (1–6 with CDJ-3000s) break
  metadata queries — [PA, Creating a Virtual CDJ](https://djl-analysis.deepsymmetry.org/djl-analysis/vcdj.html#creating-vcdj);
  we make none. The reference implementation self-assigns from 7 to avoid
  CDJ-3000s on 5 and 6 — [BL, `VirtualCdj.selfAssignDeviceNumber`](https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/VirtualCdj.java).

### Our policy (design decisions, not protocol facts)

- We claim a **specific** number (`a` = `02`), 5 by default, configurable
  (`ProlinkConfig::device_number`). Self-assignment is not implemented.
- We start the hellos once our interface address is known, and put our
  number on the wire (stage 2) no earlier than 2.1 s after we started
  listening: longer than one CDJ keep-alive period, so every present player
  has announced itself.
- **Real hardware always wins.** If any other device announces, claims or
  defends our number — before or after we joined — we stop announcing,
  report a warning, and keep listening passively. We never send kind `08`.
  (The reference implementation does defend its number —
  [BL, `DeviceFinder.start`](https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/DeviceFinder.java)
  — but a drum machine must never knock a deck off the network.)
- We send keep-alives every 1.5 s with `p` = devices we see + 1 and byte
  `25` latched as "first on the network" when we started claiming.
- A mixer's assignment is accepted, as a CDJ does.

## Broadcast, unicast, and what works passively

| Traffic | Delivery | Passive listener sees it? |
|---------|----------|---------------------------|
| Hellos, claims, keep-alives (50000) | broadcast ([PA, startup](https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#cdj-startup)) | yes |
| Beat packets (50001) | broadcast ([PA, Beat Packets](https://djl-analysis.deepsymmetry.org/djl-analysis/beats.html#beat-packets); captured to `x.x.255.255` in fixture `beat-cdj-2000nexus`) | yes |
| Channels on air (50001) | broadcast by the mixer ([PA, Channels on Air](https://djl-analysis.deepsymmetry.org/djl-analysis/mixer_integration.html#channels-on-air)) | yes |
| Precise position (50001) | "sent to all connected devices" every 30 ms ([PA, Absolute Position](https://djl-analysis.deepsymmetry.org/djl-analysis/beats.html#absolute-position-packets)) | not stated; not relied on |
| CDJ and mixer status (50002) | unicast to announced devices ([PA, Creating a Virtual CDJ](https://djl-analysis.deepsymmetry.org/djl-analysis/vcdj.html#creating-vcdj)); on hardware, never to a host that had not announced itself ([CAP, captures README](https://github.com/Deep-Symmetry/dysentery/tree/main/doc/assets/captures)) | **no** |
| Assignment exchange, conflicts (50000) | unicast ([PA, Channel-Specific Port](https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#assignment-intention-packet), [PA, Channel Conflicts](https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#channel-conflict-packet)) | no |

So in passive mode (`ProlinkConfig::passive`) player5 knows the devices,
every playing player's beats, phase and tempo, and the mixer's on-air flags,
but **not** who is tempo master (that is only in status). Following
"master" then falls back to the lowest-numbered playing player (our
choice). A player counts as playing while its beats keep arriving: CDJs send
beat packets only while playing a rekordbox-analysed track —
[PA, Beat Packets](https://djl-analysis.deepsymmetry.org/djl-analysis/beats.html#beat-packets).

## Beat packets

Port 50001, kind `28`, `60` bytes, sent on each beat; "even the arrival of
the packet ... means that the player is starting a new beat" —
[PA, Beat Packets](https://djl-analysis.deepsymmetry.org/djl-analysis/beats.html#beat-packets):

| Offset | Field |
|--------|-------|
| `21` (and `5f`) | Device number `D` |
| `24`–`27` | ms until the next beat |
| `28`–`2b` | ms until the second beat |
| `2c`–`2f` | ms until the next bar (1–4 beats away) |
| `30`–`33` | ms until the fourth beat |
| `34`–`37` | ms until the second bar (5–8 beats away) |
| `38`–`3b` | ms until the eighth beat |
| `3c`–`53` | `ff` |
| `54`–`57` | Pitch (see [Pitch](#pitch)) |
| `5a`–`5b` | Track BPM × 100 |
| `5c` | Beat within bar `Bb`, 1 → 2 → 3 → 4 |

- The upcoming-beat times are given as if playing at +0 % pitch; scale by
  the pitch yourself. `ffffffff` means the track ends first —
  [PA, beat offsets](https://djl-analysis.deepsymmetry.org/djl-analysis/beats.html#status-beat-offsets).
- `Bb` identifies the downbeat when it comes from the master player and the
  beat grid is right — [PA, Beat Packets](https://djl-analysis.deepsymmetry.org/djl-analysis/beats.html#beat-packets).
- The mixer sends beat packets all the time as a backup metronome, always
  with +0 % pitch and the master player's BPM — [PA, Beat Packets](https://djl-analysis.deepsymmetry.org/djl-analysis/beats.html#beat-packets);
  its beat-within-bar is meaningless — [BL, `Beat.getBeatWithinBar`](https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/Beat.java).
- The CDJ-3000 can analyse an unanalysed track itself the first time it
  plays it — [PA, Beat Packets](https://djl-analysis.deepsymmetry.org/djl-analysis/beats.html#beat-packets).
- **Timing relative to the audible beat.** The sources say only that the
  packet marks the start of the beat. No published measurement gives a
  fixed offset between a player's audio output and its beat packet, so we
  apply none: the observation is stamped with the receive time. The global
  latency offset (`ClockControls`) absorbs whatever the booth adds. In the
  real capture (fixture `beat-cdj-2000nexus`, [CAP, S06](https://github.com/Deep-Symmetry/dysentery/tree/main/doc/assets/captures/S06-load-and-play))
  consecutive beat packets from a player at 127.99 BPM effective arrive
  469 ms apart, as expected (60 / 127.99 = 0.4688 s).

## Pitch

- Beat and status packets encode pitch as a 4-byte value where `00100000`
  is +0 %, `00000000` is −100 % (stopped) and `00200000` is +100 % —
  [PA, Beat Packets](https://djl-analysis.deepsymmetry.org/djl-analysis/beats.html#beat-packets);
  the neutral value is 1048576 — [BL, `Util.NEUTRAL_PITCH`](https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/Util.java).
- BPM fields are track BPM × 100; effective (playing) BPM is track BPM ×
  pitch / `100000` (hex), i.e. `bpm × pitch / 6400000` (hex) —
  [PA, Beat Packets](https://djl-analysis.deepsymmetry.org/djl-analysis/beats.html#beat-packets).
- In status packets the track BPM is `ffff` when no track is loaded —
  [PA, CDJ Status Packets](https://djl-analysis.deepsymmetry.org/djl-analysis/vcdj.html#current-track-bpm).
- Precise position packets use a friendlier encoding: pitch as a signed
  32-bit percentage × 100, effective BPM × 10, `ffffffff` for unknown —
  [PA, Absolute Position](https://djl-analysis.deepsymmetry.org/djl-analysis/beats.html#absolute-position-packets).

## CDJ status

Port 50002, kind `0a`, every ~200 ms, unicast to announced devices —
[PA, CDJ Status Packets](https://djl-analysis.deepsymmetry.org/djl-analysis/vcdj.html#cdj-status-packets).
Lengths: `d0` (older players), `d4` (nexus), `11c`/`124` (newer firmware,
nexus 2), `11b` (XDJ-1000), `200` (CDJ-3000) (same section); a CDJ-2000nexus
on firmware 1.44 sends `11c`, so length does not identify the generation —
[CAP, captures README](https://github.com/Deep-Symmetry/dysentery/tree/main/doc/assets/captures).
The reference implementation rejects packets shorter than `cc` —
[BL, `CdjStatus.MINIMUM_PACKET_SIZE`](https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/CdjStatus.java);
so do we.

| Offset | Field |
|--------|-------|
| `21`, `24` | Device number |
| `27` | Activity `A` (00 idle, 01 playing/searching/loading) |
| `2a` | Track type `Tr` |
| `7b` | Play state `P1` (03 playing, 04 looping, 05 paused, 06 at cue, 09 searching, ...) |
| `7c`–`7f` | Firmware, ASCII |
| `89` | Status flags `F` ([below](#status-flags)) |
| `8b` | Play state `P2` (`6a`/`7a`/`9a`/`fa` moving, `6e`/`7e`/`9e`/`fe` stopped) |
| `8c`–`8f` | `Pitch1`: the effective pitch |
| `92`–`93` | Track BPM × 100 at the playhead |
| `9e` | `Mm`: 01 master with a rekordbox track, 02 master without one |
| `9f` | `Mh`: `ff`, or the device the master role is being handed to |
| `a0`–`a3` | Beat number from 1 (`ffffffff` without an analysed track) |
| `a6` | Beat within bar 1–4 (0 without an analysed track) |

All from [PA, CDJ Status Packets](https://djl-analysis.deepsymmetry.org/djl-analysis/vcdj.html#cdj-status-packets).
Players older than nexus send `00` for `F`; whether they play must be
inferred from `P1`/`P2` — [BL, `CdjStatus.isPlaying`](https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/CdjStatus.java).

### Status flags

Bit 6 play, bit 5 master, bit 4 sync, bit 3 on air, bit 1 BPM-only sync;
bits 7 and 2 have always been 1 —
[PA, CDJ status flag bits](https://djl-analysis.deepsymmetry.org/djl-analysis/vcdj.html#cdj-status-flag-bits).
The captured playing master in fixture `status-cdj-2000nexus-playing-master`
has `F` = `e4`.

## Mixer status

Port 50002, kind `29`, `38` bytes, unicast —
[PA, Mixer Status Packets](https://djl-analysis.deepsymmetry.org/djl-analysis/vcdj.html#mixer-status-packets):
`21`/`24` device number, `27` flags (`f0` when tempo master, `d0` when not),
`28`–`2b` pitch (always +0 %), `2e`–`2f` BPM × 100 (valid only while a
rekordbox-analysed source plays), `36` `Mh`, `37` beat within bar, which is
not synchronised with the master and arrives at arbitrary times (same
section).

## Tempo master

- A CDJ is master when bit 5 of `F` is set (and `Mm` is non-zero); a mixer
  when its `F` is `f0` —
  [PA, CDJ status](https://djl-analysis.deepsymmetry.org/djl-analysis/vcdj.html#cdj-status-flag-bits),
  [PA, Mixer status](https://djl-analysis.deepsymmetry.org/djl-analysis/vcdj.html#mixer-status-packets).
- During a handoff the outgoing master keeps its master flags and puts the
  incoming device's number in `Mh` until the new master asserts itself —
  [PA, Tempo Master Handoff](https://djl-analysis.deepsymmetry.org/djl-analysis/sync.html#tempo-master-handoff).
  When two devices claim master at once we pick the one not handing off.
- Without status (passive, or before joining) the master is unknown; see
  [above](#broadcast-unicast-and-what-works-passively).
- To get downbeats, take the master from status and its beats from beat
  packets — [PA, CDJ Status Packets](https://djl-analysis.deepsymmetry.org/djl-analysis/vcdj.html#cdj-status-packets).

## Precise position

CDJ-3000 only. Port 50001, kind `0b`, `3c` bytes, byte `1f` = `02`, every
30 ms while a track is loaded, playing or not: `21` device, `24`–`27` track
length in s, `28`–`2b` playhead in ms, `2c`–`2f` pitch × 100 (signed),
`38`–`3b` effective BPM × 10 —
[PA, Absolute Position Packets](https://djl-analysis.deepsymmetry.org/djl-analysis/beats.html#absolute-position-packets);
device number offset and length also in
[BL, `PrecisePosition`](https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/PrecisePosition.java).

The packet has no beat information. Turning the playhead into a beat needs
the track's beat grid —
[PA, Absolute Position Packets](https://djl-analysis.deepsymmetry.org/djl-analysis/beats.html#absolute-position-packets);
the reference implementation looks the playhead up in the beat grid it
downloaded — [BL, `TimeFinder` precise position listener](https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/data/TimeFinder.java).
player5 does not query track metadata, so precise position only refines
the device list's tempo and **never yields `Precision::Exact`**; phase stays
`Fine`, from beat packets. Exact phase would need beat-grid queries (a
future session).

## Channels on air

Port 50001, kind `03`, broadcast by the mixer: `24`–`27` one flag per
channel 1–4 (`01` on air); a six-channel variant (subtype `03`, `35` bytes)
adds channels 5 and 6 at `2d`–`2e` —
[PA, Channels on Air](https://djl-analysis.deepsymmetry.org/djl-analysis/mixer_integration.html#channels-on-air),
offsets as read by [BL, `BeatFinder.getAudibleChannels`](https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/BeatFinder.java).
The diagram shows subtype `00` for four channels; a DJM-2000nexus sends
`02` (fixture `on-air-djm-2000nexus`). Neither parser nor builder depends
on it; the builder copies the hardware.

## What the source reports

- A `SourceEvent::Observation` for every beat packet of the followed device:
  `Phase::Bar(Bb − 1)` (`Phase::Beat(0.0)` from a mixer or when `Bb` is not
  meaningful), effective BPM, `Precision::Fine`, stamped with the host time
  at which `recv_from` returned. No fixed offset is subtracted (none is
  sourced, see [Beat packets](#beat-packets)).
- A `Phase::TempoOnly` observation when the followed device's status shows
  a new effective tempo between beats.
- `SourceEvent::Devices` whenever the device table changes.
- `SourceEvent::Status` for: listening/joining progress, a mixer's
  assignment, giving up our number, no traffic for 5 s (and recovery),
  joined but no status arriving, an Opus Quad on the network, and changes of
  the followed device.
- Follow target: `FollowTarget::Device(n)`, or `FollowTarget::Master` =
  the master from status, else the lowest-numbered playing player.

## Limitations

- **Ports cannot be shared.** `std::net` cannot set `SO_REUSEADDR` /
  `SO_REUSEPORT`, so player5 cannot run on a computer where rekordbox,
  beat-link-based tools or a second player5 already hold 50000–50002;
  `start` fails with `AddrInUse` and says so. Run the bridge on another
  machine, or quit the other program.
- **No interface enumeration.** `std::net` cannot list interfaces, read
  netmasks or MAC addresses. The interface is discovered by connecting a UDP
  socket towards the first peer heard and reading its local address (or set
  `interface`). The broadcast address is assumed: `169.254.255.255` for
  link-local addresses — DJ Link networks without DHCP self-assign in
  169.254/16 ([CAP, captures README](https://github.com/Deep-Symmetry/dysentery/tree/main/doc/assets/captures))
  and broadcast to `169.254.255.255` (fixture `beat-cdj-2000nexus`, [CAP,
  S06](https://github.com/Deep-Symmetry/dysentery/tree/main/doc/assets/captures/S06-load-and-play))
  — and the /24 directed broadcast otherwise. Set `broadcast` for other
  netmasks.
- **MAC address.** The sources say to announce the interface's real MAC —
  [PA, Creating a Virtual CDJ](https://djl-analysis.deepsymmetry.org/djl-analysis/vcdj.html#creating-vcdj).
  Without a configured `mac` we announce `02:70:` + the IPv4 address (a
  locally administered address, unique per host on the subnet). Whether
  any player cares has not been tested against hardware; configure the
  real MAC if one misbehaves.
- **Receiving broadcasts** needs the sockets bound to `0.0.0.0` (the
  default `listen_address`). Two interfaces on the same booth network
  deliver every broadcast twice; the reference implementation warns about
  this — [BL, `VirtualCdj.createVirtualCdj`](https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/VirtualCdj.java).
- **Passive mode** cannot identify the tempo master (see above).
- **Unanalysed tracks** produce no beat packets, so nothing to follow until
  the track is analysed (above).
- **Fine, not Exact.** Phase comes from packet arrival times with network
  and scheduling jitter of a few milliseconds; precise position cannot
  improve it without beat grids.
- **Unverified on hardware:** joining as device 5 next to a CDJ-3000 and a
  DJM-V10, the fallback MAC, and the passive reception of precise position.

## Fixtures

`fixtures/prolink/*.hex`: one packet per file, hex, with a header naming the
capture (file, frame, addresses) or construction and the documenting PA
section. Tests in `core/sync/src/prolink/tests.rs` parse every one and
rebuild the real beats, on-air, mixer status and mixer assignment packets
byte for byte.

| Fixture | Origin |
|---------|--------|
| `keep-alive-cdj-2000nexus`, `keep-alive-djm-2000nexus`, `keep-alive-virtual-cdj`, `beat-djm-2000nexus`, `on-air-djm-2000nexus`, `status-cdj-2000nexus-idle`, `status-djm-2000nexus` | Real, [`to-virtual.pcapng`](https://github.com/Deep-Symmetry/dysentery/blob/main/doc/assets/to-virtual.pcapng) |
| `hello-cdj-2000nexus`, `claim-stage1-cdj-2000nexus` | Real, [`powerup.pcapng`](https://github.com/Deep-Symmetry/dysentery/blob/main/doc/assets/powerup.pcapng) |
| `claim-stage2-cdj-2000nexus`, `claim-stage3-cdj-2000nexus` | Real, [S01-cold-boot-a](https://github.com/Deep-Symmetry/dysentery/tree/main/doc/assets/captures/S01-cold-boot-a) |
| `assignment-intention-djm-2000nexus`, `assignment-request-cdj-2000nexus`, `assignment-djm-2000nexus`, `assignment-finished-cdj-2000nexus` | Real, [`LinkInfo.pcapng`](https://github.com/Deep-Symmetry/dysentery/blob/main/doc/assets/LinkInfo.pcapng) |
| `beat-cdj-2000nexus`, `status-cdj-2000nexus-playing-master` | Real, [S06-load-and-play](https://github.com/Deep-Symmetry/dysentery/tree/main/doc/assets/captures/S06-load-and-play) |
| `player5-hello`, `player5-claim-stage1/2/3`, `player5-keep-alive` | Constructed: golden bytes of what we send (CDJ-3000 variants), checked against the reference templates in [BL, `VirtualCdj`](https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/VirtualCdj.java) |
| `constructed-number-in-use`, `constructed-precise-position-cdj-3000`, `constructed-on-air-6ch`, `constructed-status-cdj-3000` | Constructed from the documented layouts: no published capture contains them |

Regenerate the constructed ones after an intended change with
`UPDATE_PROLINK_FIXTURES=1 cargo test -p sync constructed_fixtures`.

## Other implementations (not used as sources here)

- `prolink-connect` (TypeScript): <https://github.com/EvanPurkhiser/prolink-connect>
- `prolink-go`, `python-prodj-link`, listed in
  [PA, Background](https://djl-analysis.deepsymmetry.org/djl-analysis/packets.html).
