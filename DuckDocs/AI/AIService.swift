//
//  AIService.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-01-31.
//

import Foundation
import AppKit
import os.log

/// Central service for AI-powered image analysis with multi-provider support
@Observable
@MainActor
final class AIService {
    private static let logger = Logger(subsystem: "com.duckdocs", category: "AIService")
    enum State: Equatable {
        case idle
        case loading
        case processing
        case error(String)
    }

    private(set) var state: State = .idle
    private(set) var progress: Double = 0.0
    private(set) var isReady: Bool = true
    private(set) var loadingProgress: Double = 1.0

    /// Current provider configuration
    private(set) var config: AIProviderConfig

    /// Custom prompt for analysis
    var customPrompt: String?

    /// Selected prompt template
    var selectedTemplate: PromptTemplate = .general

    /// Maximum tokens for generation
    var maxTokens: Int = 4096

    /// Shared instance for app-wide use
    static let shared = AIService()

    var sharedConfigurationSummary: String {
        "These AI settings power both Capture Workflow and Document Import."
    }

    var configurationIssue: String? {
        let effectiveBaseURL = config.effectiveBaseURL
        guard URL(string: effectiveBaseURL) != nil else {
            return "Enter a valid server URL for \(config.providerType.rawValue)."
        }

        switch config.providerType {
        case .openRouter, .openAI, .anthropic:
            guard !config.apiKey.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
                return "Add an API key for \(config.providerType.rawValue) before generating markdown."
            }
        case .ollama:
            let isCloudURL = effectiveBaseURL.contains("ollama.com")
            if isCloudURL && config.apiKey.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                return "Add an Ollama API key to use Ollama Cloud, or clear the server URL to use local Ollama."
            }
        }

        return nil
    }

    // MARK: - UserDefaults Keys

    private let configKey = "ai_provider_config"
    private let templateKey = "selected_prompt_template"

    // MARK: - Initialization

    init() {
        // Load saved configuration or use default
        if let data = UserDefaults.standard.data(forKey: configKey),
           let savedConfig = try? JSONDecoder().decode(AIProviderConfig.self, from: data) {
            self.config = savedConfig
        } else {
            // Default to OpenRouter
            self.config = AIProviderConfig.defaultConfig(for: .openRouter)
        }

        // Load saved template
        if let data = UserDefaults.standard.data(forKey: templateKey),
           let savedTemplate = try? JSONDecoder().decode(PromptTemplate.self, from: data) {
            self.selectedTemplate = savedTemplate
        }

        // Try to load API key from environment if not set
        loadAPIKeyFromEnvironment()
    }

    // MARK: - Provider Management

    /// Get the current provider type
    var providerType: AIProviderType {
        config.providerType
    }

    /// Get the current model ID
    var modelId: String {
        config.modelId
    }

    /// Get the current API key
    var apiKey: String {
        get { config.apiKey }
        set {
            config.apiKey = newValue
            saveConfig()
            // Save to Keychain
            try? KeychainService.save(apiKey: newValue, for: config.providerType)
        }
    }

    /// Update the selected template
    func setTemplate(_ template: PromptTemplate) {
        selectedTemplate = template
        if let data = try? JSONEncoder().encode(template) {
            UserDefaults.standard.set(data, forKey: templateKey)
        }
    }

    /// Switch to a different provider
    func switchProvider(_ type: AIProviderType) {
        // Save current API key to Keychain
        try? KeychainService.save(apiKey: config.apiKey, for: config.providerType)

        // Load new provider config
        config.providerType = type
        config.modelId = type.defaultModel

        // Try to load API key for the new provider
        if let savedKey = KeychainService.load(for: type), !savedKey.isEmpty {
            config.apiKey = savedKey
        } else if let envKey = ProcessInfo.processInfo.environment[type.envVariable], !envKey.isEmpty {
            config.apiKey = envKey
        } else {
            config.apiKey = ""
        }

        saveConfig()
    }

    /// Update the model ID
    func setModelId(_ modelId: String) {
        config.modelId = modelId
        saveConfig()
    }

    /// Update the base URL (for custom endpoints)
    func setBaseURL(_ baseURL: String?) {
        config.baseURL = baseURL
        saveConfig()
    }

    // MARK: - Image Analysis

    /// Analyze a single image and convert to markdown
    func analyzeImage(_ image: NSImage, prompt: String? = nil) async throws -> String {
        if let configurationIssue {
            throw AIProviderError.requestFailed(configurationIssue)
        }

        state = .processing
        progress = 0.0

        let analysisPrompt = prompt ?? customPrompt ?? selectedTemplate.prompt

        Self.logger.info("Using provider: \(self.config.providerType.rawValue, privacy: .public), model: \(self.config.modelId, privacy: .public)")

        let provider = createProvider()
        let result = try await provider.analyzeImage(image, prompt: analysisPrompt)

        state = .idle
        progress = 1.0

        return result
    }

    // MARK: - Private Methods

    private func createProvider() -> AIProvider {
        switch config.providerType {
        case .openRouter:
            return OpenRouterProvider(config: config, maxTokens: maxTokens)
        case .openAI:
            return OpenAIProvider(config: config, maxTokens: maxTokens)
        case .anthropic:
            return AnthropicProvider(config: config, maxTokens: maxTokens)
        case .ollama:
            return OllamaProvider(config: config)
        }
    }

    private func saveConfig() {
        if let data = try? JSONEncoder().encode(config) {
            UserDefaults.standard.set(data, forKey: configKey)
        }
    }

    private func loadAPIKeyFromEnvironment() {
        // Try to load from environment variable
        if let key = ProcessInfo.processInfo.environment[config.providerType.envVariable], !key.isEmpty {
            config.apiKey = key
            return
        }

        // Try to load from Keychain
        if let key = KeychainService.load(for: config.providerType), !key.isEmpty {
            config.apiKey = key
        }
    }
}
