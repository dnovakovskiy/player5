import Foundation
import SwiftUI

/// The dumb master: one output gain, an optional soft safety limiter, and
/// (macOS) the output device. No EQ or compression; the mixer does that.
struct MasterView: View {
    @ObservedObject var model: AppModel

    var body: some View {
        Panel("Master") {
            VStack(alignment: .leading, spacing: 12) {
                LabeledSlider(
                    title: "Output",
                    value: model.binding(\.outputGain),
                    range: 0...2,
                    format: Formatting.decibels)

                Toggle(isOn: model.limiterBinding) {
                    Text("Safety limiter")
                        .font(Theme.mono(12))
                }

                #if os(macOS)
                    Picker("Output device", selection: outputBinding) {
                        Text("System default").tag("")
                        ForEach(model.outputDevices) { device in
                            Text("\(device.name) (\(device.outputChannels) ch)").tag(device.uid)
                        }
                    }
                    .pickerStyle(.menu)
                    Text("The mono mix plays on outputs 1–2 of the chosen device.")
                        .font(Theme.mono(11))
                        .foregroundStyle(Theme.muted)
                        .fixedSize(horizontal: false, vertical: true)
                #endif

                Text("0 dB ≈ −6 dBFS peaks. Gain staging and EQ belong to the mixer channel.")
                    .font(Theme.mono(11))
                    .foregroundStyle(Theme.muted)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    #if os(macOS)
        private var outputBinding: Binding<String> {
            Binding(
                get: { model.outputDeviceUID ?? "" },
                set: { uid in model.selectOutputDevice(uid: uid.isEmpty ? nil : uid) })
        }
    #endif
}
