import Foundation
import SwiftUI

#if os(macOS)
    import AppKit
#else
    import UIKit
#endif

/// Presets, the user's saved patterns, and sharing with the web app
/// (`#p=` links) or as pattern JSON.
struct PatternLibraryView: View {
    @ObservedObject var model: AppModel
    @State private var saveName = ""
    @State private var importText = ""

    var body: some View {
        Panel("Patterns") {
            VStack(alignment: .leading, spacing: 12) {
                HStack(spacing: 10) {
                    Menu {
                        ForEach(Presets.all) { preset in
                            Button(preset.name) {
                                model.load(preset)
                            }
                        }
                    } label: {
                        Text("PRESETS")
                            .font(Theme.mono(14, .semibold))
                    }
                    .frame(minWidth: 110, minHeight: 44)
                    Button {
                        model.clearAll()
                    } label: {
                        Text("CLEAR ALL")
                    }
                    .buttonStyle(BoothButtonStyle(minWidth: 110))
                    Spacer(minLength: 0)
                }

                HStack(spacing: 10) {
                    TextField("Name", text: $saveName)
                        .textFieldStyle(.roundedBorder)
                        .plainTextEntry()
                        .onSubmit { save() }
                    Button {
                        save()
                    } label: {
                        Text("SAVE")
                    }
                    .buttonStyle(BoothButtonStyle(minWidth: 80))
                }

                ForEach(model.library) { saved in
                    HStack(spacing: 10) {
                        Button {
                            model.load(saved)
                        } label: {
                            Text(saved.name)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                        .buttonStyle(BoothButtonStyle())
                        Button {
                            model.delete(saved)
                        } label: {
                            Image(systemName: "trash")
                        }
                        .buttonStyle(BoothButtonStyle())
                        .accessibilityLabel("Delete \(saved.name)")
                    }
                }

                Divider().overlay(Theme.line)

                Text("SHARE")
                    .font(Theme.mono(11, .semibold))
                    .foregroundStyle(Theme.muted)
                TextField("Web app address, e.g. https://…/player5/", text: $model.webAppURL)
                    .textFieldStyle(.roundedBorder)
                    .plainTextEntry()
                HStack(spacing: 10) {
                    Button {
                        copy(model.shareLink, note: "Link copied.")
                    } label: {
                        Text("COPY LINK")
                    }
                    .buttonStyle(BoothButtonStyle(minWidth: 110))
                    Button {
                        copy(model.patternFileJSON, note: "Pattern JSON copied.")
                    } label: {
                        Text("COPY JSON")
                    }
                    .buttonStyle(BoothButtonStyle(minWidth: 110))
                    ShareLink(item: model.shareLink)
                        .frame(minHeight: 44)
                    Spacer(minLength: 0)
                }
                HStack(spacing: 10) {
                    TextField("Paste a link or pattern JSON", text: $importText)
                        .textFieldStyle(.roundedBorder)
                        .plainTextEntry()
                        .onSubmit { importPasted() }
                    Button {
                        importPasted()
                    } label: {
                        Text("IMPORT")
                    }
                    .buttonStyle(BoothButtonStyle(minWidth: 90))
                }
                if let message = model.shareMessage {
                    Text(message)
                        .font(Theme.mono(11))
                        .foregroundStyle(Theme.muted)
                }
            }
        }
    }

    private func save() {
        model.saveToLibrary(name: saveName)
        saveName = ""
    }

    private func importPasted() {
        if model.importPattern(from: importText) {
            importText = ""
        }
    }

    private func copy(_ text: String, note: String) {
        #if os(macOS)
            let board = NSPasteboard.general
            board.clearContents()
            board.setString(text, forType: .string)
        #else
            UIPasteboard.general.string = text
        #endif
        model.note(note)
    }
}
