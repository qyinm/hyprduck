//
//  DuckDocsApp.swift
//  DuckDocs
//
//  Created by hippoo on 1/30/26.
//

import SwiftUI
import UniformTypeIdentifiers

@main
struct DuckDocsApp: App {
    @State private var appState = AppState.shared
    @State private var documentImportService = DocumentImportService()

    var body: some Scene {
        WindowGroup {
            ContentView(documentImportService: documentImportService)
                .environment(appState)
        }
        .windowStyle(.hiddenTitleBar)
        .windowResizability(.contentSize)
        .commands {
            CommandGroup(replacing: .newItem) {
                Button("Import File...") {
                    importDocument()
                }
                .keyboardShortcut("i", modifiers: .command)
                .disabled(!canStartImport)
            }
        }
    }

    private func importDocument() {
        guard canStartImport else { return }

        let panel = NSOpenPanel()
        panel.allowedContentTypes = [
            .pdf,
            UTType(filenameExtension: "docx") ?? .data,
            UTType(filenameExtension: "doc") ?? .data
        ]
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        panel.message = "Select a PDF or Word document to convert to Markdown"
        panel.prompt = "Import"

        if panel.runModal() == .OK, let url = panel.url {
            do {
                if case .idle = documentImportService.state {
                    // Keep current state.
                } else {
                    documentImportService.reset()
                }
                let job = try DocumentImportJob(fileURL: url)
                documentImportService.run(job: job, aiService: AIService.shared)
            } catch {
                print("Error creating import job: \(error)")
            }
        }
    }

    private var canStartImport: Bool {
        guard AIService.shared.configurationIssue == nil else {
            return false
        }

        switch documentImportService.state {
        case .converting, .processing, .saving:
            return false
        default:
            return true
        }
    }
}
