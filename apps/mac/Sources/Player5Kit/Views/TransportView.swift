import Foundation
import SwiftUI

/// Play/stop, tempo, tap, and the pattern-wide feel controls.
struct TransportView: View {
    @ObservedObject var model: AppModel
    @State private var bpmText = ""
    @FocusState private var bpmFocused: Bool

    var body: some View {
        Panel("Transport") {
            VStack(alignment: .leading, spacing: 14) {
                // Two short rows fit a phone; the BPM field exists once so
                // its focus state stays unambiguous.
                HStack(spacing: 10) {
                    playButton
                    tapButton
                    Spacer(minLength: 0)
                }
                HStack(spacing: 10) {
                    tempoControls
                    Spacer(minLength: 0)
                }

                ClockReadout(model: model)

                LazyVGrid(
                    columns: [GridItem(.adaptive(minimum: 160), spacing: 16, alignment: .top)],
                    alignment: .leading,
                    spacing: 12
                ) {
                    LabeledSlider(title: "Shuffle", value: model.binding(\.shuffle))
                    LabeledSlider(title: "Accent", value: model.binding(\.accent))
                    LabeledSlider(title: "Flam", value: model.binding(\.flam))
                }
            }
        }
        .onAppear {
            bpmText = Formatting.bpm(model.pattern.bpm)
        }
        .onChange(of: model.pattern.bpm) { bpm in
            if !bpmFocused {
                bpmText = Formatting.bpm(bpm)
            }
        }
        .onChange(of: bpmFocused) { focused in
            if !focused {
                commitBPM()
            }
        }
    }

    private var playButton: some View {
        Button {
            model.togglePlay()
        } label: {
            Text(model.isPlaying ? "STOP" : "PLAY")
                .tracking(2)
        }
        .buttonStyle(BoothButtonStyle(prominent: !model.isPlaying, minWidth: 120))
        .accessibilityLabel(Text(model.isPlaying ? "Stop" : "Play"))
    }

    private var tapButton: some View {
        Button {
            model.tap()
        } label: {
            Text("TAP")
        }
        .buttonStyle(BoothButtonStyle(minWidth: 72))
        .accessibilityLabel("Tap tempo")
    }

    private var tempoControls: some View {
        HStack(spacing: 10) {
            Button {
                model.nudgeBPM(by: -1)
            } label: {
                Text("−")
            }
            .buttonStyle(BoothButtonStyle())
            .accessibilityLabel("Tempo down")

            TextField("BPM", text: $bpmText)
                .textFieldStyle(.plain)
                .font(Theme.mono(20, .semibold))
                .multilineTextAlignment(.center)
                .frame(width: 96, height: 44)
                .background(
                    RoundedRectangle(cornerRadius: Theme.corner, style: .continuous)
                        .fill(Color.black.opacity(0.35))
                )
                .overlay(
                    RoundedRectangle(cornerRadius: Theme.corner, style: .continuous)
                        .stroke(Theme.line, lineWidth: 1)
                )
                .focused($bpmFocused)
                .onSubmit { commitBPM() }
                .plainTextEntry()
                .accessibilityLabel("Tempo in BPM")

            Button {
                model.nudgeBPM(by: 1)
            } label: {
                Text("+")
            }
            .buttonStyle(BoothButtonStyle())
            .accessibilityLabel("Tempo up")
        }
    }

    private func commitBPM() {
        let cleaned = bpmText.trimmingCharacters(in: .whitespaces).replacingOccurrences(of: ",", with: ".")
        if let bpm = Double(cleaned), bpm.isFinite {
            model.setBPM(bpm)
        }
        bpmText = Formatting.bpm(model.pattern.bpm)
    }
}

/// Tempo of the active clock, lock state and bar/beat position.
struct ClockReadout: View {
    @ObservedObject var model: AppModel

    var body: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: 14) {
                tempo
                lock
                position
                source
                Spacer(minLength: 0)
            }
            VStack(alignment: .leading, spacing: 6) {
                HStack(spacing: 14) {
                    tempo
                    lock
                }
                HStack(spacing: 14) {
                    position
                    source
                }
            }
        }
        .font(Theme.mono(13, .medium))
        .accessibilityElement(children: .combine)
    }

    private var tempo: some View {
        Text(String(format: "%.2f BPM", model.tempo))
            .monospacedDigit()
    }

    private var lock: some View {
        HStack(spacing: 6) {
            Circle()
                .fill(model.locked ? Theme.good : Theme.warning)
                .frame(width: 10, height: 10)
            Text(model.locked ? "LOCKED" : "SEARCHING")
        }
    }

    private var position: some View {
        Text("BAR \(model.bar) · \(model.beatInBar)")
            .monospacedDigit()
    }

    private var source: some View {
        Text(model.clock.kind.title.uppercased())
            .foregroundStyle(Theme.muted)
    }
}
