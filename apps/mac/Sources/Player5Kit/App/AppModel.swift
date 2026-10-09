import Foundation
import SwiftUI

#if os(macOS)
    import CoreAudio
#endif

/// App state and the one place UI actions go. Use from the main thread
/// only (SwiftUI views, and the engine's callbacks, which arrive there).
///
/// The pattern is the source of truth: every edit is normalized, saved to
/// UserDefaults and sent whole to the core as pattern JSON (the web app does
/// the same; the core only forwards controls that changed).
public final class AppModel: ObservableObject {
    // MARK: Pattern

    @Published public private(set) var pattern: Pattern
    @Published public var selectedVoice: Voice = .kick
    /// In flam mode a step tap toggles the flam instead of cycling.
    @Published public var flamMode = false

    // MARK: Engine readouts

    @Published public private(set) var isPlaying = false
    @Published public private(set) var playingStep = -1
    @Published public private(set) var tempo: Double = 0
    @Published public private(set) var bar = 1
    @Published public private(set) var beatInBar = 1
    @Published public private(set) var locked = true
    @Published public private(set) var sourceStatus = ""
    @Published public private(set) var devices: [NetworkDevice] = []
    @Published public private(set) var audioStatus: AudioStatus = .off
    @Published public private(set) var deviceLatencyMs: Double = 0

    // MARK: Clock settings

    @Published public private(set) var clock: ClockSettings
    /// Explains a clock that could not start (shown in the clock panel).
    @Published public private(set) var clockMessage: String? = nil

    // MARK: Library and sharing

    @Published public private(set) var library: [SavedPattern]
    /// The web app's address, used as the base of shared links.
    @Published public var webAppURL: String {
        didSet { store.webAppURL = webAppURL }
    }
    @Published public private(set) var shareMessage: String? = nil

    #if os(macOS)
        // MARK: macOS audio and MIDI

        @Published public private(set) var outputDevices: [OutputDevice] = []
        /// Chosen output device UID; `nil` = system default.
        @Published public private(set) var outputDeviceUID: String? = nil
        @Published public private(set) var midiSources: [String] = []
        private var midiInput: MIDIClockInput? = nil
        /// The device the engine was last pointed at (the chosen one, or
        /// the system default at that moment).
        private var appliedOutputDeviceID: AudioDeviceID? = nil
    #endif

    private let store: SettingsStore
    private let engine: EngineHost
    private var lastDevicesJSON = "[]"

    public init(defaults: UserDefaults = .standard) {
        let store = SettingsStore(defaults: defaults)
        let pattern = store.loadPattern() ?? Presets.initial
        self.store = store
        self.pattern = pattern
        self.clock = store.loadClock()
        self.library = store.loadLibrary()
        self.webAppURL = store.webAppURL
        self.engine = EngineHost()
        #if os(macOS)
            self.outputDeviceUID = store.outputDeviceUID
        #endif
        tempo = pattern.bpm

        engine.onSnapshot = { [weak self] snapshot in
            self?.apply(snapshot)
        }
        engine.onAudioStatus = { [weak self] status in
            self?.audioStatusChanged(status)
        }
        engine.onClockResult = { [weak self] kind, result in
            self?.clockResult(kind, result)
        }

        engine.load(patternJSON: pattern.jsonString())
        engine.setNudge(ms: clock.nudgeMs)
        engine.follow(device: clock.followDevice)
        applyLatency()

        #if os(macOS)
            refreshOutputDevices()
            OutputDevices.observeChanges { [weak self] in
                self?.refreshOutputDevices()
            }
        #endif

        // A saved external clock needs audio running (host-time mapping), so
        // restoring it starts audio; otherwise audio starts on first Play.
        let saved = clock.kind
        selectClock(ClockKind.available.contains(saved) ? saved : .internalClock)
    }

    // MARK: Audio

    /// Whether audio output is running.
    public var audioRunning: Bool {
        if case .running = audioStatus {
            return true
        }
        return false
    }

    /// Starts audio output if it is not running.
    public func ensureAudio() {
        if !audioRunning {
            engine.startAudio()
        }
    }

    /// One-line audio state for the header.
    public var audioStatusText: String {
        switch audioStatus {
        case .off:
            return "audio off — press Play"
        case .running(let sampleRate, _):
            return String(format: "audio on · %.1f kHz", sampleRate / 1_000)
        case .failed(let reason):
            return "audio: \(reason)"
        }
    }

    private func audioStatusChanged(_ status: AudioStatus) {
        audioStatus = status
        if case .running(_, let latency) = status, latency.isFinite {
            deviceLatencyMs = latency
            applyLatency()
        }
    }

    // MARK: Transport

    public func togglePlay() {
        setPlaying(!isPlaying)
    }

    public func setPlaying(_ playing: Bool) {
        if playing {
            ensureAudio()
        }
        isPlaying = playing
        engine.setPlaying(playing)
    }

    public func setBPM(_ bpm: Double) {
        update { $0.bpm = (bpm * 100).rounded() / 100 }
    }

    public func nudgeBPM(by delta: Double) {
        setBPM(pattern.bpm + delta)
    }

    /// Tap tempo. The tap is timestamped now, on the main thread, so queue
    /// latency does not skew it. On the internal clock the tapped tempo
    /// becomes the pattern's BPM.
    public func tap() {
        let now = HostClock.nowNanoseconds()
        ensureAudio()
        engine.tap(hostNanoseconds: now) { [weak self] tempo in
            guard let self = self, let tempo = tempo, tempo.isFinite, tempo > 0 else { return }
            let rounded = (tempo * 100).rounded() / 100
            if abs(rounded - self.pattern.bpm) >= 0.01 {
                self.update { $0.bpm = rounded }
            }
        }
    }

    // MARK: Pattern editing

    /// Applies an edit: normalize, save, send to the core.
    public func update(_ change: (inout Pattern) -> Void) {
        var next = pattern
        change(&next)
        next.normalize()
        guard next != pattern else { return }
        pattern = next
        store.savePattern(next)
        engine.load(patternJSON: next.jsonString())
        // Without audio there are no engine snapshots; keep the readout true.
        if !audioRunning && !clock.kind.isExternal && tempo != next.bpm {
            tempo = next.bpm
        }
    }

    /// A step tap: cycles off → hit → accent, or toggles flam in flam mode.
    public func tapStep(_ voice: Voice, _ index: Int) {
        guard index >= 0, index < Step.count else { return }
        let flam = flamMode
        update { p in
            let step = p[voice].steps[index]
            p[voice].steps[index] = flam ? step.flamToggled : step.cycled
        }
    }

    public func toggleMute(_ voice: Voice) {
        update { $0[voice].mute.toggle() }
    }

    public func clear(_ voice: Voice) {
        update { $0[voice].steps = Array(repeating: .off, count: Step.count) }
    }

    public func clearAll() {
        update { $0.clearSteps() }
    }

    public func load(_ preset: Preset) {
        replacePattern(preset.pattern)
    }

    public func replacePattern(_ newPattern: Pattern) {
        update { $0 = newPattern }
    }

    /// A binding to a pattern-level value (shuffle, accent, flam, gain…).
    public func binding(_ keyPath: WritableKeyPath<Pattern, Double>) -> Binding<Double> {
        Binding(
            get: { self.pattern[keyPath: keyPath] },
            set: { value in self.update { $0[keyPath: keyPath] = value } })
    }

    /// A binding to one voice control.
    public func binding(_ voice: Voice, _ control: VoiceControl) -> Binding<Double> {
        let keyPath = control.keyPath
        return Binding(
            get: { self.pattern[voice][keyPath: keyPath] },
            set: { value in self.update { $0[voice][keyPath: keyPath] = value } })
    }

    /// A binding to the limiter switch.
    public var limiterBinding: Binding<Bool> {
        Binding(
            get: { self.pattern.limiter },
            set: { value in self.update { $0.limiter = value } })
    }

    // MARK: Clock

    /// Selects a clock source. Sources the core lacks or that fail fall back
    /// to the internal clock with an explanation in `clockMessage`.
    public func selectClock(_ kind: ClockKind) {
        clockMessage = nil
        var settings = clock
        settings.kind = kind
        setClockSettings(settings)
        if kind.isExternal {
            ensureAudio()
        }
        #if os(macOS)
            if kind == .midi {
                startMIDIIfNeeded()
            }
        #endif
        engine.setClock(kind, simulatedBPM: pattern.bpm) { [weak self] result in
            self?.clockResult(kind, result)
        }
    }

    /// Which device to follow: 0 = tempo master.
    public func follow(device: Int) {
        var settings = clock
        settings.followDevice = device
        setClockSettings(settings)
        engine.follow(device: device)
    }

    /// Moves the phase nudge by `ms` (positive = later).
    public func nudge(by ms: Double) {
        setNudge(clock.nudgeMs + ms)
    }

    public func setNudge(_ ms: Double) {
        var settings = clock
        settings.nudgeMs = Pattern.clamp((ms * 10).rounded() / 10, -250...250, fallback: 0)
        setClockSettings(settings)
        engine.setNudge(ms: settings.nudgeMs)
    }

    /// The user's latency offset in ms (positive = trigger earlier).
    public func setLatency(_ ms: Double) {
        var settings = clock
        settings.latencyMs = Pattern.clamp(ms.rounded(), -100...500, fallback: 0)
        setClockSettings(settings)
        applyLatency()
    }

    public func setCompensateDeviceLatency(_ on: Bool) {
        var settings = clock
        settings.compensateDeviceLatency = on
        setClockSettings(settings)
        applyLatency()
    }

    /// Offset sent to the core: the user's value plus, optionally, what the
    /// output hardware reports.
    public var totalLatencyMs: Double {
        clock.latencyMs + (clock.compensateDeviceLatency ? deviceLatencyMs : 0)
    }

    public func resync() {
        engine.resync()
    }

    private func applyLatency() {
        engine.setLatency(ms: totalLatencyMs)
    }

    private func setClockSettings(_ settings: ClockSettings) {
        if clock != settings {
            clock = settings
        }
        store.saveClock(settings)
    }

    private func clockResult(_ kind: ClockKind, _ result: ClockStartResult) {
        switch result {
        case .ok:
            return
        case .notInThisBuild:
            clockMessage = "\(kind.title) is not in this build yet; running on the internal clock."
        case .failed(let reason):
            clockMessage = "\(kind.title) could not start (\(reason)); running on the internal clock."
        }
        // The engine has fallen back already; mirror it, unless the user has
        // picked something else in the meantime.
        if clock.kind == kind {
            var settings = clock
            settings.kind = .internalClock
            setClockSettings(settings)
        }
    }

    private func apply(_ s: EngineSnapshot) {
        if playingStep != s.step {
            playingStep = s.step
        }
        if s.tempo > 0, tempo != s.tempo {
            tempo = s.tempo
        }
        if bar != s.bar {
            bar = s.bar
        }
        if beatInBar != s.beatInBar {
            beatInBar = s.beatInBar
        }
        if locked != s.locked {
            locked = s.locked
        }
        if sourceStatus != s.status {
            sourceStatus = s.status
        }
        if s.devicesJSON != lastDevicesJSON {
            lastDevicesJSON = s.devicesJSON
            devices = NetworkDevice.list(fromJSON: s.devicesJSON)
        }
    }

    // MARK: Sharing

    /// A link the web app opens (`<webAppURL>#p=…`).
    public var shareLink: String {
        ShareCodec.link(for: pattern, base: webAppURL)
    }

    /// The pattern as a pretty-printed pattern file.
    public var patternFileJSON: String {
        pattern.jsonString(pretty: true)
    }

    /// Imports a web-app link, `#p=` hash or pattern JSON.
    @discardableResult
    public func importPattern(from text: String) -> Bool {
        guard let imported = ShareCodec.pattern(from: text) else {
            shareMessage = "Not a player5 link or pattern file."
            return false
        }
        replacePattern(imported)
        shareMessage = "Pattern imported."
        return true
    }

    /// For the view after it copied something.
    public func note(_ message: String) {
        shareMessage = message
    }

    // MARK: Library

    public func saveToLibrary(name: String) {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        let finalName = trimmed.isEmpty ? "Pattern \(library.count + 1)" : trimmed
        if let index = library.firstIndex(where: { $0.name == finalName }) {
            library[index] = SavedPattern(id: library[index].id, name: finalName, pattern: pattern)
        } else {
            library.append(SavedPattern(name: finalName, pattern: pattern))
        }
        store.saveLibrary(library)
    }

    public func load(_ saved: SavedPattern) {
        if let restored = saved.pattern {
            replacePattern(restored)
        }
    }

    public func delete(_ saved: SavedPattern) {
        library.removeAll { $0.id == saved.id }
        store.saveLibrary(library)
    }

    #if os(macOS)
        // MARK: macOS output device and MIDI

        public func refreshOutputDevices() {
            let list = OutputDevices.all()
            if list != outputDevices {
                outputDevices = list
            }
            applyOutputDeviceSelection()
        }

        /// Chooses the output device by UID; `nil` = system default.
        public func selectOutputDevice(uid: String?) {
            outputDeviceUID = uid
            store.outputDeviceUID = uid
            applyOutputDeviceSelection(force: true)
        }

        /// Resolves the chosen UID (it may be unplugged: then the system
        /// default plays) and hands the device to the engine when the device
        /// that would play changes, including a new system default while
        /// "System default" is selected (the engine pins the device it starts
        /// on, so it would not follow by itself).
        private func applyOutputDeviceSelection(force: Bool = false) {
            let target = OutputDevices.resolve(
                uid: outputDeviceUID, among: outputDevices, defaultID: OutputDevices.defaultOutputID())
            guard force || target.effective != appliedOutputDeviceID else { return }
            appliedOutputDeviceID = target.effective
            engine.setOutputDevice(target.chosen)
        }

        private func startMIDIIfNeeded() {
            guard midiInput == nil else { return }
            let engine = self.engine
            guard
                let input = MIDIClockInput(handler: { code, hostNanoseconds in
                    engine.midi(code, hostNanoseconds: hostNanoseconds)
                })
            else {
                clockMessage = "CoreMIDI is unavailable."
                return
            }
            input.onSourcesChanged = { [weak self] names in
                self?.midiSources = names
            }
            midiSources = input.sourceNames
            midiInput = input
        }
    #endif
}
