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
    @State private var showAdvancedSettings = false
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

    private var canStartCapture: Bool {
        guard appState.canUseCapture, aiService.configurationIssue == nil else { return false }

        switch captureService.state {
        case .idle, .completed, .error, .partiallyCompleted:
            return true
        default:
            return false
        }
    }

    private var canStartImport: Bool {
        guard appState.canUseImport, aiService.configurationIssue == nil else { return false }

        switch documentImportService.state {
        case .converting, .processing, .saving:
            return false
        default:
            return true
        }
    }

    private var aiSummary: String {
        "\(aiService.providerType.rawValue)  ·  \(aiService.modelId)  ·  \(aiService.selectedTemplate.rawValue)"
    }

    var body: some View {
        ZStack {
            LinearGradient(
                colors: [
                    Color(nsColor: .windowBackgroundColor),
                    Color(nsColor: .controlBackgroundColor).opacity(0.75)
                ],
                startPoint: .topLeading,
                endPoint: .bottomTrailing
            )
            .ignoresSafeArea()

            ScrollView {
                VStack(alignment: .leading, spacing: 24) {
                    HeroSection(
                        aiSummary: aiSummary,
                        canStartCapture: canStartCapture,
                        canStartImport: canStartImport,
                        startCapture: startCapture,
                        importDocument: importDocument
                    )

                    if aiService.configurationIssue != nil || !appState.canUseCapture {
                        HomeNoticeStack(
                            permissionManager: appState.permissionManager,
                            configurationIssue: aiService.configurationIssue
                        )
                    }

                    AdvancedSettingsPanel(
                        isExpanded: $showAdvancedSettings,
                        aiService: aiService
                    )

                    WorkflowDashboard(
                        captureService: $captureService,
                        job: $job,
                        showWindowPicker: $showWindowPicker,
                        documentImportService: documentImportService,
                        aiService: aiService
                    )
                }
                .padding(32)
                .frame(maxWidth: 1100)
                .frame(maxWidth: .infinity)
            }
        }
        .frame(minWidth: 720, minHeight: 640)
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

    private func startCapture() {
        guard canStartCapture else { return }
        captureService.run(job: job, aiService: aiService)
    }

    private func importDocument() {
        ImportPanelPresenter.present(importService: documentImportService, aiService: aiService)
    }
}

struct HeroSection: View {
    let aiSummary: String
    let canStartCapture: Bool
    let canStartImport: Bool
    let startCapture: () -> Void
    let importDocument: () -> Void

    var body: some View {
        HStack(alignment: .top, spacing: 24) {
            VStack(alignment: .leading, spacing: 14) {
                HStack(spacing: 10) {
                    Image(nsImage: NSApp.applicationIconImage)
                        .resizable()
                        .aspectRatio(contentMode: .fit)
                        .frame(width: 56, height: 56)
                        .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
                        .overlay(
                            RoundedRectangle(cornerRadius: 14, style: .continuous)
                                .stroke(Color.white.opacity(0.7), lineWidth: 1)
                        )

                    VStack(alignment: .leading, spacing: 2) {
                        Text("DuckDocs")
                            .font(.system(size: 32, weight: .bold, design: .rounded))
                        Text("Capture steps or import docs, then ship polished markdown.")
                            .foregroundStyle(.secondary)
                    }
                }

                HStack(spacing: 8) {
                    Label("AI Engine", systemImage: "bolt.fill")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Text(aiSummary)
                        .font(.caption)
                        .foregroundStyle(.primary)
                        .lineLimit(1)
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 10)
                .background(Color.white.opacity(0.72), in: Capsule())
            }

            Spacer(minLength: 0)

            VStack(spacing: 12) {
                Button(action: startCapture) {
                    Label("Start Capture", systemImage: "play.fill")
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 10)
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .disabled(!canStartCapture)

                Button(action: importDocument) {
                    Label("Import File", systemImage: "square.and.arrow.down.on.square")
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 10)
                }
                .buttonStyle(.bordered)
                .controlSize(.large)
                .disabled(!canStartImport)
            }
            .frame(width: 220)
        }
        .padding(28)
        .background(
            RoundedRectangle(cornerRadius: 28, style: .continuous)
                .fill(
                    LinearGradient(
                        colors: [
                            Color.white.opacity(0.92),
                            Color(nsColor: .controlBackgroundColor).opacity(0.78)
                        ],
                        startPoint: .topLeading,
                        endPoint: .bottomTrailing
                    )
                )
        )
        .overlay(
            RoundedRectangle(cornerRadius: 28, style: .continuous)
                .stroke(Color.white.opacity(0.55), lineWidth: 1)
        )
    }
}

struct HomeNoticeStack: View {
    @Bindable var permissionManager: PermissionManager
    let configurationIssue: String?

    var body: some View {
        VStack(spacing: 12) {
            if let configurationIssue {
                WorkflowNotice(
                    title: "AI Setup Required",
                    message: configurationIssue,
                    systemImage: "bolt.horizontal.circle"
                )
            }

            if !permissionManager.canUseCapture {
                CompactPermissionNotice(permissionManager: permissionManager)
            }
        }
    }
}

struct CompactPermissionNotice: View {
    @Bindable var permissionManager: PermissionManager

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: "lock.shield")
                .foregroundStyle(.orange)

            VStack(alignment: .leading, spacing: 4) {
                Text("Capture needs macOS permissions")
                    .font(.subheadline)
                    .fontWeight(.semibold)
                Text(permissionManager.capturePermissionCallout)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Spacer()

            HStack(spacing: 8) {
                if !permissionManager.accessibilityGranted {
                    Button("Accessibility") {
                        permissionManager.requestAccessibilityPermission()
                    }
                    .buttonStyle(.bordered)
                }

                if !permissionManager.screenCaptureGranted {
                    Button("Screen Recording") {
                        permissionManager.requestScreenCapturePermission()
                    }
                    .buttonStyle(.bordered)
                }
            }
        }
        .padding()
        .background(Color.orange.opacity(0.08), in: RoundedRectangle(cornerRadius: 16))
    }
}

struct AdvancedSettingsPanel: View {
    @Binding var isExpanded: Bool
    let aiService: AIService

    var body: some View {
        DisclosureGroup(isExpanded: $isExpanded) {
            AISettingsForm(aiService: aiService)
                .padding(.top, 16)
        } label: {
            HStack {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Advanced AI Settings")
                        .font(.headline)
                    Text("Provider, model, template, API key, and Ollama server settings.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                Spacer()

                Label("Shared for Capture and Import", systemImage: "slider.horizontal.3")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(20)
        .background(Color(nsColor: .controlBackgroundColor).opacity(0.52), in: RoundedRectangle(cornerRadius: 20))
    }
}

struct WorkflowDashboard: View {
    @Binding var captureService: AutoCaptureService
    @Binding var job: CaptureJob
    @Binding var showWindowPicker: Bool
    let documentImportService: DocumentImportService
    let aiService: AIService

    var body: some View {
        ViewThatFits(in: .horizontal) {
            HStack(alignment: .top, spacing: 20) {
                CaptureWorkflowSection(
                    captureService: $captureService,
                    job: $job,
                    showWindowPicker: $showWindowPicker,
                    aiService: aiService
                )
                .frame(maxWidth: .infinity, alignment: .top)

                DocumentImportSection(importService: documentImportService, aiService: aiService)
                    .frame(maxWidth: .infinity, alignment: .top)
            }

            VStack(spacing: 20) {
                CaptureWorkflowSection(
                    captureService: $captureService,
                    job: $job,
                    showWindowPicker: $showWindowPicker,
                    aiService: aiService
                )

                DocumentImportSection(importService: documentImportService, aiService: aiService)
            }
        }
    }
}

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
        WorkflowCard(
            eyebrow: "Capture Workflow",
            title: "Automate step-by-step screenshots",
            description: "Choose a target, decide the next action, and turn the resulting sequence into a markdown guide."
        ) {
            VStack(alignment: .leading, spacing: 16) {
                if !appState.canUseCapture {
                    CapturePermissionsBanner(permissionManager: appState.permissionManager)
                }

                CaptureSummarySettingsSection(job: $job, showWindowPicker: $showWindowPicker)

                StatusSection(captureService: captureService)

                ActionButton(
                    captureService: captureService,
                    job: job,
                    aiService: aiService,
                    canStart: canStartCapture,
                    canRetry: canRetryCapture
                )
            }
        }
    }
}

struct WorkflowCard<Content: View>: View {
    let eyebrow: String
    let title: String
    let description: String
    @ViewBuilder let content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            VStack(alignment: .leading, spacing: 8) {
                Text(eyebrow.uppercased())
                    .font(.caption)
                    .fontWeight(.semibold)
                    .foregroundStyle(.secondary)
                Text(title)
                    .font(.title3)
                    .fontWeight(.semibold)
                Text(description)
                    .foregroundStyle(.secondary)
            }

            content
        }
        .padding(24)
        .background(
            RoundedRectangle(cornerRadius: 24, style: .continuous)
                .fill(Color(nsColor: .controlBackgroundColor).opacity(0.62))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 24, style: .continuous)
                .stroke(Color.white.opacity(0.42), lineWidth: 1)
        )
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

struct AISettingsForm: View {
    let aiService: AIService
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
                            Text("Optional for Ollama Cloud. Local Ollama can run without a key.")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
            }

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
                        Text("Leave empty for local, or use https://ollama.com for cloud.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
            }

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

        if let index = selectedProvider.presetModels.firstIndex(of: aiService.modelId) {
            selectedModelIndex = index
            useCustomModel = false
        } else {
            customModel = aiService.modelId
            useCustomModel = true
        }
    }
}

struct CaptureSummarySettingsSection: View {
    @Binding var job: CaptureJob
    @Binding var showWindowPicker: Bool
    @State private var regionSelectorWindow: RegionSelectorWindow?

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Setup")
                .font(.headline)

            LazyVGrid(columns: [
                GridItem(.adaptive(minimum: 220), spacing: 12)
            ], spacing: 12) {
                CaptureSettingTile(title: "Output Name", systemImage: "doc.text") {
                    TextField("Workflow Documentation", text: $job.outputName)
                        .textFieldStyle(.roundedBorder)
                }

                CaptureSettingTile(title: "Capture Target", systemImage: job.captureMode.icon) {
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
                        CompactMenuLabel(primary: job.captureMode.displayName)
                    }
                    .menuStyle(.borderlessButton)
                }

                CaptureSettingTile(title: "Next Action", systemImage: "command") {
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
                        CompactMenuLabel(primary: job.nextAction.displayName)
                    }
                    .menuStyle(.borderlessButton)
                }

                CaptureSettingTile(title: "Steps", systemImage: "number") {
                    VStack(alignment: .leading, spacing: 6) {
                        Stepper("\(job.captureCount) captures", value: $job.captureCount, in: 1...100)
                        Text("How many screenshots to take in sequence.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }

                CaptureSettingTile(title: "Delay", systemImage: "timer") {
                    VStack(alignment: .leading, spacing: 6) {
                        Slider(value: $job.delayBetweenCaptures, in: 0.2...3.0, step: 0.1)
                        Text("\(job.delayBetweenCaptures, specifier: "%.1f")s between captures")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
            }
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

struct CaptureSettingTile<Content: View>: View {
    let title: String
    let systemImage: String
    @ViewBuilder let content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Label(title, systemImage: systemImage)
                .font(.subheadline)
                .fontWeight(.medium)
            content
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.white.opacity(0.55), in: RoundedRectangle(cornerRadius: 16))
    }
}

struct CompactMenuLabel: View {
    let primary: String

    var body: some View {
        HStack {
            Text(primary)
                .lineLimit(1)
            Spacer()
            Image(systemName: "chevron.down")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 10)
        .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 10))
    }
}

struct StatusSection: View {
    let captureService: AutoCaptureService

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Status")
                .font(.headline)

            VStack(spacing: 12) {
                switch captureService.state {
                case .idle:
                    Label("Ready to capture", systemImage: "checkmark.circle")
                        .foregroundStyle(.secondary)

                case .preparing:
                    HStack {
                        ProgressView()
                            .scaleEffect(0.8)
                        Text("Preparing... Switch to the target app now.")
                            .foregroundStyle(.orange)
                    }

                case .capturing(let current, let total):
                    VStack(spacing: 8) {
                        ProgressView(value: Double(current), total: Double(total))
                        Text("Capturing \(current) of \(total)")
                    }

                case .processing(let current, let total):
                    VStack(spacing: 8) {
                        ProgressView(value: Double(current), total: Double(total))
                        Text("AI Processing \(current) of \(total)")
                    }

                case .saving:
                    HStack {
                        ProgressView()
                            .scaleEffect(0.8)
                        Text("Saving markdown and images...")
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

                if !captureService.capturedImages.isEmpty {
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 8) {
                            ForEach(Array(captureService.capturedImages.enumerated()), id: \.offset) { index, image in
                                ZStack(alignment: .topTrailing) {
                                    Image(nsImage: image)
                                        .resizable()
                                        .aspectRatio(contentMode: .fit)
                                        .frame(height: 68)
                                        .cornerRadius(8)
                                        .overlay(
                                            RoundedRectangle(cornerRadius: 8)
                                                .stroke(statusColor(for: index), lineWidth: 2)
                                        )

                                    if let result = captureService.processingResults.first(where: { $0.id == index }) {
                                        statusIcon(for: result.status)
                                            .offset(x: 4, y: -4)
                                    }
                                }
                            }
                        }
                        .padding(.horizontal, 2)
                    }
                    .frame(height: 80)
                }
            }
            .padding()
            .frame(maxWidth: .infinity)
            .background(Color.white.opacity(0.55), in: RoundedRectangle(cornerRadius: 16))
        }
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
                    .padding(.vertical, 10)
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
                        .padding(.vertical, 10)
                }
                .buttonStyle(.borderedProminent)
                .disabled(!canRetry)

                Button {
                    captureService.saveResults()
                } label: {
                    Label("Save Partial", systemImage: "square.and.arrow.down")
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 10)
                }
                .buttonStyle(.bordered)
            }

        case .preparing, .capturing, .processing, .saving:
            Button {
                captureService.cancel()
            } label: {
                Label("Cancel", systemImage: "stop.fill")
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 10)
            }
            .buttonStyle(.bordered)
            .controlSize(.large)
            .tint(.red)
        }
    }
}

#Preview {
    ContentView(
        captureService: .constant(AutoCaptureService()),
        job: .constant(CaptureJob()),
        documentImportService: DocumentImportService()
    )
    .environment(AppState.shared)
}
