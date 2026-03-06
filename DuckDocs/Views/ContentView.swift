//
//  ContentView.swift
//  DuckDocs
//
//  Created by hippoo on 1/30/26.
//

import SwiftUI

struct ContentView: View {
    @Environment(AppState.self) var appState
    @AppStorage("hasDismissedCapturePermissionOnboarding") private var hasDismissedCapturePermissionOnboarding = false
    @State private var showOnboarding = false
    @Binding var captureService: AutoCaptureService
    @Binding var job: CaptureJob
    @State private var showWindowPicker = false
    let documentImportService: DocumentImportService
    private var aiService: AIService { AIService.shared }

    init(
        captureService: Binding<AutoCaptureService>,
        job: Binding<CaptureJob>,
        documentImportService: DocumentImportService
    ) {
        self._captureService = captureService
        self._job = job
        self.documentImportService = documentImportService
    }

    var body: some View {
        ScrollView {
            VStack(spacing: 24) {
                // Header
                Text("DuckDocs")
                    .font(.largeTitle)
                    .fontWeight(.bold)

                Text("Capture screens or import documents, then turn them into markdown with AI.")
                    .foregroundStyle(.secondary)

                CaptureWorkflowSection(
                    captureService: $captureService,
                    job: $job,
                    showWindowPicker: $showWindowPicker,
                    aiService: aiService
                )

                DocumentImportSection(importService: documentImportService, aiService: aiService)
            }
            .padding(32)
        }
        .frame(minWidth: 500, minHeight: 600)
        .task {
            await appState.permissionManager.checkAllPermissions()
            if !appState.canUseCapture && !hasDismissedCapturePermissionOnboarding {
                showOnboarding = true
            }
        }
        .sheet(isPresented: $showOnboarding) {
            OnboardingView()
        }
        .sheet(isPresented: $showWindowPicker) {
            WindowPickerView { windowID, title, appName in
                job.captureMode = .window(windowID: windowID, title: title, appName: appName)
            }
        }
    }
}

// MARK: - Settings Section

struct CaptureWorkflowSection: View {
    @Environment(AppState.self) var appState
    @Binding var captureService: AutoCaptureService
    @Binding var job: CaptureJob
    @Binding var showWindowPicker: Bool
    let aiService: AIService

    private var canStartCapture: Bool {
        appState.canUseCapture && aiService.configurationIssue == nil
    }

    private var canRetryCapture: Bool {
        aiService.configurationIssue == nil
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Capture Workflow")
                .font(.headline)

            Text("Automate full-screen, region, or window captures, then compile each step into a markdown guide.")
                .foregroundStyle(.secondary)

            if !appState.canUseCapture {
                CapturePermissionsBanner(permissionManager: appState.permissionManager)
            }

            SettingsSection(job: $job, showWindowPicker: $showWindowPicker, aiService: aiService)

            if let configurationIssue = aiService.configurationIssue {
                WorkflowNotice(
                    title: "AI Setup Required",
                    message: configurationIssue,
                    systemImage: "bolt.horizontal.circle"
                )
            }

            StatusSection(captureService: captureService)

            ActionButton(
                captureService: captureService,
                job: job,
                aiService: aiService,
                canStart: canStartCapture,
                canRetry: canRetryCapture
            )
        }
        .padding()
        .background(Color(nsColor: .controlBackgroundColor).opacity(0.35))
        .cornerRadius(16)
    }
}

struct CapturePermissionsBanner: View {
    @Bindable var permissionManager: PermissionManager

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .top, spacing: 12) {
                Image(systemName: "lock.shield")
                    .foregroundStyle(.orange)

                VStack(alignment: .leading, spacing: 4) {
                    Text("Capture permissions needed")
                        .font(.subheadline)
                        .fontWeight(.semibold)
                    Text(permissionManager.capturePermissionCallout)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }

            HStack(spacing: 8) {
                if !permissionManager.accessibilityGranted {
                    Button("Grant Accessibility") {
                        permissionManager.requestAccessibilityPermission()
                    }
                    .buttonStyle(.bordered)
                }

                if !permissionManager.screenCaptureGranted {
                    Button("Grant Screen Recording") {
                        permissionManager.requestScreenCapturePermission()
                    }
                    .buttonStyle(.bordered)
                }
            }
        }
        .padding()
        .background(Color.orange.opacity(0.08))
        .cornerRadius(12)
    }
}

struct WorkflowNotice: View {
    let title: String
    let message: String
    let systemImage: String

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: systemImage)
                .foregroundStyle(.orange)

            VStack(alignment: .leading, spacing: 4) {
                Text(title)
                    .font(.subheadline)
                    .fontWeight(.semibold)
                Text(message)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .padding()
        .background(Color.orange.opacity(0.08))
        .cornerRadius(12)
    }
}

struct SettingsSection: View {
    @Binding var job: CaptureJob
    @Binding var showWindowPicker: Bool
    let aiService: AIService
    @State private var regionSelectorWindow: RegionSelectorWindow?
    @State private var selectedProvider: AIProviderType = AIService.shared.providerType
    @State private var selectedModelIndex: Int = 0
    @State private var customModel: String = ""
    @State private var apiKey: String = AIService.shared.apiKey
    @State private var useCustomModel: Bool = false
    @State private var baseURL: String = ""
    @State private var selectedTemplate: PromptTemplate = AIService.shared.selectedTemplate

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(aiService.sharedConfigurationSummary)
                .font(.caption)
                .foregroundStyle(.secondary)

            // AI Provider
            HStack {
                Text("AI Provider:")
                    .frame(width: 120, alignment: .trailing)

                Picker("", selection: $selectedProvider) {
                    ForEach(AIProviderType.allCases) { provider in
                        Text(provider.rawValue).tag(provider)
                    }
                }
                .pickerStyle(.menu)
                .frame(maxWidth: .infinity)
                .onChange(of: selectedProvider) { _, newValue in
                    aiService.switchProvider(newValue)
                    apiKey = aiService.apiKey
                    selectedModelIndex = 0
                    customModel = ""
                    useCustomModel = false
                    baseURL = aiService.config.baseURL ?? ""
                }
            }

            // Model Selection
            HStack {
                Text("Model:")
                    .frame(width: 120, alignment: .trailing)

                VStack(alignment: .leading, spacing: 8) {
                    if useCustomModel {
                        HStack {
                            TextField("model-name", text: $customModel)
                                .textFieldStyle(.roundedBorder)
                                .onChange(of: customModel) { _, newValue in
                                    if !newValue.isEmpty {
                                        aiService.setModelId(newValue)
                                    }
                                }
                            Button("Presets") {
                                useCustomModel = false
                                if let first = selectedProvider.presetModels.first {
                                    aiService.setModelId(first)
                                }
                            }
                            .buttonStyle(.borderless)
                        }
                    } else {
                        HStack {
                            Picker("", selection: $selectedModelIndex) {
                                ForEach(Array(selectedProvider.presetModels.enumerated()), id: \.offset) { index, model in
                                    Text(model).tag(index)
                                }
                            }
                            .pickerStyle(.menu)
                            .frame(maxWidth: .infinity)
                            .onChange(of: selectedModelIndex) { _, newValue in
                                let models = selectedProvider.presetModels
                                if newValue < models.count {
                                    aiService.setModelId(models[newValue])
                                }
                            }
                            Button("Custom") {
                                useCustomModel = true
                                customModel = aiService.modelId
                            }
                            .buttonStyle(.borderless)
                        }
                    }
                }
            }

            // API Key
            if selectedProvider.requiresAPIKey || selectedProvider == .ollama {
                HStack {
                    Text("API Key:")
                        .frame(width: 120, alignment: .trailing)
                    VStack(alignment: .leading, spacing: 4) {
                        SecureField(apiKeyPlaceholder, text: $apiKey)
                            .textFieldStyle(.roundedBorder)
                            .onChange(of: apiKey) { _, newValue in
                                aiService.apiKey = newValue
                            }
                        if selectedProvider == .ollama {
                            Text("Optional - for Ollama Cloud (ollama.com/settings/keys)")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
            }

            // Base URL (for Ollama local or custom endpoints)
            if selectedProvider == .ollama {
                HStack {
                    Text("Server URL:")
                        .frame(width: 120, alignment: .trailing)
                    VStack(alignment: .leading, spacing: 4) {
                        TextField("http://localhost:11434", text: $baseURL)
                            .textFieldStyle(.roundedBorder)
                            .onChange(of: baseURL) { _, newValue in
                                aiService.setBaseURL(newValue.isEmpty ? nil : newValue)
                            }
                        Text("Leave empty for local, or use https://ollama.com for cloud")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
            }

            Divider()

            // Output Name
            HStack {
                Text("Output Name:")
                    .frame(width: 120, alignment: .trailing)
                TextField("Workflow Documentation", text: $job.outputName)
                    .textFieldStyle(.roundedBorder)
            }

            // Capture Target
            HStack {
                Text("Capture Target:")
                    .frame(width: 120, alignment: .trailing)

                Menu {
                    Button("Full Screen") {
                        job.captureMode = .fullScreen
                    }
                    Button("Select Region...") {
                        showRegionSelector()
                    }
                    Button("Select Window...") {
                        showWindowPicker = true
                    }
                } label: {
                    HStack {
                        Image(systemName: job.captureMode.icon)
                        Text(job.captureMode.displayName)
                        Spacer()
                        Image(systemName: "chevron.down")
                    }
                    .padding(.horizontal, 8)
                    .padding(.vertical, 6)
                    .background(Color(nsColor: .controlBackgroundColor))
                    .cornerRadius(6)
                }
                .menuStyle(.borderlessButton)
                .frame(maxWidth: .infinity)
            }

            // Next Action
            HStack {
                Text("Next Action:")
                    .frame(width: 120, alignment: .trailing)

                Menu {
                    Button("→ Right Arrow") {
                        job.nextAction = .keyPress(keyCode: 124, modifiers: [])
                    }
                    Button("← Left Arrow") {
                        job.nextAction = .keyPress(keyCode: 123, modifiers: [])
                    }
                    Button("↓ Down Arrow") {
                        job.nextAction = .keyPress(keyCode: 125, modifiers: [])
                    }
                    Button("Space") {
                        job.nextAction = .keyPress(keyCode: 49, modifiers: [])
                    }
                    Button("Enter") {
                        job.nextAction = .keyPress(keyCode: 36, modifiers: [])
                    }
                    Button("None (Manual)") {
                        job.nextAction = .none
                    }
                } label: {
                    HStack {
                        Text(job.nextAction.displayName)
                        Spacer()
                        Image(systemName: "chevron.down")
                    }
                    .padding(.horizontal, 8)
                    .padding(.vertical, 6)
                    .background(Color(nsColor: .controlBackgroundColor))
                    .cornerRadius(6)
                }
                .menuStyle(.borderlessButton)
                .frame(maxWidth: .infinity)
            }

            // Capture Count
            HStack {
                Text("Capture Count:")
                    .frame(width: 120, alignment: .trailing)

                Stepper("\(job.captureCount) steps", value: $job.captureCount, in: 1...100)
            }

            // Delay
            HStack {
                Text("Delay:")
                    .frame(width: 120, alignment: .trailing)

                Slider(value: $job.delayBetweenCaptures, in: 0.2...3.0, step: 0.1)
                Text("\(job.delayBetweenCaptures, specifier: "%.1f")s")
                    .frame(width: 40)
            }

            // Prompt Template
            HStack {
                Text("Prompt Template:")
                    .frame(width: 120, alignment: .trailing)

                Picker("", selection: $selectedTemplate) {
                    ForEach(PromptTemplate.allCases) { template in
                        Label(template.rawValue, systemImage: template.icon).tag(template)
                    }
                }
                .pickerStyle(.menu)
                .frame(maxWidth: .infinity)
                .onChange(of: selectedTemplate) { _, newValue in
                    aiService.setTemplate(newValue)
                }
            }
        }
        .onAppear {
            syncUIWithService()
        }
    }

    private var apiKeyPlaceholder: String {
        switch selectedProvider {
        case .openRouter: return "sk-or-..."
        case .openAI: return "sk-..."
        case .anthropic: return "sk-ant-..."
        case .ollama: return "ollama_... (optional for cloud)"
        }
    }

    private func syncUIWithService() {
        selectedProvider = aiService.providerType
        apiKey = aiService.apiKey
        baseURL = aiService.config.baseURL ?? ""
        selectedTemplate = aiService.selectedTemplate

        // Find current model in presets
        if let index = selectedProvider.presetModels.firstIndex(of: aiService.modelId) {
            selectedModelIndex = index
            useCustomModel = false
        } else {
            customModel = aiService.modelId
            useCustomModel = true
        }
    }

    private func showRegionSelector() {
        let window = RegionSelectorWindow()
        window.onRegionSelected = { region in
            job.captureMode = .region(region)
            regionSelectorWindow = nil
        }
        window.onCancelled = {
            regionSelectorWindow = nil
        }
        regionSelectorWindow = window
        window.show()
    }
}

// MARK: - Status Section

struct StatusSection: View {
    let captureService: AutoCaptureService

    var body: some View {
        VStack(spacing: 12) {
            switch captureService.state {
            case .idle:
                Label("Ready to capture", systemImage: "checkmark.circle")
                    .foregroundStyle(.secondary)

            case .preparing:
                HStack {
                    ProgressView()
                        .scaleEffect(0.8)
                    Text("Preparing... Switch to target app now!")
                        .foregroundStyle(.orange)
                }

            case .capturing(let current, let total):
                VStack(spacing: 8) {
                    ProgressView(value: Double(current), total: Double(total))
                    Text("Capturing: \(current) / \(total)")
                }

            case .processing(let current, let total):
                VStack(spacing: 8) {
                    ProgressView(value: Double(current), total: Double(total))
                    Text("AI Processing: \(current) / \(total)")
                }

            case .saving:
                HStack {
                    ProgressView()
                        .scaleEffect(0.8)
                    Text("Saving...")
                }

            case .completed(let url):
                VStack(spacing: 12) {
                    Label("Completed!", systemImage: "checkmark.circle.fill")
                        .foregroundStyle(.green)
                        .font(.headline)

                    Text(url.deletingLastPathComponent().path)
                        .font(.caption)
                        .foregroundStyle(.secondary)

                    HStack {
                        Button("Open Folder") {
                            NSWorkspace.shared.selectFile(url.path, inFileViewerRootedAtPath: url.deletingLastPathComponent().path)
                        }
                        Button("Open File") {
                            NSWorkspace.shared.open(url)
                        }
                    }
                }

            case .error(let message):
                VStack(spacing: 8) {
                    Label("Error", systemImage: "exclamationmark.triangle.fill")
                        .foregroundStyle(.red)
                    Text(message)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

            case .partiallyCompleted(let successCount, let failedCount):
                VStack(spacing: 12) {
                    Label("Partial Completion", systemImage: "exclamationmark.triangle.fill")
                        .foregroundStyle(.orange)
                        .font(.headline)

                    Text("\(successCount) succeeded, \(failedCount) failed")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
            }

            // Preview thumbnails with status
            if !captureService.capturedImages.isEmpty {
                ScrollView(.horizontal) {
                    HStack(spacing: 8) {
                        ForEach(Array(captureService.capturedImages.enumerated()), id: \.offset) { index, image in
                            ZStack(alignment: .topTrailing) {
                                Image(nsImage: image)
                                    .resizable()
                                    .aspectRatio(contentMode: .fit)
                                    .frame(height: 60)
                                    .cornerRadius(4)
                                    .overlay(
                                        RoundedRectangle(cornerRadius: 4)
                                            .stroke(statusColor(for: index), lineWidth: 2)
                                    )

                                // Status indicator
                                if let result = captureService.processingResults.first(where: { $0.id == index }) {
                                    statusIcon(for: result.status)
                                        .offset(x: 4, y: -4)
                                }
                            }
                        }
                    }
                    .padding(.horizontal)
                }
                .frame(height: 70)
            }
        }
        .padding()
        .frame(maxWidth: .infinity)
        .background(Color(nsColor: .controlBackgroundColor).opacity(0.3))
        .cornerRadius(12)
    }

    private func statusColor(for index: Int) -> Color {
        guard let result = captureService.processingResults.first(where: { $0.id == index }) else {
            return Color.secondary.opacity(0.3)
        }
        switch result.status {
        case .pending: return Color.secondary.opacity(0.3)
        case .processing: return Color.blue
        case .success: return Color.green
        case .failed: return Color.red
        }
    }

    @ViewBuilder
    private func statusIcon(for status: ImageProcessingResult.Status) -> some View {
        switch status {
        case .pending:
            EmptyView()
        case .processing:
            ProgressView()
                .scaleEffect(0.5)
                .frame(width: 16, height: 16)
        case .success:
            Image(systemName: "checkmark.circle.fill")
                .foregroundStyle(.green)
                .font(.caption)
                .background(Circle().fill(.white).padding(2))
        case .failed:
            Image(systemName: "xmark.circle.fill")
                .foregroundStyle(.red)
                .font(.caption)
                .background(Circle().fill(.white).padding(2))
        }
    }
}

// MARK: - Action Button

struct ActionButton: View {
    let captureService: AutoCaptureService
    let job: CaptureJob
    let aiService: AIService
    let canStart: Bool
    let canRetry: Bool

    var body: some View {
        switch captureService.state {
        case .idle, .completed, .error:
            Button {
                captureService.run(job: job, aiService: aiService)
            } label: {
                Label("Start Capture", systemImage: "play.fill")
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 8)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .disabled(!canStart)

        case .partiallyCompleted:
            HStack(spacing: 12) {
                Button {
                    captureService.retryFailed(aiService: aiService)
                } label: {
                    Label("Retry Failed", systemImage: "arrow.clockwise")
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 8)
                }
                .buttonStyle(.borderedProminent)
                .disabled(!canRetry)

                Button {
                    captureService.saveResults()
                } label: {
                    Label("Save Partial", systemImage: "square.and.arrow.down")
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 8)
                }
                .buttonStyle(.bordered)
            }

        case .preparing, .capturing, .processing, .saving:
            Button {
                captureService.cancel()
            } label: {
                Label("Cancel", systemImage: "stop.fill")
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 8)
            }
            .buttonStyle(.bordered)
            .controlSize(.large)
            .tint(.red)
        }
    }
}

// MARK: - Preview

#Preview {
    ContentView(
        captureService: .constant(AutoCaptureService()),
        job: .constant(CaptureJob()),
        documentImportService: DocumentImportService()
    )
        .environment(AppState.shared)
}
