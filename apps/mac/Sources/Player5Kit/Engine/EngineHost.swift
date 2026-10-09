import AVFoundation
import Foundation
import Player5Core

#if os(macOS)
    import AudioToolbox
    import CoreAudio
#endif

/// What the control queue reports to the UI: polled every 6th tick
/// (~30 ms) and delivered on the main thread only when something changed.
public struct EngineSnapshot: Equatable {
    /// Audible pattern step, or -1 when stopped.
    public var step: Int = -1
    /// Active clock tempo, rounded to 0.01 BPM.
    public var tempo: Double = 0
    /// 1-based bar count of the active clock's timeline.
    public var bar: Int = 1
    /// 1-based beat within the bar.
    public var beatInBar: Int = 1
    /// Whether the active clock tracks its source (always for internal).
    public var locked: Bool = true
    /// Last status message from the clock source.
    public var status: String = ""
    /// Devices the network source sees, as JSON.
    public var devicesJSON: String = "[]"

    public init() {}
}

/// State of the audio output.
public enum AudioStatus: Equatable {
    case off
    case running(sampleRate: Double, deviceLatencyMs: Double)
    case failed(String)
}

/// Why audio could not start.
public enum EngineError: Error, LocalizedError {
    case noOutput
    case coreRefused
    case format

    public var errorDescription: String? {
        switch self {
        case .noOutput: return "no audio output device"
        case .coreRefused: return "the engine core could not be created"
        case .format: return "unsupported output format"
        }
    }
}

/// Owns the Rust core's two halves and the audio graph (ADR-0008).
///
/// Threads:
/// - **main**: every public method, the `AVAudioEngine` graph, renderer
///   lifetime. Callbacks (`onSnapshot`, `onAudioStatus`, `onClockResult`)
///   arrive here.
/// - **control queue** (serial, user-interactive): every `p5_control_*`
///   call, including the 5 ms `DispatchSourceTimer` that ticks the
///   scheduler. `P5Control` is not thread-safe; nothing else touches it.
/// - **audio**: the source node's render block. It calls
///   `p5_renderer_render` and `memcpy`, nothing else: no allocation, locks,
///   logging, Objective-C messaging or Swift class references (it captures
///   only the raw renderer pointer).
public final class EngineHost {
    // MARK: Main-thread state

    private var engine = AVAudioEngine()
    private var sourceNode: AVAudioSourceNode?
    /// Owned. Freed only after its node is detached and the engine stopped.
    private var renderer: OpaquePointer?
    private var pairSampleRate: Double = 0
    private var configObserver: NSObjectProtocol?
    private var sessionObservers: [NSObjectProtocol] = []
    /// Keeps App Nap and timer coalescing away from the 5 ms control timer
    /// while audio runs.
    private var activity: NSObjectProtocol?
    #if os(macOS)
        private var outputDeviceID: AudioDeviceID?
    #endif

    /// Whether the user wants audio running (restarts follow changes).
    public private(set) var audioWanted = false
    /// Engine state readouts. Main thread.
    public var onSnapshot: ((EngineSnapshot) -> Void)?
    /// Audio output state changes. Main thread.
    public var onAudioStatus: ((AudioStatus) -> Void)?
    /// A clock source that had to fall back to the internal clock while
    /// re-applying settings (e.g. after an output change). Main thread.
    public var onClockResult: ((ClockKind, ClockStartResult) -> Void)?

    // MARK: Control-queue state

    private let controlQueue = DispatchQueue(label: "player5.control", qos: .userInteractive)
    private var timer: DispatchSourceTimer?
    private var control: OpaquePointer?
    private var desired = DesiredState()
    private var ticks = 0
    private var lastSnapshot = EngineSnapshot()
    private var nudgeTempo: Double = 0

    /// Everything the UI asked for, so a rebuilt control handle (new sample
    /// rate) comes back in the same state.
    private struct DesiredState {
        var patternJSON: String?
        var playing = false
        var clock: ClockKind = .internalClock
        var simulatedBPM: Double = 120
        var followDevice: Int32 = 0
        var nudgeMs: Double = 0
        var latencyMs: Double = 0
    }

    public init() {
        startTimer()
        observeEngine()
        observeSession()
    }

    deinit {
        timer?.cancel()
        if let o = configObserver {
            NotificationCenter.default.removeObserver(o)
        }
        for o in sessionObservers {
            NotificationCenter.default.removeObserver(o)
        }
        engine.stop()
        if let token = activity {
            ProcessInfo.processInfo.endActivity(token)
        }
        // No other reference to self exists, so no control-queue block can
        // be using these.
        if let c = control {
            p5_control_free(c)
        }
        if let r = renderer {
            p5_renderer_free(r)
        }
    }

    // MARK: Audio output (main thread)

    /// Starts audio output, or restarts it after a device or format change.
    public func startAudio() {
        startAudio(forceNewPair: false)
    }

    /// Stops audio output (the control loop keeps its state).
    public func stopAudio() {
        audioWanted = false
        engine.stop()
        if let token = activity {
            ProcessInfo.processInfo.endActivity(token)
            activity = nil
        }
        onAudioStatus?(.off)
    }

    /// Output latency the hardware reports (device + stream + safety
    /// offset), ms. Valid while running.
    public var deviceLatencyMs: Double {
        engine.outputNode.presentationLatency * 1_000
    }

    #if os(macOS)
        /// Chooses the output device; `nil` follows the system default at
        /// start. Restarts audio if it is running.
        public func setOutputDevice(_ id: AudioDeviceID?) {
            outputDeviceID = id
            if audioWanted {
                startAudio()
            }
        }
    #endif

    private func startAudio(forceNewPair: Bool) {
        audioWanted = true
        if activity == nil {
            activity = ProcessInfo.processInfo.beginActivity(
                options: [.userInitiated, .latencyCritical],
                reason: "player5 is sequencing audio")
        }
        do {
            try configureAndStart(forceNewPair: forceNewPair)
            onAudioStatus?(.running(sampleRate: pairSampleRate, deviceLatencyMs: deviceLatencyMs))
        } catch {
            onAudioStatus?(.failed(error.localizedDescription))
        }
    }

    private func configureAndStart(forceNewPair: Bool) throws {
        #if os(iOS)
            let session = AVAudioSession.sharedInstance()
            try session.setCategory(.playback, mode: .default, options: [])
            try? session.setPreferredIOBufferDuration(0.005)
            try session.setActive(true)
        #endif

        if engine.isRunning {
            engine.stop()
        }
        detachSourceNode()

        #if os(macOS)
            applyOutputDevice()
        #endif

        let output = engine.outputNode
        let hardware = output.outputFormat(forBus: 0)
        var sampleRate = hardware.sampleRate
        if sampleRate <= 0 {
            sampleRate = output.inputFormat(forBus: 0).sampleRate
        }
        guard sampleRate > 0, hardware.channelCount > 0 else {
            throw EngineError.noOutput
        }

        if forceNewPair || renderer == nil || sampleRate != pairSampleRate {
            try rebuildPair(sampleRate: sampleRate)
        }
        guard let renderer = renderer else {
            throw EngineError.coreRefused
        }

        // Mono core, rendered once and copied to every channel of a stereo
        // (or mono) bus. The mixer passes a matching format through at
        // unity; on interfaces with more outputs it feeds channels 1-2.
        let channels: AVAudioChannelCount = hardware.channelCount >= 2 ? 2 : 1
        guard let format = AVAudioFormat(standardFormatWithSampleRate: sampleRate, channels: channels)
        else {
            throw EngineError.format
        }
        let node = EngineHost.makeSourceNode(renderer: renderer, format: format)
        engine.attach(node)
        engine.connect(node, to: engine.mainMixerNode, format: format)
        engine.connect(engine.mainMixerNode, to: output, format: format)
        engine.mainMixerNode.outputVolume = 1
        sourceNode = node

        engine.prepare()
        try engine.start()
    }

    private func detachSourceNode() {
        guard let node = sourceNode else { return }
        engine.disconnectNodeOutput(node)
        engine.detach(node)
        sourceNode = nil
    }

    /// Creates a control/render pair at `sampleRate` and moves the UI's
    /// settings onto it. The engine is stopped and no node uses the old
    /// renderer when this runs.
    private func rebuildPair(sampleRate: Double) throws {
        var newControl: OpaquePointer?
        var newRenderer: OpaquePointer?
        guard p5_split_new(Float(sampleRate), &newControl, &newRenderer) == 0,
            let c = newControl, let r = newRenderer
        else {
            throw EngineError.coreRefused
        }
        let oldRenderer = renderer
        renderer = r
        pairSampleRate = sampleRate

        let (kind, result): (ClockKind, ClockStartResult) = controlQueue.sync {
            if let old = control {
                p5_control_free(old)
            }
            control = c
            lastSnapshot = EngineSnapshot()
            let requested = desired.clock
            return (requested, applyDesired(to: c))
        }
        if result != .ok {
            onClockResult?(kind, result)
        }

        if let old = oldRenderer {
            // engine.stop() has returned, so its render thread is done with
            // the old renderer; free it a moment later regardless, in case a
            // final cycle was still unwinding.
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) {
                p5_renderer_free(old)
            }
        }
    }

    /// The render block. Captures only the renderer pointer (a plain
    /// value); runs on the audio thread.
    private static func makeSourceNode(renderer: OpaquePointer, format: AVAudioFormat)
        -> AVAudioSourceNode
    {
        AVAudioSourceNode(format: format) { (_, timestamp, frameCount, bufferList) -> OSStatus in
            let buffers = UnsafeMutableAudioBufferListPointer(bufferList)
            let count = buffers.count
            guard count > 0, let first = buffers[0].mData else {
                return noErr
            }
            let capacity = Int(buffers[0].mDataByteSize) / MemoryLayout<Float>.stride
            let frames = min(Int(frameCount), capacity)
            let stamp = timestamp.pointee
            let hostTicks: UInt64 = stamp.mFlags.contains(.hostTimeValid) ? stamp.mHostTime : 0
            p5_renderer_render(renderer, first.assumingMemoryBound(to: Float.self), frames, hostTicks)
            let bytes = frames * MemoryLayout<Float>.stride
            var channel = 1
            while channel < count {
                if let data = buffers[channel].mData {
                    memcpy(data, first, min(bytes, Int(buffers[channel].mDataByteSize)))
                }
                channel += 1
            }
            return noErr
        }
    }

    #if os(macOS)
        /// Points the output unit at the chosen device (or the current system
        /// default). Engine stopped.
        private func applyOutputDevice() {
            guard let unit = engine.outputNode.audioUnit else { return }
            guard var target = outputDeviceID ?? OutputDevices.defaultOutputID() else { return }
            var current = AudioDeviceID(0)
            var size = UInt32(MemoryLayout<AudioDeviceID>.size)
            let read = AudioUnitGetProperty(
                unit, kAudioOutputUnitProperty_CurrentDevice, kAudioUnitScope_Global, 0,
                &current, &size)
            if read == noErr && current == target {
                return
            }
            _ = AudioUnitSetProperty(
                unit, kAudioOutputUnitProperty_CurrentDevice, kAudioUnitScope_Global, 0,
                &target, UInt32(MemoryLayout<AudioDeviceID>.size))
        }
    #endif

    // MARK: Notifications (main thread)

    private func observeEngine() {
        if let o = configObserver {
            NotificationCenter.default.removeObserver(o)
        }
        configObserver = NotificationCenter.default.addObserver(
            forName: .AVAudioEngineConfigurationChange, object: engine, queue: .main
        ) { [weak self] _ in
            self?.handleConfigurationChange()
        }
    }

    /// The hardware changed (device unplugged, sample rate or channel count
    /// changed, route change). The engine has stopped itself; rebuild the
    /// graph at the new format.
    private func handleConfigurationChange() {
        guard audioWanted, !engine.isRunning else { return }
        startAudio()
    }

    private func observeSession() {
        #if os(iOS)
            let center = NotificationCenter.default
            let session = AVAudioSession.sharedInstance()
            sessionObservers.append(
                center.addObserver(
                    forName: AVAudioSession.interruptionNotification, object: session, queue: .main
                ) { [weak self] note in
                    self?.handleInterruption(note)
                })
            sessionObservers.append(
                center.addObserver(
                    forName: AVAudioSession.mediaServicesWereResetNotification, object: session,
                    queue: .main
                ) { [weak self] _ in
                    self?.handleMediaServicesReset()
                })
        #endif
    }

    #if os(iOS)
        private func handleInterruption(_ note: Notification) {
            guard let raw = note.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt,
                let type = AVAudioSession.InterruptionType(rawValue: raw)
            else {
                return
            }
            switch type {
            case .began:
                onAudioStatus?(.failed("interrupted by another app"))
            case .ended:
                if audioWanted {
                    startAudio()
                }
            @unknown default:
                break
            }
        }

        /// Every audio object is invalid after a media-server reset: start
        /// over with a new engine and a new pair.
        private func handleMediaServicesReset() {
            sourceNode = nil
            engine = AVAudioEngine()
            observeEngine()
            if audioWanted {
                startAudio(forceNewPair: true)
            }
        }
    #endif

    // MARK: Control API (any thread; work runs on the control queue)

    /// Loads pattern JSON (the `core/engine/src/spec.rs` format).
    public func load(patternJSON json: String) {
        controlQueue.async { [weak self] in
            guard let self = self else { return }
            self.desired.patternJSON = json
            guard let c = self.control else { return }
            _ = json.withCString { p5_control_load_pattern_json(c, $0) }
        }
    }

    /// Starts or stops the sequencer.
    public func setPlaying(_ playing: Bool) {
        controlQueue.async { [weak self] in
            guard let self = self else { return }
            self.desired.playing = playing
            guard let c = self.control else { return }
            if playing {
                p5_control_start(c)
            } else {
                p5_control_stop(c)
            }
        }
    }

    /// Selects the clock. Network sources start inside the core; `completion`
    /// (main thread) reports whether that worked. Before audio has started
    /// the choice is stored and applied when it does.
    public func setClock(
        _ kind: ClockKind, simulatedBPM: Double,
        completion: @escaping (ClockStartResult) -> Void
    ) {
        controlQueue.async { [weak self] in
            guard let self = self else { return }
            self.desired.clock = kind
            self.desired.simulatedBPM = simulatedBPM
            var result = ClockStartResult.ok
            if let c = self.control {
                result = self.applyClock(c)
            }
            DispatchQueue.main.async {
                completion(result)
            }
        }
    }

    /// Follows a device: 0 = tempo master, else a device number.
    public func follow(device: Int) {
        let number = Int32(clamping: device)
        controlQueue.async { [weak self] in
            guard let self = self else { return }
            self.desired.followDevice = number
            guard let c = self.control else { return }
            p5_control_follow(c, number)
        }
    }

    /// Phase nudge in ms (positive = later).
    public func setNudge(ms: Double) {
        controlQueue.async { [weak self] in
            guard let self = self else { return }
            self.desired.nudgeMs = ms
            guard let c = self.control else { return }
            p5_control_set_nudge_ms(c, ms)
            self.nudgeTempo = p5_control_tempo(c)
        }
    }

    /// Total output latency compensation in ms (positive = trigger earlier).
    public func setLatency(ms: Double) {
        controlQueue.async { [weak self] in
            guard let self = self else { return }
            self.desired.latencyMs = ms
            guard let c = self.control else { return }
            p5_control_set_latency_ms(c, ms)
        }
    }

    /// Quantized re-sync.
    public func resync() {
        controlQueue.async { [weak self] in
            guard let self = self, let c = self.control else { return }
            p5_control_resync(c)
        }
    }

    /// A tap at `hostNanoseconds` (taken when the user tapped). On the
    /// internal clock `completion` gets the new tempo, else `nil`.
    public func tap(hostNanoseconds: UInt64, completion: @escaping (Double?) -> Void) {
        controlQueue.async { [weak self] in
            guard let self = self, let c = self.control else { return }
            p5_control_tap_host(c, hostNanoseconds)
            let isInternal = !self.desired.clock.isExternal
            let tempo: Double? = isInternal ? p5_control_tempo(c) : nil
            DispatchQueue.main.async {
                completion(tempo)
            }
        }
    }

    /// A MIDI clock message (`p5_control_midi_host` codes: 0 clock, 1 start,
    /// 2 continue, 3 stop) received at `hostNanoseconds`. Ignored unless the
    /// MIDI clock is selected. Safe to call from CoreMIDI's thread.
    public func midi(_ message: Int32, hostNanoseconds: UInt64) {
        controlQueue.async { [weak self] in
            guard let self = self, let c = self.control, self.desired.clock == .midi else { return }
            _ = p5_control_midi_host(c, message, hostNanoseconds)
        }
    }

    // MARK: Control queue internals

    private func startTimer() {
        let t = DispatchSource.makeTimerSource(flags: .strict, queue: controlQueue)
        t.schedule(deadline: .now() + .milliseconds(5), repeating: .milliseconds(5), leeway: .milliseconds(1))
        t.setEventHandler { [weak self] in
            self?.tick()
        }
        t.resume()
        timer = t
    }

    private func tick() {
        guard let c = control else { return }
        p5_control_tick(c)
        ticks &+= 1
        if ticks % 6 == 0 {
            publish(c)
        }
    }

    /// Applies everything the UI asked for to a fresh handle.
    private func applyDesired(to c: OpaquePointer) -> ClockStartResult {
        if let json = desired.patternJSON {
            _ = json.withCString { p5_control_load_pattern_json(c, $0) }
        }
        p5_control_set_nudge_ms(c, desired.nudgeMs)
        nudgeTempo = p5_control_tempo(c)
        p5_control_set_latency_ms(c, desired.latencyMs)
        let result = applyClock(c)
        if desired.playing {
            p5_control_start(c)
        }
        return result
    }

    /// Switches the core to `desired.clock`. A source the core cannot run
    /// falls back to the internal clock.
    private func applyClock(_ c: OpaquePointer) -> ClockStartResult {
        let kind = desired.clock
        guard let code = kind.sourceCode else {
            p5_control_stop_source(c)
            // 4 = follow, jittery (MIDI clock); 0 = internal.
            _ = p5_control_set_clock_mode(c, kind == .midi ? 4 : 0)
            return .ok
        }
        let rc = p5_control_start_source(c, code, 0, desired.simulatedBPM)
        if rc == 0 {
            if desired.followDevice != 0 {
                p5_control_follow(c, desired.followDevice)
            }
            return .ok
        }
        let message = rc == 3 ? EngineHost.string(p5_control_status(c)) : ""
        p5_control_stop_source(c)
        _ = p5_control_set_clock_mode(c, 0)
        desired.clock = .internalClock
        if rc == 2 {
            return .notInThisBuild
        }
        return .failed(message.isEmpty ? "could not start (code \(rc))" : message)
    }

    private func publish(_ c: OpaquePointer) {
        let tempo = p5_control_tempo(c)
        // The core stores the nudge in beats at the tempo of the moment; keep
        // the millisecond value true when the tempo moves.
        if desired.nudgeMs != 0, abs(tempo - nudgeTempo) > 0.05 {
            p5_control_set_nudge_ms(c, desired.nudgeMs)
            nudgeTempo = tempo
        }

        var snap = EngineSnapshot()
        snap.step = Int(p5_control_playing_step(c))
        snap.tempo = tempo.isFinite ? (tempo * 100).rounded() / 100 : 0
        let beat = p5_control_beat(c)
        if beat.isFinite, abs(beat) < 1e12 {
            let barStart = (beat / 4).rounded(.down)
            let inBar = beat - barStart * 4
            snap.bar = Int(barStart) + 1
            snap.beatInBar = min(max(Int(inBar) + 1, 1), 4)
        }
        snap.locked = p5_control_clock_locked(c) != 0
        snap.status = EngineHost.string(p5_control_status(c))
        snap.devicesJSON = EngineHost.string(p5_control_devices_json(c))
        if snap.devicesJSON.isEmpty {
            snap.devicesJSON = "[]"
        }

        guard snap != lastSnapshot else { return }
        lastSnapshot = snap
        DispatchQueue.main.async { [weak self] in
            self?.onSnapshot?(snap)
        }
    }

    /// Copies a C string the core owns (valid until the next tick).
    private static func string(_ pointer: UnsafePointer<CChar>?) -> String {
        guard let pointer = pointer else { return "" }
        return String(cString: pointer)
    }
}
