import Foundation
import SwiftUI

/// Clock source, follow target, lock state and the global timing controls
/// (nudge, latency offset, re-sync, tap).
struct ClockPanelView: View {
    @ObservedObject var model: AppModel

    var body: some View {
        Panel("Clock") {
            VStack(alignment: .leading, spacing: 12) {
                HStack(spacing: 10) {
                    Picker("Source", selection: sourceBinding) {
                        ForEach(ClockKind.available) { kind in
                            Text(kind.title).tag(kind)
                        }
                    }
                    .pickerStyle(.menu)
                    .frame(maxWidth: 260, alignment: .leading)
                    Spacer(minLength: 0)
                }

                if let message = model.clockMessage {
                    Text(message)
                        .font(Theme.mono(12))
                        .foregroundStyle(Theme.hit)
                        .fixedSize(horizontal: false, vertical: true)
                }

                ClockReadout(model: model)

                if !model.sourceStatus.isEmpty {
                    Text(model.sourceStatus)
                        .font(Theme.mono(11))
                        .foregroundStyle(Theme.muted)
                        .fixedSize(horizontal: false, vertical: true)
                }

                if model.clock.kind.listsDevices || !model.devices.isEmpty {
                    followSection
                }

                platformNotes

                nudgeRow

                Stepper(value: latencyBinding, in: -100...500, step: 1) {
                    Text("Latency offset \(Int(model.clock.latencyMs)) ms")
                        .font(Theme.mono(12))
                }

                Toggle(isOn: compensateBinding) {
                    Text(String(format: "Add output latency (%.1f ms)", model.deviceLatencyMs))
                        .font(Theme.mono(12))
                }

                HStack(spacing: 10) {
                    Button {
                        model.resync()
                    } label: {
                        Text("RE-SYNC")
                    }
                    .buttonStyle(BoothButtonStyle(minWidth: 100))
                    .accessibilityLabel("Quantized re-sync")

                    Button {
                        model.tap()
                    } label: {
                        Text("TAP")
                    }
                    .buttonStyle(BoothButtonStyle(minWidth: 100))
                    .accessibilityLabel("Tap tempo")
                    Spacer(minLength: 0)
                }
            }
        }
    }

    private var sourceBinding: Binding<ClockKind> {
        Binding(
            get: { model.clock.kind },
            set: { model.selectClock($0) })
    }

    private var followBinding: Binding<Int> {
        Binding(
            get: { model.clock.followDevice },
            set: { model.follow(device: $0) })
    }

    private var latencyBinding: Binding<Double> {
        Binding(
            get: { model.clock.latencyMs },
            set: { model.setLatency($0) })
    }

    private var compensateBinding: Binding<Bool> {
        Binding(
            get: { model.clock.compensateDeviceLatency },
            set: { model.setCompensateDeviceLatency($0) })
    }

    private var followSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            Picker("Follow", selection: followBinding) {
                Text("Tempo master").tag(0)
                ForEach(model.devices) { device in
                    Text("\(device.number) · \(device.name)").tag(device.number)
                }
            }
            .pickerStyle(.menu)
            .frame(maxWidth: 260, alignment: .leading)

            if model.devices.isEmpty {
                Text("No devices seen yet.")
                    .font(Theme.mono(11))
                    .foregroundStyle(Theme.muted)
            }
            ForEach(model.devices) { device in
                DeviceRow(device: device, followed: model.clock.followDevice == device.number)
            }
        }
    }

    @ViewBuilder
    private var platformNotes: some View {
        #if os(macOS)
            if model.clock.kind == .midi {
                Text(
                    model.midiSources.isEmpty
                        ? "No MIDI sources connected."
                        : "Listening on: " + model.midiSources.joined(separator: ", ")
                )
                .font(Theme.mono(11))
                .foregroundStyle(Theme.muted)
                .fixedSize(horizontal: false, vertical: true)
            }
        #else
            if model.clock.kind == .proDJLink || model.clock.kind == .opusQuad || model.clock.kind == .link {
                Text("On iOS, network clocks need Apple's multicast entitlement (requested; see docs/ios-multicast-entitlement.md).")
                    .font(Theme.mono(11))
                    .foregroundStyle(Theme.muted)
                    .fixedSize(horizontal: false, vertical: true)
            }
        #endif
    }

    private var nudgeRow: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Text("Phase nudge")
                    .font(Theme.mono(12))
                    .foregroundStyle(Theme.muted)
                Spacer(minLength: 0)
                Button {
                    model.setNudge(0)
                } label: {
                    Text("RESET")
                }
                .buttonStyle(BoothButtonStyle())
                .accessibilityLabel("Reset nudge")
            }
            HStack(spacing: 8) {
                nudgeButton("−5", -5)
                nudgeButton("−1", -1)
                Text(String(format: "%+.1f ms", model.clock.nudgeMs))
                    .font(Theme.mono(13, .semibold))
                    .monospacedDigit()
                    .frame(minWidth: 76)
                nudgeButton("+1", 1)
                nudgeButton("+5", 5)
                Spacer(minLength: 0)
            }
        }
    }

    private func nudgeButton(_ title: String, _ ms: Double) -> some View {
        let spoken: String =
            ms < 0
            ? "Nudge earlier by \(Int(-ms)) milliseconds"
            : "Nudge later by \(Int(ms)) milliseconds"
        return Button {
            model.nudge(by: ms)
        } label: {
            Text(title)
        }
        .buttonStyle(BoothButtonStyle())
        .accessibilityLabel(spoken)
    }
}

private struct DeviceRow: View {
    let device: NetworkDevice
    let followed: Bool

    var body: some View {
        HStack(spacing: 8) {
            Text("\(device.number)")
                .font(Theme.mono(13, .bold))
                .frame(width: 26)
            VStack(alignment: .leading, spacing: 2) {
                Text(device.name)
                    .font(Theme.mono(12, .semibold))
                Text("\(device.kind) · \(device.address)")
                    .font(Theme.mono(10))
                    .foregroundStyle(Theme.muted)
            }
            Spacer(minLength: 0)
            if let bpm = device.bpm {
                Text(String(format: "%.2f", bpm))
                    .font(Theme.mono(12))
                    .monospacedDigit()
            }
            if device.master == true {
                Tag(text: "MASTER", color: Theme.accent)
            }
            if device.onAir == true {
                Tag(text: "ON AIR", color: Theme.warning)
            }
            if device.playing == true {
                Tag(text: "PLAY", color: Theme.good)
            }
        }
        .padding(8)
        .background(
            RoundedRectangle(cornerRadius: Theme.corner, style: .continuous)
                .fill(followed ? Theme.accent.opacity(0.15) : Theme.control)
        )
    }
}

private struct Tag: View {
    let text: String
    let color: Color

    var body: some View {
        Text(text)
            .font(Theme.mono(9, .bold))
            .foregroundStyle(Color.black)
            .padding(.horizontal, 5)
            .padding(.vertical, 2)
            .background(Capsule().fill(color))
    }
}
