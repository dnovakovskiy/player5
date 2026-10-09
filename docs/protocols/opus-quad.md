# Opus Quad

Status: digested with Opus Quad mode. Implementation:
`core/sync/src/opus/` (`packets.rs` parses and builds, `tracker.rs`
estimates beat times, `session.rs` is the source as a pure state machine,
`source.rs` runs it on UDP). Fixtures: [`fixtures/opus-quad/`](fixtures/opus-quad/).

The Opus Quad does not fully support Pro DJ Link, but it does support
rekordbox's "PRO DJ LINK Lighting" even in standalone mode
([analysis README][k-intro]). player5 announces itself as rekordbox in
lighting mode, which makes the unit send CDJ-style status packets for its
four decks ([analysis README §3][k-3]; [beat-link `VirtualRekordbox`][bl-vr]).
Those carry tempo, pitch, play state, the master flag, the beat counter
and the beat within the bar, but in this mode the unit sends no beat
packets and no precise-position packets ([beat-link `VirtualCdj.start`][bl-vc]),
and status packets come only about every 200 ms, so beat timing can be up
to 200 ms off ([beat-link 8.0.0 change log][bl-cl]). player5 therefore
reports [`Precision::Coarse`](../../core/sync/src/follower.rs) observations
and narrows the error itself (see [Timing precision](#timing-precision)).

Nothing here has been checked against hardware by us. Every fact below
carries the source it came from; things we chose are under
[player5 policies](#player5-policies), and what we could not source is under
[Limitations and open questions](#limitations-and-open-questions).

## Sources

Read on 2026-10-09 through `raw.githubusercontent.com` (branch `main`),
because the GitHub web UI and the published dysentery site are not reachable
from our build containers. Links point at the published locations.

- Kyle Awayan, *OPUS-QUAD Pro DJ Link Reverse Engineer Packet Analysis*:
  [README.md][k-readme], [pro-dj-link.js][k-js], [index.js][k-index].
  Packet bytes in the JavaScript are what rekordbox sends, per the README.
- Deep Symmetry, beat-link (Java), the Opus Quad support released in 8.0.0:
  [`VirtualRekordbox.java`][bl-vr], [`VirtualCdj.java`][bl-vc],
  [`CdjStatus.java`][bl-cs], [`DeviceUpdate.java`][bl-du],
  [`DeviceAnnouncement.java`][bl-da], [`DeviceFinder.java`][bl-df],
  [`Util.java`][bl-util], [`data/OpusProvider.java`][bl-op],
  [`CHANGELOG.md`][bl-cl].
- Deep Symmetry, *DJ Link Ecosystem Analysis* (dysentery), read from the
  dysentery repository's `doc/modules/ROOT/pages/*.adoc` and cited at the
  published URLs: [packets][dy-packets], [startup][dy-startup],
  [vcdj][dy-vcdj], [beats][dy-beats].

## Ports and header

- Every DJ Link packet starts with the ten bytes
  `51 73 70 74 31 57 6d 4a 4f 4c`, followed by a kind byte (offset `0a`)
  that together with the port identifies the packet ([dysentery: packet
  types][dy-packets]).
- Port 50000 carries announcements, kind `06` being the keep-alive
  ([dysentery: packet types][dy-packets]).
- Port 50002 carries device status, kind `0a` being the CDJ status packet
  ([dysentery: packet types][dy-packets]).
- On port 50002 the Opus Quad conversation also uses kind `10` (the unit's
  "rekordbox lighting hello", [beat-link `Util.PacketType`][bl-util]), kind
  `11` (rekordbox's request, [analysis README §3][k-3]), kind `55` (a
  phrase-data request, [analysis README §3][k-3]) and kind `56` (binary
  metadata from the unit, [analysis README: metadata][k-meta]).
- In port-50000 packets the device name occupies bytes `0c`–`1f`
  ([beat-link `VirtualRekordbox.DEVICE_NAME_OFFSET`/`LENGTH`][bl-vr]); in
  port-50002 packets it occupies bytes `0b`–`1e`, byte `1f` is `01` and
  byte `20` is a structure variant ([dysentery: CDJ status
  packet][dy-status-packet]).

## Joining as rekordbox lighting

### The keep-alive

Broadcast to port 50000. Both sources publish the same 54 bytes
([beat-link `rekordboxKeepAliveBytes`][bl-vr]; [`sendKeepAlive()`][k-js]);
fixture [`rekordbox-keep-alive.hex`](fixtures/opus-quad/rekordbox-keep-alive.hex).

| Bytes   | Value                      | Meaning | Source |
|---------|----------------------------|---------|--------|
| `00`–`09` | magic                    | DJ Link header | [dysentery][dy-packets] |
| `0a`    | `06`                       | keep-alive | [dysentery][dy-packets] |
| `0b`    | `00`                       | | [beat-link][bl-vr], [analysis][k-js] |
| `0c`–`1f` | `rekordbox`, NUL-padded  | device name | [beat-link][bl-vr] |
| `20`    | `01`                       | constant in the keep-alive layout | [dysentery: CDJ keep-alive][dy-keepalive] |
| `21`    | `03`                       | structure variant (CDJs show `02`) | [beat-link][bl-vr]; [dysentery][dy-keepalive] |
| `22`–`23` | `00 36`                  | packet length | [dysentery][dy-keepalive] |
| `24`    | `17`                       | device number | [beat-link `DEVICE_NUMBER_OFFSET`][bl-vr] |
| `25`    | `01`                       | (on a CDJ: whether it booted alone) | [beat-link][bl-vr]; [dysentery][dy-keepalive] |
| `26`–`2b` | MAC                      | our interface's MAC | [beat-link `MAC_ADDRESS_OFFSET`, `createVirtualRekordbox`][bl-vr] |
| `2c`–`2f` | IPv4                     | our interface's address | [beat-link `createVirtualRekordbox`][bl-vr] |
| `30`    | `04`                       | peer count on a CDJ; a constant in both rekordbox sources | [dysentery][dy-keepalive]; [beat-link][bl-vr] |
| `31`–`35` | `01 00 00 04 08`         | byte `34` is `01` on CDJs and `02` on mixers | [beat-link][bl-vr]; [dysentery][dy-keepalive] |

- The packet must be exactly this; with a mistake in it the unit neither
  sends metadata nor answers phrase requests ([analysis README §2][k-2]).
- It must be repeated "every few seconds" to stay on the network
  ([analysis README §2][k-2]); the analysis script sends it every 2 s
  ([index.js][k-index]); beat-link every 1.5 s by default, settable from
  200 to 2000 ms ([beat-link `announceInterval`][bl-vr]).
- The analysis script broadcasts to the /24 broadcast address of the
  interface ([`pro-dj-link.js` constructor][k-js]); beat-link to the
  broadcast address of the interface that sees the unit
  ([beat-link `createVirtualRekordbox`][bl-vr]).
- rekordbox also sends first- and second-stage number claims on launch, but
  the unit still sends metadata without them ([analysis README §2][k-2]).
  player5 does not send them.

### Device number

- Both sources announce `0x17` ([beat-link][bl-vr]; [analysis][k-js]).
- beat-link keeps that number unless another device uses it, in which
  case it takes the first unused number in `0x13`–`0x27`, "higher than two
  rekordbox laptops would use, and less than rekordbox mobile uses"
  ([beat-link `selfAssignDeviceNumber`][bl-vr]).
- rekordbox's own status packets carry device number `0x11`, rekordbox
  mobile `0x29` ([dysentery: rekordbox status packets][dy-rb-status]).

### The lighting request (0x11)

Sent to the unit's port 50002, it makes the unit start sending status
packets ([analysis README §3][k-3]). Both sources publish the same 296
bytes for the computer name "macbook pro" ([beat-link
`rekordboxLightingRequestStatusBytes`][bl-vr]; [`sendCdj()`][k-js]);
fixture [`rekordbox-lighting-request.hex`](fixtures/opus-quad/rekordbox-lighting-request.hex).

| Bytes   | Value | Meaning | Source |
|---------|-------|---------|--------|
| `00`–`0a` | magic, `11` | | [analysis][k-js] |
| `0b`–`1e` | `rekordbox`, NUL-padded | device name | [analysis][k-js] |
| `1f`–`28` | `01 01 17 01 04 17 01 00 00 00` | | [analysis][k-js] |
| `29`–`127` | computer name, one ASCII byte at every other offset | | [`encodeWeirdString`][k-js] |

- `17` at `21` and `24` sits where status packets carry the device number
  ([dysentery: CDJ status packet][dy-status-packet]), and `01 04` at
  `22`–`23` equals the bytes after offset `24` (`0x128 − 0x24`), the
  status packets' _len~r~_ ([dysentery][dy-status-packet]). Both readings
  are ours; player5 writes its own device number into `21` and `24`.
- The computer name "does not affect functionality" ([analysis README:
  example script][k-script]).
- The analysis script sends the request once, to the unit's address, when
  the unit's first port-50002 packet arrives ([index.js][k-index]);
  beat-link broadcasts it with every keep-alive ([beat-link
  `sendAnnouncements`, `sendRekordboxLightingPacket`][bl-vr]).
- beat-link writes its MAC over bytes `26`–`2b` and its address over
  `2c`–`2f` of this packet, overwriting part of the name ([beat-link
  `createVirtualRekordbox`][bl-vr]); the analysis script does not. That
  both implementations work suggests the unit ignores these bytes (our
  inference).
- rekordbox also sends mixer-status-like packets on port 50002, but the
  unit still sends metadata without them ([analysis README §3][k-3]).
  player5 does not send them.

## What the unit sends

- A keep-alive on port 50000 every few seconds, starting
  `51 73 70 74 31 57 6d 4a 4f 4c 06` and similar to a CDJ keep-alive
  ([analysis README §2][k-2]). Its name is `OPUS-QUAD`
  ([beat-link `OpusProvider.OPUS_NAME`][bl-op]); beat-link expects 54 bytes
  and turns each such keep-alive into four devices numbered 1–4
  ([beat-link `DeviceAnnouncement`][bl-da]; [`createAndProcessOpusAnnouncements`][bl-df]).
  No source quotes its bytes; [`constructed-opus-keep-alive.hex`](fixtures/opus-quad/constructed-opus-keep-alive.hex)
  follows the CDJ layout with invented values.
- Contradicting that, dysentery says the Opus Quad "exposes three device
  IDs on the network (1 for Player 1, 2 for Player 2, and 33 for the mixer
  section)" ([dysentery: USB-to-host network][dy-xz]). player5 identifies
  the unit by name, so either form is found.
- After it sees our keep-alive, kind-`10` packets to our port 50002, which
  do not yet contain deck status ([analysis README §3][k-3]). Their first
  36 bytes are published: name `OPUS-QUAD`, byte `21` = `09`
  ([`firstPacketOn50002FromOpusQuad`][k-js]; fixture
  [`opus-lighting-hello-prefix.hex`](fixtures/opus-quad/opus-lighting-hello-prefix.hex)).
  The unit stops sending them and starts sending status once it gets the
  lighting request ([beat-link `Util.PacketType`][bl-util]); beat-link reads
  a kind-`10` packet of at least `0xcc` bytes as a status packet
  ([beat-link `buildUpdate`][bl-vr]).
- After the lighting request, CDJ status packets on port 50002
  ([analysis README §3][k-3]), with the layout dysentery documents, though
  not every value is filled in: the USB slot and looping status are missing
  ([analysis README: CDJ statuses][k-statuses]).
- On track load, fragmented kind-`56` packets (album art and two unknown
  types), about a second after the load ([analysis README: metadata][k-meta]).
  player5 ignores them.
- Track IDs refer to the Device Library Plus database, not the classic
  export ([analysis README: how it works][k-how]). player5 does not use them.

## Deck numbers

- Decks 1–4 report device numbers 9–12 ([analysis README §3][k-3];
  [CDJ statuses][k-statuses]).
- beat-link maps them with `number & 7` ([beat-link
  `translateOpusPlayerNumbers`][bl-util]), only for packets named
  `OPUS-QUAD` ([beat-link `DeviceUpdate`][bl-du]).

## Status packet fields

Offsets in the CDJ status packet that player5 reads, all from [dysentery:
CDJ status packets][dy-status]:

| Bytes   | Field | Meaning | Source |
|---------|-------|---------|--------|
| `21`    | _D_   | device number | [layout][dy-status-packet] |
| `2c`–`2f` | _rekordbox_ | track ID | [layout][dy-status-packet] |
| `7b`    | _P~1~_ | `03` playing, `04` looping, `05` paused, `06` cued, `09` searching, ... | [P~1~ values][dy-p1] |
| `89`    | _F_   | bit 6 play, bit 5 master, bit 4 sync, bit 3 on air, bit 1 BPM-only sync | [flag bits][dy-flags] |
| `8b`    | _P~2~_ | `7a` moving / `7e` stopped; pre-nexus `6a`/`6e`, nxs2 `fa`/`fe`, XDJ-XZ `9a`/`9e` | [flag bits section][dy-flags] |
| `8c`–`8f` | _Pitch~1~_ | effective pitch, `100000` = ±0 %, `0` = stopped, `200000` = +100 % | [flag bits section][dy-flags] |
| `92`–`93` | _BPM_ | track tempo × 100; `ffff` with no track | [current track BPM][dy-bpm] |
| `9e`    | _M~m~_ | master "meaningful" | [P~3~ values section][dy-p3] |
| `a0`–`a3` | _Beat_ | beat counter from 1; `0` while paused at the start; `ffffffff` without a rekordbox-analysed track | [P~3~ values section][dy-p3] |
| `a6`    | _B~b~_ | beat within the bar, 1–4; `0` without an analysed track | [P~3~ values section][dy-p3] |

- Effective tempo = _BPM_ / 100 × _Pitch~1~_ / `0x100000`
  ([dysentery: current track BPM][dy-bpm]).
- Status packets are `d4` bytes on nexus players, `d0` on older ones,
  `11c`/`124` on newer firmware and `200` on the CDJ-3000
  ([dysentery: CDJ status packets][dy-status]); the Opus Quad's length is
  not published. beat-link refuses packets under `0xcc` bytes
  ([beat-link `CdjStatus.MINIMUM_PACKET_SIZE`][bl-cs]) and trusts _F_ for
  the play state only from `0xd4` bytes, inferring it from _P~1~_/_P~2~_
  in shorter packets ([beat-link `CdjStatus.isPlaying`][bl-cs]).
- The analysis script finds the master deck by `packet[0x89] & 32` and
  computes its tempo from `0x92` and `0x8d` ([`scanForNeededBytesForMixerStatus`][k-js]),
  so the master flag and tempo fields are live on the Opus Quad.

## Quirks

- The unit sometimes sends a status packet whose _F_ byte is zero; beat-link
  reuses the last non-zero _F_ from the same device, and drops the packet
  if it has none ([beat-link `buildUpdate`][bl-vr]). player5 does the same
  (fixture [`constructed-opus-status-zero-flags.hex`](fixtures/opus-quad/constructed-opus-status-zero-flags.hex)).
- The unit has been seen reporting _P~2~_ = `fa` while playing although "the
  main status flag lies about that fact" ([beat-link
  `CdjStatus.PlayState2.OPUS_MOVING`][bl-cs]). player5 counts a deck as
  playing when the play bit is set or _P~2~_ is one of the moving values.

## Timing precision

- Status packets come about every 200 ms ([dysentery: creating a virtual
  CDJ][dy-vcdj-create]).
- A CDJ broadcasts a beat packet on every beat, so its arrival marks the
  beat ([dysentery: beat packets][dy-beat-packets]); the CDJ-3000 also sends
  absolute-position packets every 30 ms ([dysentery: absolute
  position][dy-abs]). In
  lighting mode the Opus Quad sends neither ([beat-link
  `VirtualCdj.start`][bl-vc]), so beat-link may be "up to 200ms out of sync
  with beats" ([beat-link 8.0.0 change log][bl-cl]).

What player5 does with that (design, not protocol):

1. When a deck's beat counter goes from _n_ to _n_ + 1 between two packets
   received at _t~0~_ and _t~1~_, beat _n_ + 1 started in [_t~0~_, _t~1~_]
   (widened by 4 ms for receive jitter). Its midpoint is a first estimate,
   good to about ±100 ms.
2. The brackets of up to 16 previous beats are shifted forward by whole
   beat periods at the current tempo and intersected with the newest one.
   Packet and beat periods are not commensurate, so the intersection
   narrows to a few tens of milliseconds within a couple of bars (the unit
   tests see under 40 ms at 128 BPM with 200 ms packets; when the periods
   are commensurate, e.g. 120 BPM against exactly 200 ms, it settles near
   ±50 ms). Brackets that no longer agree, a tempo change over 0.05 %, a
   pause, a jump in the counter or a gap over 1 s drop the history.
3. Each estimate becomes one `SourceEvent::Observation`: `host_ns` at the
   estimated beat start (so up to a packet interval in the past),
   `Phase::Bar(b − 1)` when _B~b~_ = _b_ is known, else `Phase::Beat(0.0)`,
   the effective tempo, `Precision::Coarse` and the deck number. Nothing is
   reported while the followed deck is stopped, so followers lose lock and
   free-run.
4. A constant network or processing delay shifts every estimate equally;
   the global latency offset absorbs it.

## Alternatives not taken

- Announced with a CDJ keep-alive instead, the unit does send absolute
  position packets, but then sends no metadata on load and ignores phrase
  requests ([analysis README: absolute position packets][k-abs]). beat-link
  uses those packets to fake beat packets in its "SQLite mode"
  ([beat-link `VirtualCdj.start`][bl-vc]). That path would give much finer
  timing through the Pro DJ Link source; it is not part of this mode.

## player5 policies

Choices of ours, with the reasoning:

- **Interface.** When not configured, the interface address is the local
  address of a UDP socket `connect()`ed toward the unit, which only picks a
  route and sends nothing ([udp(7)][udp7]). Until the unit is seen, nothing
  is announced; the unit's own keep-alive (or its kind-`10` packet) reveals
  it.
- **MAC.** When not configured: `02:50:a:b:c:d` for interface address
  `a.b.c.d`, deterministic and unique per address on the LAN. The first
  octet sets the locally administered bit and clears the multicast bit
  ([MAC address: U/L bit][mac-ul]). Pass the real MAC when you can; whether
  the unit cares is unknown.
- **Destination.** The configured broadcast address, else
  `169.254.255.255` on a link-local interface (`169.254.0.0/16`, the range
  hosts configure themselves in when no DHCP server answers,
  [RFC 3927][rfc3927]), else the
  /24 broadcast address as the analysis script does ([`pro-dj-link.js`][k-js]).
  On loopback (tests) the address itself.
- **Cadence.** Keep-alive and lighting request together every 1.5 s
  (beat-link's default), configurable within beat-link's 200–2000 ms, plus
  immediately when the unit is first seen. The request goes unicast to the
  unit.
- **Device number.** `0x17`; on conflict the first free number in
  `0x13`–`0x27`, as beat-link does, but immediately rather than after a
  4 s watch. When the other device is also named `rekordbox`, only the one
  with the higher MAC moves, so two player5s settle instead of leapfrogging.
- **Deck numbers.** 9–12 map to decks 1–4; 1–4 are kept as they are (in
  case of dysentery's numbering); anything else is ignored, because
  beat-link's `& 7` would alias dysentery's mixer ID 33 to deck 1.
- **Device table.** Once the unit is seen, four `AllInOne` entries named
  `OPUS-QUAD`, numbered 1–4, with tempo, play, master and on-air filled in
  from status; other keep-alives are listed by name (`rekordbox`) or by
  byte `34`. Entries expire after 10 s of silence, beat-link's
  `MAXIMUM_AGE` ([beat-link `DeviceFinder`][bl-df]), about when a CDJ's
  peer count drops ([dysentery: CDJ keep-alive][dy-keepalive]).
- **Follow target.** `FollowTarget::Device(n)` follows deck _n_.
  `FollowTarget::Master` follows the playing tempo master (staying on the
  current one during a hand-off), else the deck already followed while it
  plays, else the only playing deck, else a stopped master. The point is
  the deck people hear.
- **Second unit.** Packets from a second `OPUS-QUAD` address are ignored
  until the first expires.

## Limitations and open questions

- No packet capture of an Opus Quad is available to us. The unit's
  keep-alive and status packets in the fixtures are constructed from the
  documented layouts; the real status length, subtype byte and keep-alive
  device number are unknown.
- Not verified on hardware: whether the unit accepts a fabricated MAC,
  whether it fills in _B~b~_ (if not, observations fall back to
  `Phase::Beat`), and how many device IDs current firmware announces.
- player5 binds UDP 50000 and 50002, so it cannot run next to rekordbox on
  the same computer, nor at the same time as player5's Pro DJ Link source.
- One unit per network.
- Phase is coarse by nature: with steady tempo the estimate settles to a
  few tens of milliseconds, but after a pitch move, a nudge or a loop it
  is back to about ±100 ms for a beat or two.

## Fixtures

All in [`fixtures/opus-quad/`](fixtures/opus-quad/): `#` header lines
naming the source, then the UDP payload in hex, 16 bytes per line.

| File | What | Origin |
|------|------|--------|
| `rekordbox-keep-alive.hex` | our keep-alive | quoted, beat-link and the analysis |
| `rekordbox-lighting-request.hex` | our lighting request | quoted, beat-link and the analysis |
| `opus-lighting-hello-prefix.hex` | first 36 bytes of the unit's kind-`10` packet | quoted, the analysis |
| `constructed-opus-keep-alive.hex` | the unit's keep-alive | constructed |
| `constructed-opus-status-deck1-master.hex` | deck 1 playing as master | constructed |
| `constructed-opus-status-zero-flags.hex` | deck 2 with a zero _F_ byte | constructed |

[k-readme]: https://github.com/kyleawayan/opus-quad-pro-dj-link-analysis/blob/main/README.md
[k-intro]: https://github.com/kyleawayan/opus-quad-pro-dj-link-analysis/blob/main/README.md#opus-quad-pro-dj-link-reverse-engineer-packet-analysis
[k-script]: https://github.com/kyleawayan/opus-quad-pro-dj-link-analysis/blob/main/README.md#example-script
[k-how]: https://github.com/kyleawayan/opus-quad-pro-dj-link-analysis/blob/main/README.md#how-it-works
[k-2]: https://github.com/kyleawayan/opus-quad-pro-dj-link-analysis/blob/main/README.md#2-initialization
[k-3]: https://github.com/kyleawayan/opus-quad-pro-dj-link-analysis/blob/main/README.md#3-cdj-status-packets-and-metadata
[k-statuses]: https://github.com/kyleawayan/opus-quad-pro-dj-link-analysis/blob/main/README.md#cdj-statuses
[k-abs]: https://github.com/kyleawayan/opus-quad-pro-dj-link-analysis/blob/main/README.md#absolute-position-packets
[k-meta]: https://github.com/kyleawayan/opus-quad-pro-dj-link-analysis/blob/main/README.md#metadata-on-song-load
[k-js]: https://github.com/kyleawayan/opus-quad-pro-dj-link-analysis/blob/main/pro-dj-link.js
[k-index]: https://github.com/kyleawayan/opus-quad-pro-dj-link-analysis/blob/main/index.js
[bl-vr]: https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/VirtualRekordbox.java
[bl-vc]: https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/VirtualCdj.java
[bl-cs]: https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/CdjStatus.java
[bl-du]: https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/DeviceUpdate.java
[bl-da]: https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/DeviceAnnouncement.java
[bl-df]: https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/DeviceFinder.java
[bl-util]: https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/Util.java
[bl-op]: https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/data/OpusProvider.java
[bl-cl]: https://github.com/Deep-Symmetry/beat-link/blob/main/CHANGELOG.md#800---2025-07-21
[dy-packets]: https://djl-analysis.deepsymmetry.org/djl-analysis/packets.html#packet-types
[dy-startup]: https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html
[dy-keepalive]: https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#cdj-keep-alive
[dy-xz]: https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#xdj-xz-usb-network
[dy-vcdj]: https://djl-analysis.deepsymmetry.org/djl-analysis/vcdj.html
[dy-vcdj-create]: https://djl-analysis.deepsymmetry.org/djl-analysis/vcdj.html#creating-vcdj
[dy-status]: https://djl-analysis.deepsymmetry.org/djl-analysis/vcdj.html#cdj-status-packets
[dy-status-packet]: https://djl-analysis.deepsymmetry.org/djl-analysis/vcdj.html#cdj-status-packet
[dy-p1]: https://djl-analysis.deepsymmetry.org/djl-analysis/vcdj.html#known-p1-values
[dy-flags]: https://djl-analysis.deepsymmetry.org/djl-analysis/vcdj.html#cdj-status-flag-bits
[dy-bpm]: https://djl-analysis.deepsymmetry.org/djl-analysis/vcdj.html#current-track-bpm
[dy-p3]: https://djl-analysis.deepsymmetry.org/djl-analysis/vcdj.html#known-p3-values
[dy-rb-status]: https://djl-analysis.deepsymmetry.org/djl-analysis/vcdj.html#rekordbox-status-packets
[dy-beats]: https://djl-analysis.deepsymmetry.org/djl-analysis/beats.html
[dy-beat-packets]: https://djl-analysis.deepsymmetry.org/djl-analysis/beats.html#beat-packets
[dy-abs]: https://djl-analysis.deepsymmetry.org/djl-analysis/beats.html#absolute-position-packets
[udp7]: https://man7.org/linux/man-pages/man7/udp.7.html
[mac-ul]: https://en.wikipedia.org/wiki/MAC_address#Universal_vs._local_(U/L_bit)
[rfc3927]: https://www.rfc-editor.org/rfc/rfc3927
