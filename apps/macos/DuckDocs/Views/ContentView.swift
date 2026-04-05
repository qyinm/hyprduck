//
//  ContentView.swift
//  DuckDocs
//
//  Created by hippoo on 1/30/26.
//

import SwiftUI

struct ContentView: View {
    @State private var showAdvancedSettings = false
    let documentImportService: DocumentImportService
    private var aiService: AIService { AIService.shared }

    private var canStartImport: Bool {
        guard aiService.configurationIssue == nil else { return false }

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
                    ImportHeroSection(
                        aiSummary: aiSummary,
                        canStartImport: canStartImport,
                        importDocument: importDocument
                    )

                    HomeNoticeStack(configurationIssue: aiService.configurationIssue)

                    AdvancedSettingsPanel(
                        isExpanded: $showAdvancedSettings,
                        aiService: aiService
                    )

                    FileParsingDashboard(
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
    }

    private func importDocument() {
        ImportPanelPresenter.present(importService: documentImportService, aiService: aiService)
    }
}

struct ImportHeroSection: View {
    let aiSummary: String
    let canStartImport: Bool
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
                        Text("Parse PDFs and Word files into linked markdown with AI.")
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
                Button(action: importDocument) {
                    Label("Import File", systemImage: "square.and.arrow.down.on.square")
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 10)
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .disabled(!canStartImport)

                Text("No screen or accessibility permissions required.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
            }
            .frame(width: 260)
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

            WorkflowNotice(
                title: "Files First",
                message: "DuckDocs now focuses on parsing existing files into markdown packages. Import works without extra macOS permissions.",
                systemImage: "doc.text.magnifyingglass"
            )
        }
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

                Label("Shared for file parsing", systemImage: "slider.horizontal.3")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(20)
        .background(Color(nsColor: .controlBackgroundColor).opacity(0.52), in: RoundedRectangle(cornerRadius: 20))
    }
}

struct FileParsingDashboard: View {
    let documentImportService: DocumentImportService
    let aiService: AIService

    var body: some View {
        ViewThatFits(in: .horizontal) {
            HStack(alignment: .top, spacing: 20) {
                DocumentImportSection(importService: documentImportService, aiService: aiService)
                    .frame(maxWidth: .infinity, alignment: .top)

                ParsingGuideCard()
                    .frame(width: 320, alignment: .top)
            }

            VStack(spacing: 20) {
                DocumentImportSection(importService: documentImportService, aiService: aiService)
                ParsingGuideCard()
            }
        }
    }
}

struct ParsingGuideCard: View {
    private let steps = [
        "Choose a PDF, DOCX, or DOC file.",
        "DuckDocs converts each page into an image snapshot.",
        "The selected AI provider extracts text and structure into markdown.",
        "Results are saved with linked page images in ~/Documents/DuckDocs/."
    ]

    var body: some View {
        WorkflowCard(
            eyebrow: "Workflow",
            title: "What DuckDocs creates",
            description: "Every import becomes a markdown package with page images and provider-generated structure."
        ) {
            VStack(alignment: .leading, spacing: 14) {
                ForEach(Array(steps.enumerated()), id: \.offset) { index, step in
                    HStack(alignment: .top, spacing: 12) {
                        Text(String(format: "%02d", index + 1))
                            .font(.caption)
                            .fontWeight(.semibold)
                            .foregroundStyle(.secondary)
                            .padding(.top, 2)

                        Text(step)
                            .font(.subheadline)
                    }
                }
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

#Preview {
    ContentView(documentImportService: DocumentImportService())
        .environment(AppState.shared)
}
