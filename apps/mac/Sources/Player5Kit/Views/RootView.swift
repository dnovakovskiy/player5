import Foundation
import SwiftUI

/// The whole app UI, shared by macOS and iOS.
public struct Player5RootView: View {
    @ObservedObject private var model: AppModel

    public init(model: AppModel) {
        self.model = model
    }

    public var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: 16) {
                HeaderView(model: model)
                TransportView(model: model)
                StepGridView(model: model)
                LazyVGrid(
                    // 340 pt fits every iOS 16 iPhone in portrait (375 pt minus padding).
                    columns: [GridItem(.adaptive(minimum: 340), spacing: 16, alignment: .top)],
                    alignment: .leading,
                    spacing: 16
                ) {
                    VoiceControlsView(model: model)
                    ClockPanelView(model: model)
                    MasterView(model: model)
                    PatternLibraryView(model: model)
                }
            }
            .padding(16)
            .frame(maxWidth: 1_120)
            .frame(maxWidth: .infinity)
        }
        .background(Theme.background)
        .foregroundStyle(Theme.text)
        .tint(Theme.accent)
        .preferredColorScheme(.dark)
    }
}

struct HeaderView: View {
    @ObservedObject var model: AppModel

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            HStack(spacing: 0) {
                Text("player")
                Text("5").foregroundStyle(Theme.accent)
            }
            .font(Theme.mono(24, .bold))
            .accessibilityElement(children: .combine)
            Spacer()
            Text(model.audioStatusText)
                .font(Theme.mono(12))
                .foregroundStyle(statusColor)
                .multilineTextAlignment(.trailing)
        }
    }

    private var statusColor: Color {
        switch model.audioStatus {
        case .off: return Theme.muted
        case .running: return Theme.good
        case .failed: return Theme.warning
        }
    }
}

extension View {
    /// Text entry for links, names and numbers: no auto-capitalization or
    /// autocorrection on iOS.
    func plainTextEntry() -> some View {
        #if os(iOS)
            return self
                .textInputAutocapitalization(.never)
                .disableAutocorrection(true)
        #else
            return self
        #endif
    }
}
