import Player5Kit
import SwiftUI
import UIKit

/// iOS entry point. Everything else lives in the shared Swift package
/// (apps/mac, product `Player5Kit`); see ADR-0008.
@main
struct Player5App: App {
    @StateObject private var model = AppModel()

    var body: some Scene {
        WindowGroup {
            Player5RootView(model: model)
                .onAppear {
                    // A booth instrument: keep the screen awake while open.
                    UIApplication.shared.isIdleTimerDisabled = true
                }
        }
    }
}
