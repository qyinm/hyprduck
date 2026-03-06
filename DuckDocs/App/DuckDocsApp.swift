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
    @State private var captureService = AutoCaptureService()
    @State private var job = CaptureJob()
    @State private var showWindowPicker = false
    @State private var regionSelectorWindow: RegionSelectorWindow?
    @State private var documentImportService = DocumentImportService()

    init() {
        // API-based service, no preloading needed
    }

    var body: some Scene {
        WindowGroup {
            ContentView(
                captureService: $captureService,
                job: $job,
                documentImportService: documentImportService
            )
                .environment(appState)
                .onAppear {
                    setupKeyboardShortcuts()
                }
                .sheet(isPresented: $showWindowPicker) {
                    WindowPickerView { windowID, title, appName in
                        job.captureMode = .window(windowID: windowID, title: title, appName: appName)
                        showWindowPicker = false
                        // Start capture after window selection
                        Task { @MainActor in
                            try? await Task.sleep(for: .milliseconds(100))
                            startCapture()
                        }
                    }
                }
        }
        .windowStyle(.hiddenTitleBar)
        .windowResizability(.contentSize)
        .commands {
            CommandGroup(replacing: .newItem) { }
            CommandMenu("Capture") {
                Button("Quick Capture...") {
                    showQuickEntry()
                }
                .keyboardShortcut(" ", modifiers: .option)
                .disabled(!canStartCapture)

                Divider()

                Button("Start Full Screen Capture") {
                    job.captureMode = .fullScreen
                    startCapture()
                }
                .keyboardShortcut("1", modifiers: [.command])
                .disabled(!canStartCapture)

                Button("Cancel Capture") {
                    stopCapture()
                }
                .keyboardShortcut("x", modifiers: [.command, .shift])
                .disabled(!canStopCapture)

                Divider()

                Button("Import Document...") {
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

    private var canStartCapture: Bool {
        guard appState.canUseCapture, AIService.shared.configurationIssue == nil else { return false }

        if case .idle = captureService.state { return true }
        if case .completed = captureService.state { return true }
        if case .error = captureService.state { return true }
        if case .partiallyCompleted = captureService.state { return true }
        return false
    }

    private var canStartImport: Bool {
        guard appState.canUseImport, AIService.shared.configurationIssue == nil else {
            return false
        }

        switch documentImportService.state {
        case .converting, .processing, .saving:
            return false
        default:
            return true
        }
    }

    private var canStopCapture: Bool {
        switch captureService.state {
        case .preparing, .capturing, .processing, .saving:
            return true
        default:
            return false
        }
    }

    private func setupKeyboardShortcuts() {
        let manager = KeyboardShortcutManager.shared

        // Quick Entry trigger
        manager.onQuickEntry = { [self] in
            showQuickEntry()
        }

        // Cancel capture
        manager.onCancelCapture = { [self] in
            if canStopCapture {
                captureService.cancel()
            }
        }

        manager.start()

        // Listen for region/window selection from Quick Entry
        NotificationCenter.default.addObserver(
            forName: .quickEntrySelectRegion,
            object: nil,
            queue: .main
        ) { [self] _ in
            showRegionSelector()
        }

        NotificationCenter.default.addObserver(
            forName: .quickEntrySelectWindow,
            object: nil,
            queue: .main
        ) { [self] _ in
            showWindowPicker = true
        }
    }

    private func showQuickEntry() {
        // Don't show if already capturing
        guard canStartCapture else { return }

        // Dismiss existing if any
        QuickEntryWindow.shared?.dismiss()

        let window = QuickEntryWindow()
        window.onStartCapture = { [self] mode in
            job.captureMode = mode
            startCapture()
        }
        window.show()
    }

    private func showRegionSelector() {
        let window = RegionSelectorWindow()
        window.onRegionSelected = { [self] region in
            job.captureMode = .region(region)
            regionSelectorWindow = nil
            // Start capture after region selection
            Task { @MainActor in
                try? await Task.sleep(for: .milliseconds(100))
                startCapture()
            }
        }
        window.onCancelled = { [self] in
            regionSelectorWindow = nil
        }
        regionSelectorWindow = window
        window.show()
    }

    private func startCapture() {
        guard canStartCapture else { return }
        captureService.run(job: job, aiService: AIService.shared)
    }

    private func stopCapture() {
        guard canStopCapture else { return }
        captureService.cancel()
    }
}
