# MIDI clock

Status: digested. Covers MIDI 1.0 System Real-Time clock messages,
receiving them through Web MIDI (browser) and through CoreMIDI's Universal
MIDI Packet input (macOS shell). The DJM-specific behaviour is open (see the
end).

## How the sources were read

- **MIDI 1.0 Detailed Specification** (MIDI Association,
  https://midi.org/midi-1-0-detailed-specification) and its message summary
  (https://midi.org/summary-of-midi-1-0-messages): **midi.org was not
  reachable from the session that wrote this note.** Facts below that rely
  on it are either corroborated by a secondary source that was read (linked
  per fact), or marked **[unverified]**: the author is confident of them
  but did not re-read the primary text. Re-check every [unverified] line
  against the specification before relying on it for anything new.
- **Web MIDI API**, W3C, published at https://www.w3.org/TR/webmidi/. Read
  as the editor's draft source from the Audio Working Group's repository
  (https://github.com/WebAudio/web-midi-api, branch `gh-pages`,
  `index.html`); www.w3.org itself was not reachable. Section links below
  use the anchors defined in that source.
- **Web Audio API**, W3C, published at https://www.w3.org/TR/webaudio/,
  read as the editor's draft source (https://github.com/WebAudio/web-audio-api,
  branch `main`, `index.bs`).
- **DOM Standard**, WHATWG, https://dom.spec.whatwg.org/, read from its
  source (https://github.com/whatwg/dom, `dom.bs`).
- **Universal MIDI Packet (UMP) Format and MIDI 2.0 Protocol**, MIDI
  Association document M2-104-UM,
  https://midi.org/universal-midi-packet-ump-and-midi-2-0-protocol-specification:
  **not read — midi.org was not reachable** (HTTP 403 from the build
  container's proxy, 2026-10-09). The UMP facts below are taken from two
  sources that were read and agree with each other: Apple's CoreMIDI
  headers and an independent MIT-licensed MIDI 2.0 library. Re-check them
  against M2-104-UM when it is reachable.
- **CoreMIDI** (Apple). Reference pages read through Apple's documentation
  JSON (`developer.apple.com/tutorials/data/documentation/coremidi/…`):
  [`MIDIEventPacket`](https://developer.apple.com/documentation/coremidi/midieventpacket),
  [`MIDITimeStamp`](https://developer.apple.com/documentation/coremidi/miditimestamp),
  [`MIDIInputPortCreateWithProtocol`](https://developer.apple.com/documentation/coremidi/midiinputportcreatewithprotocol(_:_:_:_:_:)).
  Header text (with the message-type sizes, which the reference pages do
  not give) read from the macOS 11.3 SDK as mirrored in
  https://github.com/phracker/MacOSX-SDKs (`MacOSX11.3.sdk/System/Library/Frameworks/CoreMIDI.framework/Versions/A/Headers/`,
  files `MIDIMessages.h` and `MIDIServices.h`), via
  `raw.githubusercontent.com`. Line links below point into that mirror.
- **AM MIDI 2.0 Lib** (midi2-dev, MIT), an independent UMP implementation:
  https://github.com/midi2-dev/AM_MIDI2.0Lib, `include/utils.h` and
  `src/umpProcessor.cpp` on `main`.
- Secondary, for MIDI 1.0 facts: JUCE's `MidiMessage` documentation
  (https://github.com/juce-framework/JUCE/blob/master/modules/juce_audio_basics/midi/juce_MidiMessage.h)
  and the mido message table
  (https://github.com/mido/mido/blob/main/docs/message_types.rst).

## The messages

| Status | Name | Length | Meaning (what we rely on) |
|---|---|---|---|
| `0xF8` | Timing Clock | 1 byte | Sent 24 times per quarter note while the master runs. |
| `0xFA` | Start | 1 byte | Start the song from its beginning. |
| `0xFB` | Continue | 1 byte | Resume from the current song position. |
| `0xFC` | Stop | 1 byte | Stop; the song position is kept. |
| `0xF2` | Song Position Pointer | 3 bytes | Song position in "MIDI beats" (sixteenth notes). |
| `0xFE` | Active Sensing | 1 byte | Not used for clock. |
| `0xFF` | System Reset | 1 byte | Not used for clock. |

Per fact:

- `F8`, `FA`, `FB`, `FC`, `FE` and `FF` are complete one-byte messages;
  `F2` is three bytes; `F4`, `F5`, `F9` and `FD` are not valid messages.
  Source: Web MIDI API, Terminology, the non-normative note on valid MIDI
  messages (https://www.w3.org/TR/webmidi/, section "Terminology").
- System Real-Time messages may arrive in the middle of another message;
  Web MIDI dispatches them as they occur and buffers the interrupted
  message until it is complete. Source: Web MIDI API, MIDIInput, the
  paragraph after the `midimessage` steps
  (https://www.w3.org/TR/webmidi/#event-midiinput-message).
- 24 Timing Clocks per quarter note: "there are 24 midi clocks in a
  quarter-note". Source: JUCE `MidiMessage::songPositionPointer`
  documentation (link above). Primary: MIDI 1.0 spec, System Real-Time
  messages **[unverified]**.
- Song Position Pointer counts MIDI beats from the start of the song, one
  MIDI beat = 6 Timing Clocks, so 4 MIDI beats per quarter note. Source:
  JUCE `MidiMessage::songPositionPointer` documentation. The value is 14
  bits (0..16383). Source: mido message table, `songpos` / `pos`.
- Message names Timing Clock / Start / Continue / Stop / Song Position /
  Active Sensing / Reset. Source: mido message table; MIDI 1.0 spec
  **[unverified]**.
- Start begins the song at its beginning (song position 0), Continue
  resumes at the current song position, Stop halts playback and keeps the
  position. Source: MIDI 1.0 spec, System Real-Time messages
  **[unverified]**.
- After Start (or Continue), a receiver begins playing on the **next**
  Timing Clock, which is the first pulse of the (re)started song; the Start
  byte itself carries no timing. Source: MIDI 1.0 spec **[unverified]**.
  This is the rule `MidiClockFollower` implements (beat 0 = first Clock
  after Start); it matches the follower's documented contract in
  `core/sync/src/midi.rs` but should be confirmed against the spec text.
- Song Position Pointer is sent while stopped, typically followed by
  Continue, to move the receiver's song position. Source: MIDI 1.0 spec
  **[unverified]**.

Nothing above says the master's clock phase is continuous across Start:
whether the first Clock after Start lands on the old Clock grid depends on
the sending device (no source found either way). `MidiClockFollower`
therefore treats every Start/Continue as the beginning of a new run of
pulses that shares the tempo estimate but not the phase.

## Web MIDI (Chromium)

- Access: `navigator.requestMIDIAccess()` is only available in a secure
  context. Source: Web MIDI API, Extensions to the Navigator interface
  (https://www.w3.org/TR/webmidi/, section "Extensions to the Navigator
  interface"). The API is a policy-controlled feature named `"midi"` with
  default allowlist `'self'`. Source: same document, section "Permissions
  Policy Integration".
- All Web MIDI interfaces are `[SecureContext, Exposed=(Window,Worker)]`.
  Source: the IDL blocks, e.g. MIDIInput
  (https://www.w3.org/TR/webmidi/#MIDIInput). An AudioWorklet's global
  scope is a `WorkletGlobalScope`
  (`[Global=(Worklet, AudioWorklet), Exposed=AudioWorklet] interface
  AudioWorkletGlobalScope : WorkletGlobalScope`), not a Window or Worker.
  Source: Web Audio API, AudioWorkletGlobalScope
  (https://www.w3.org/TR/webaudio/#AudioWorkletGlobalScope). So MIDI cannot
  be received inside our worklet: the main thread (or a worker) receives
  it and posts it on.
- Each `midimessage` event carries one complete MIDI message in `data`
  (a `Uint8Array`), and its `timeStamp` is set to "the time the message
  was received by the system". Source: Web MIDI API, MIDIInput, the
  `midimessage` steps (https://www.w3.org/TR/webmidi/#event-midiinput-message)
  and MIDIMessageEvent (https://www.w3.org/TR/webmidi/#MIDIMessageEvent).
- `Event.timeStamp` is a `DOMHighResTimeStamp` initialised to the
  *relative high resolution coarse time* for the event's relevant global
  object, i.e. on that global's time origin. Source: DOM Standard,
  Interface Event (https://dom.spec.whatwg.org/#dom-event-timestamp) and
  the event-construction steps that initialise it. The word "coarse"
  means the value may be deliberately coarsened; by how much is defined in
  High Resolution Time, which was not read for this note.
- `MIDIOutput.send()` timestamps are milliseconds relative to the
  document's navigation start. Source: Web MIDI API, MIDIOutput
  (https://www.w3.org/TR/webmidi/#MIDIOutput). (Output only; listed
  because it confirms the time base the API uses.)
- Safari has no Web MIDI; feature-detect `navigator.requestMIDIAccess`.
  Source: ADR-0001's reference, https://caniuse.com/midi (not re-read in
  this session **[unverified]**).

### Mapping a Web MIDI timestamp onto the engine's sample clock

`AudioContext.getOutputTimestamp()` returns `{contextTime,
performanceTime}`: the context time of the sample frame the output device
is currently playing and the `performance.now()`-based time at which it
was played. The specification's own example converts a context time to a
performance time as `performanceTime + (contextTime_x - contextTime) *
1000`, noting that accuracy is best near the current output position.
Source: Web Audio API, `getOutputTimestamp()`
(https://www.w3.org/TR/webaudio/#dom-audiocontext-getoutputtimestamp) and
`AudioTimestamp` (https://www.w3.org/TR/webaudio/#AudioTimestamp).

How player5 uses it (design, not protocol): the main thread inverts that
relation for each message,

```
contextTime(msg) = ts.contextTime + (msg.timeStamp - ts.performanceTime) / 1000
sample(msg)      = contextTime(msg) * sampleRate   // the worklet's frame clock
```

and posts `(status byte, sample)` to the worklet, which calls
`p5_engine_midi`. Because `contextTime` is the *output* position, a
message mapped this way lands where it was heard relative to the audio
leaving the device; output latency compensation stays with the global
latency control (ADR-0001). The worklet-side frame counter and
`contextTime` must share an origin; that is the web shell's job
(ADR-0004).

## CoreMIDI and the Universal MIDI Packet (macOS)

What `apps/mac/Sources/Player5Kit/Clock/MIDIClockInput.swift` relies on.

- An input port created with `MIDIInputPortCreateWithProtocol` receives
  MIDI as a `MIDIEventList` of `MIDIEventPacket`s in UMP form, converted by
  the system to the protocol the port asks for; we ask for MIDI 1.0
  (`kMIDIProtocol_1_0`). Available from macOS 11. The receive block runs on
  a separate high-priority thread owned by CoreMIDI. Source: Apple,
  `MIDIInputPortCreateWithProtocol` reference page;
  [MIDIServices.h L1328–L1361](https://github.com/phracker/MacOSX-SDKs/blob/master/MacOSX11.3.sdk/System/Library/Frameworks/CoreMIDI.framework/Versions/A/Headers/MIDIServices.h#L1328-L1361).
- A `MIDIEventPacket` is `timeStamp` (`MIDITimeStamp`, 64 bits),
  `wordCount` (32 bits), then `words`: native-endian 32-bit UMP words,
  declared as 64 words, but `wordCount` "may be larger than 64 words if
  the packet is dynamically allocated". A 64-bit message never straddles
  two packets. Source: [MIDIServices.h L404–L436](https://github.com/phracker/MacOSX-SDKs/blob/master/MacOSX11.3.sdk/System/Library/Frameworks/CoreMIDI.framework/Versions/A/Headers/MIDIServices.h#L404-L436)
  (structure packed to 4 bytes, `#pragma pack(push, 4)` at L403).
- Packets in a list are variable-length and must be walked, not indexed;
  the next packet starts right after the current packet's `wordCount`
  words (`MIDIEventPacketNext` is `&pkt->words[pkt->wordCount]`). Source:
  [MIDIServices.h L437–L469](https://github.com/phracker/MacOSX-SDKs/blob/master/MacOSX11.3.sdk/System/Library/Frameworks/CoreMIDI.framework/Versions/A/Headers/MIDIServices.h#L437-L469),
  [L2378–L2392](https://github.com/phracker/MacOSX-SDKs/blob/master/MacOSX11.3.sdk/System/Library/Frameworks/CoreMIDI.framework/Versions/A/Headers/MIDIServices.h#L2378-L2392). Our walker therefore
  advances by the full `wordCount`, never by a capped one.
- `MIDITimeStamp` is a host clock time as returned by
  `mach_absolute_time()`; on a received packet it is the time the events
  occurred, and zero means "now". Sources: Apple, `MIDITimeStamp` and
  `MIDIEventPacket` reference pages;
  [MIDIServices.h L227–L239](https://github.com/phracker/MacOSX-SDKs/blob/master/MacOSX11.3.sdk/System/Library/Frameworks/CoreMIDI.framework/Versions/A/Headers/MIDIServices.h#L227-L239),
  [L408–L412](https://github.com/phracker/MacOSX-SDKs/blob/master/MacOSX11.3.sdk/System/Library/Frameworks/CoreMIDI.framework/Versions/A/Headers/MIDIServices.h#L408-L412). The shell converts ticks with
  the same `mach_timebase_info` scaling as `sync::host_time` (ADR-0008),
  and stamps a zero timestamp with the current host time.
- The message type is the top four bits of a UMP message's first word.
  Sources: [MIDIMessages.h L137](https://github.com/phracker/MacOSX-SDKs/blob/master/MacOSX11.3.sdk/System/Library/Frameworks/CoreMIDI.framework/Versions/A/Headers/MIDIMessages.h#L137)
  (`MIDIMessageTypeForUPWord`: `word >> 28`);
  [AM MIDI 2.0 Lib, umpProcessor.cpp L34](https://github.com/midi2-dev/AM_MIDI2.0Lib/blob/main/src/umpProcessor.cpp#L34).
- Message sizes by type, in 32-bit words:

  | Type | Words | Meaning |
  |---|---|---|
  | `0x0` | 1 | Utility |
  | `0x1` | 1 | System Real-Time and System Common (not SysEx) |
  | `0x2` | 1 | MIDI 1.0 Channel Voice |
  | `0x3` | 2 | Data (7-bit SysEx) |
  | `0x4` | 2 | MIDI 2.0 Channel Voice |
  | `0x5` | 4 | Data (128-bit) |
  | `0x6`, `0x7` | 1 | reserved |
  | `0x8`, `0x9`, `0xA` | 2 | reserved |
  | `0xB`, `0xC` | 3 | reserved |
  | `0xD`, `0xE`, `0xF` | 4 | `0xD` Flex Data, `0xF` UMP Stream, `0xE` reserved |

  Sources: [MIDIMessages.h L24–L44](https://github.com/phracker/MacOSX-SDKs/blob/master/MacOSX11.3.sdk/System/Library/Frameworks/CoreMIDI.framework/Versions/A/Headers/MIDIMessages.h#L24-L44) (sizes of
  every type, including the undefined ones); the same grouping in
  [AM MIDI 2.0 Lib, umpProcessor.cpp L37–L38, L129–L130, L232–L233,
  L239–L240](https://github.com/midi2-dev/AM_MIDI2.0Lib/blob/main/src/umpProcessor.cpp#L37-L240) and the type names in
  [utils.h L179–L186](https://github.com/midi2-dev/AM_MIDI2.0Lib/blob/main/include/utils.h#L179-L186). A walker that steps
  by these sizes never mistakes the payload of a longer message for a
  clock message.
- A type-`0x1` message is one word: type in bits 28–31, group in bits
  24–27, the MIDI 1.0 status byte in bits 16–23, then two data bytes in
  bits 8–15 and 0–7. Sources: [MIDIMessages.h L174–L176](https://github.com/phracker/MacOSX-SDKs/blob/master/MacOSX11.3.sdk/System/Library/Frameworks/CoreMIDI.framework/Versions/A/Headers/MIDIMessages.h#L174-L176)
  (`MIDI1UPSystemCommon` packs `status << 16`);
  [AM MIDI 2.0 Lib, umpProcessor.cpp L49–L54](https://github.com/midi2-dev/AM_MIDI2.0Lib/blob/main/src/umpProcessor.cpp#L49-L54)
  (reads `umpMess[0] >> 16 & 0xFF` as the status of a type-`0x1`
  message).
- The System status bytes are the MIDI 1.0 ones: `F8` Timing Clock, `FA`
  Start, `FB` Continue, `FC` Stop, `FE` Active Sensing, `FF` Reset, `F2`
  Song Position Pointer. Source: [MIDIMessages.h L68–L86](https://github.com/phracker/MacOSX-SDKs/blob/master/MacOSX11.3.sdk/System/Library/Frameworks/CoreMIDI.framework/Versions/A/Headers/MIDIMessages.h#L68-L86).

**What we do:** connect one MIDI 1.0 port to every source, walk each packet
by message size, and forward type-`0x1` messages whose status is `F8`,
`FA`, `FB` or `FC` to `p5_control_midi_host` with the packet's host time in
nanoseconds. Everything else is skipped.

## How player5 follows MIDI clock

Code: `core/sync/src/midi.rs` (`MidiClockFollower`), `Precision::Jittery`
in `core/sync/src/follower.rs`, `engine::Control::midi`. Design and tuning:
ADR-0006.

- **Tempo**: least-squares fit of pulse time against pulse index over the
  last 96 pulses (4 beats), one slope shared by all runs (a run begins at
  each Start/Continue) and one intercept per run. With ±1 ms of uniform
  jitter the 1σ error is ≈ 0.035 BPM at 120 BPM after 2 beats and
  ≈ 0.012 BPM once the window is full (tested). While the newest half of
  the window disagrees with the whole by more than 4σ of what the measured
  jitter explains, the period is extrapolated to the newest pulse, so a
  pitch-fader move is not reported two beats late.
- **Phase**: pulses counted since Start; beat 0 = first Clock after Start;
  reported as `Phase::Bar((pulses / 24) mod 4)` while running and
  `Phase::TempoOnly` while stopped (many masters keep sending Clock while
  stopped; the follower keeps the tempo).
- **Re-sync**: `Control::midi` asks the follower for a re-sync on Start and
  Continue, so the first pulse of the new song position snaps the timeline
  instead of slewing to it.
- **Robustness**: a pulse further than half a period from where the fit
  expects it is left out of the tempo fit (still counted for phase); four
  in a row restart the fit; non-increasing timestamps carry no timing; a
  gap longer than one pulse at 20 BPM restarts the fit.

## Open questions (not sourced yet)

- Song Position Pointer is not handled: `sync::MidiMessage` has no
  variant for it, so Continue after an SPP resumes from our own count.
  Adding it is an API change (the enum is matched in `core/ffi`).
- DJM-series MIDI clock output: whether the mixer sends Start/Stop at all,
  whether Clock runs while stopped, and its timing resolution. Needs the
  mixer's MIDI implementation chart (AlphaTheta support site), per model.
- CoreMIDI on real hardware: whether a DJM's USB MIDI driver sends zero
  timestamps (ADR-0008 lists it as untested) and how much jitter its
  timestamps carry. iOS has no MIDI clock input yet.
- The UMP facts above come from Apple's headers and one independent
  library, not from M2-104-UM itself (unreachable); re-check them there.
