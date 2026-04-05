//
//  AIModelInfo.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-01-31.
//

import Foundation

// MARK: - Model Registry

/// Central registry for all provider models
enum AIModelRegistry {
    /// Get all model IDs for a provider type
    static func modelIds(for provider: AIProviderType) -> [String] {
        switch provider {
        case .openRouter:
            return OpenRouterModels.all
        case .openAI:
            return OpenAIModels.all
        case .anthropic:
            return AnthropicModels.all
        case .ollama:
            return OllamaModels.all
        }
    }

    /// Get default model for a provider type
    static func defaultModel(for provider: AIProviderType) -> String {
        switch provider {
        case .openRouter:
            return OpenRouterModels.defaultModel
        case .openAI:
            return OpenAIModels.defaultModel
        case .anthropic:
            return AnthropicModels.defaultModel
        case .ollama:
            return OllamaModels.defaultModel
        }
    }
}
