import Foundation
import SwiftUI

/// The selected voice's TR-style controls.
struct VoiceControlsView: View {
    @ObservedObject var model: AppModel

    var body: some View {
        let voice = model.selectedVoice
        let track = model.pattern[voice]
        Panel("Voice") {
            VStack(alignment: .leading, spacing: 12) {
                HStack(spacing: 10) {
                    Picker("Voice", selection: $model.selectedVoice) {
                        ForEach(Voice.allCases) { v in
                            Text("\(v.label) · \(v.title)").tag(v)
                        }
                    }
                    .pickerStyle(.menu)
                    .labelsHidden()
                    .frame(maxWidth: 220, alignment: .leading)
                    Spacer(minLength: 0)
                    Button {
                        model.toggleMute(voice)
                    } label: {
                        Text(track.mute ? "MUTED" : "MUTE")
                    }
                    .buttonStyle(BoothButtonStyle(prominent: track.mute, minWidth: 80))
                    Button {
                        model.clear(voice)
                    } label: {
                        Text("CLEAR")
                    }
                    .buttonStyle(BoothButtonStyle(minWidth: 80))
                }

                ForEach(voice.controls) { control in
                    LabeledSlider(title: control.title, value: model.binding(voice, control))
                }

                Text("Steps: \(track.notation)")
                    .font(Theme.mono(11))
                    .foregroundStyle(Theme.muted)
                    .textSelection(.enabled)
            }
        }
    }
}
