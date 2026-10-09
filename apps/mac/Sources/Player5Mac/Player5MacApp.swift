import AppKit
import SwiftUI

// From the package, Player5Kit is a separate module. The XcodeGen app
// (apps/mac/project.yml) compiles Player5Kit's sources into the app module
// itself, so there is nothing to import there.
#if canImport(Player5Kit)
    import Player5Kit
#endif

/// macOS entry point. Built two ways: `swift run Player5Mac` (from the
/// package, no bundle) and the XcodeGen project in apps/mac/project.yml
/// (a sandboxed .app).
@main
struct Player5MacApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @StateObject private var model = AppModel()

    var body: some Scene {
        WindowGroup("player5") {
            Player5RootView(model: model)
                .frame(minWidth: 760, minHeight: 560)
        }
        .defaultSize(width: 1_120, height: 860)
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        // Launched as a bare executable (`swift run`) the process is not a
        // foreground app until it says so.
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }
}
