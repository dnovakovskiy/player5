import Foundation
import SwiftUI

/// Ten voices × sixteen steps. A tap cycles off → hit → accent; in flam
/// mode it toggles the flam. The row label selects the voice for the
/// controls panel; M mutes the track.
struct StepGridView: View {
    @ObservedObject var model: AppModel

    var body: some View {
        Panel("Pattern") {
            VStack(alignment: .leading, spacing: 12) {
                HStack(spacing: 10) {
                    Button {
                        model.flamMode.toggle()
                    } label: {
                        Text(model.flamMode ? "FLAM MODE: ON" : "FLAM MODE: OFF")
                    }
                    .buttonStyle(BoothButtonStyle(prominent: model.flamMode, minWidth: 150))
                    .accessibilityLabel("Flam mode")
                    .accessibilityValue(Text(model.flamMode ? "on" : "off"))

                    Text(model.flamMode ? "tap a step: flam on / off" : "tap a step: off → hit → accent")
                        .font(Theme.mono(11))
                        .foregroundStyle(Theme.muted)
                    Spacer(minLength: 0)
                }

                ScrollView(.horizontal, showsIndicators: false) {
                    VStack(alignment: .leading, spacing: Theme.cellSpacing) {
                        StepNumberRow()
                        ForEach(Voice.allCases) { voice in
                            TrackRow(
                                voice: voice,
                                track: model.pattern[voice],
                                playingStep: model.playingStep,
                                selected: model.selectedVoice == voice,
                                select: { model.selectedVoice = voice },
                                toggleMute: { model.toggleMute(voice) },
                                tapStep: { index in model.tapStep(voice, index) })
                        }
                    }
                    .padding(.vertical, 2)
                }
            }
        }
    }
}

/// Width of the label + mute columns, so the numbers line up with steps.
private let headerWidth: CGFloat = Theme.cell * 2 + Theme.cellSpacing

/// Extra gap before steps 5, 9 and 13.
private func groupGap(_ index: Int) -> CGFloat {
    index > 0 && index % 4 == 0 ? 6 : 0
}

private struct StepNumberRow: View {
    var body: some View {
        HStack(spacing: Theme.cellSpacing) {
            Color.clear.frame(width: headerWidth, height: 14)
            ForEach(0..<Step.count, id: \.self) { index in
                Text("\(index + 1)")
                    .font(Theme.mono(10))
                    .foregroundStyle(index % 4 == 0 ? Theme.text : Theme.muted)
                    .frame(width: Theme.cell, height: 14)
                    .padding(.leading, groupGap(index))
            }
        }
        .accessibilityHidden(true)
    }
}

private struct TrackRow: View {
    let voice: Voice
    let track: VoiceTrack
    let playingStep: Int
    let selected: Bool
    let select: () -> Void
    let toggleMute: () -> Void
    let tapStep: (Int) -> Void

    var body: some View {
        HStack(spacing: Theme.cellSpacing) {
            Button(action: select) {
                Text(voice.label)
                    .font(Theme.mono(14, .bold))
                    .foregroundStyle(selected ? Color.black : Theme.text)
                    .frame(width: Theme.cell, height: Theme.cell)
                    .background(
                        RoundedRectangle(cornerRadius: Theme.corner, style: .continuous)
                            .fill(selected ? Theme.accent : Theme.control)
                    )
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("\(voice.title), edit controls")

            Button(action: toggleMute) {
                Text("M")
                    .font(Theme.mono(13, .bold))
                    .foregroundStyle(track.mute ? Color.black : Theme.muted)
                    .frame(width: Theme.cell, height: Theme.cell)
                    .background(
                        RoundedRectangle(cornerRadius: Theme.corner, style: .continuous)
                            .fill(track.mute ? Theme.warning : Theme.control)
                    )
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Mute \(voice.title)")
            .accessibilityValue(Text(track.mute ? "muted" : "playing"))

            ForEach(0..<Step.count, id: \.self) { index in
                StepCell(
                    step: track.steps[index],
                    isBeat: index % 4 == 0,
                    isPlayhead: index == playingStep,
                    muted: track.mute,
                    label: "\(voice.title) step \(index + 1)",
                    action: { tapStep(index) })
                    .padding(.leading, groupGap(index))
            }
        }
    }
}

private struct StepCell: View {
    let step: Step
    let isBeat: Bool
    let isPlayhead: Bool
    let muted: Bool
    let label: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            ZStack {
                RoundedRectangle(cornerRadius: Theme.corner, style: .continuous)
                    .fill(fill)
                RoundedRectangle(cornerRadius: Theme.corner, style: .continuous)
                    .strokeBorder(border, lineWidth: 2)
                if step.isFlam {
                    // Two strokes: the grace note and the main hit.
                    HStack(spacing: 4) {
                        Capsule().frame(width: 4, height: 12).opacity(0.55)
                        Capsule().frame(width: 4, height: 18)
                    }
                    .foregroundStyle(Color.black.opacity(0.75))
                }
                if isPlayhead {
                    RoundedRectangle(cornerRadius: Theme.corner, style: .continuous)
                        .strokeBorder(Theme.playhead, lineWidth: 3)
                }
            }
            .frame(width: Theme.cell, height: Theme.cell)
            .opacity(muted ? 0.45 : 1)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(label)
        .accessibilityValue(stateDescription)
    }

    private var fill: Color {
        switch step {
        case .off: return Theme.stepOff
        case .hit, .flam: return Theme.hit
        case .accent, .accentFlam: return Theme.accent
        }
    }

    private var border: Color {
        switch step {
        case .accent, .accentFlam: return Color.white
        case .hit, .flam: return Theme.hit
        case .off: return isBeat ? Theme.beatLine : Theme.line
        }
    }

    private var stateDescription: String {
        switch step {
        case .off: return "off"
        case .hit: return "hit"
        case .accent: return "accent"
        case .flam: return "flam"
        case .accentFlam: return "accented flam"
        }
    }
}
