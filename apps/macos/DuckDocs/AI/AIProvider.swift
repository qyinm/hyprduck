//
//  AIProvider.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-01-31.
//

import Foundation
import AppKit
import os.log

// MARK: - Provider Types

/// AI provider types supported by DuckDocs
enum AIProviderType: String, CaseIterable, Codable, Identifiable {
    case openRouter = "OpenRouter"
    case openAI = "OpenAI"
    case anthropic = "Anthropic"
    case ollama = "Ollama"

    var id: String { rawValue }

    /// Environment variable name for API key
    var envVariable: String {
        switch self {
        case .openRouter: return "OPENROUTER_API_KEY"
        case .openAI: return "OPENAI_API_KEY"
        case .anthropic: return "ANTHROPIC_API_KEY"
        case .ollama: return "OLLAMA_API_KEY"
        }
    }

    /// Default base URL for the provider
    var defaultBaseURL: String {
        switch self {
        case .openRouter: return "https://openrouter.ai/api/v1"
        case .openAI: return "https://api.openai.com/v1"
        case .anthropic: return "https://api.anthropic.com/v1"
        case .ollama: return "http://127.0.0.1:11434"
        }
    }

    /// Cloud base URL (for providers that support cloud)
    var cloudBaseURL: String? {
        switch self {
        case .ollama: return "https://ollama.com"
        default: return nil
        }
    }

    /// Whether this provider requires an API key
    var requiresAPIKey: Bool {
        switch self {
        case .openRouter, .openAI, .anthropic: return true
        case .ollama: return false // Optional for cloud, not needed for local
        }
    }

    /// Whether this provider supports cloud mode
    var supportsCloud: Bool {
        switch self {
        case .ollama: return true
        default: return false
        }
    }

    /// Preset model options for this provider
    var presetModels: [String] {
        AIModelRegistry.modelIds(for: self)
    }

    /// Default model for this provider
    var defaultModel: String {
        AIModelRegistry.defaultModel(for: self)
    }
}

// MARK: - Provider Protocol

/// Protocol for AI providers that can analyze images
protocol AIProvider: Sendable {
    /// The type of this provider
    var providerType: AIProviderType { get }

    /// The model ID being used
    var modelId: String { get }

    /// Analyze an image and return markdown content
    /// - Parameters:
    ///   - image: The image to analyze
    ///   - prompt: The prompt for analysis
    /// - Returns: Markdown content extracted from the image
    func analyzeImage(_ image: NSImage, prompt: String) async throws -> String
}

// MARK: - Provider Configuration

/// Configuration for an AI provider
struct AIProviderConfig: Codable, Equatable {
    var providerType: AIProviderType
    var modelId: String
    var apiKey: String
    var baseURL: String?

    /// Create a default configuration for a provider type
    static func defaultConfig(for type: AIProviderType) -> AIProviderConfig {
        AIProviderConfig(
            providerType: type,
            modelId: type.defaultModel,
            apiKey: "",
            baseURL: nil
        )
    }

    /// The effective base URL (custom or default)
    var effectiveBaseURL: String {
        baseURL?.isEmpty == false ? baseURL! : providerType.defaultBaseURL
    }
}

// MARK: - Provider Errors

/// Errors that can occur during AI provider operations
enum AIProviderError: LocalizedError {
    case apiKeyMissing
    case imageConversionFailed
    case invalidURL
    case requestFailed(String)
    case invalidResponse
    case serverError(Int, String)
    case networkError(Error)

    var errorDescription: String? {
        switch self {
        case .apiKeyMissing:
            return "API key is missing. Please set it in Settings."
        case .imageConversionFailed:
            return "Failed to convert image for upload."
        case .invalidURL:
            return "Invalid API URL."
        case .requestFailed(let message):
            return "Request failed: \(message)"
        case .invalidResponse:
            return "Invalid response from server."
        case .serverError(let code, let message):
            return "Server error (\(code)): \(message)"
        case .networkError(let error):
            return "Network error: \(error.localizedDescription)"
        }
    }
}

// MARK: - Image Utilities

/// Shared utilities for image processing
enum ImageUtils {
    private static let logger = Logger(subsystem: "com.duckdocs", category: "ImageUtils")
    /// Convert NSImage to base64 JPEG string
    /// - Parameters:
    ///   - image: The image to convert
    ///   - maxDimension: Maximum dimension for resizing (default 2048)
    ///   - quality: JPEG compression quality (default 0.8)
    /// - Returns: Base64 encoded JPEG string, or nil if conversion fails
    static func imageToBase64(_ image: NSImage, maxDimension: CGFloat = 2048, quality: Double = 0.8) -> String? {
        // Resize if too large
        var targetSize = image.size

        if image.size.width > maxDimension || image.size.height > maxDimension {
            let scale = maxDimension / max(image.size.width, image.size.height)
            targetSize = NSSize(width: image.size.width * scale, height: image.size.height * scale)
        }

        let resizedImage = NSImage(size: targetSize)
        resizedImage.lockFocus()
        image.draw(in: NSRect(origin: .zero, size: targetSize),
                   from: NSRect(origin: .zero, size: image.size),
                   operation: .copy,
                   fraction: 1.0)
        resizedImage.unlockFocus()

        guard let tiffData = resizedImage.tiffRepresentation,
              let bitmap = NSBitmapImageRep(data: tiffData),
              let jpegData = bitmap.representation(using: .jpeg, properties: [.compressionFactor: quality]) else {
            return nil
        }

        logger.debug("Image size: \(Int(targetSize.width), privacy: .public)x\(Int(targetSize.height), privacy: .public), data: \(jpegData.count / 1024, privacy: .public)KB")
        return jpegData.base64EncodedString()
    }
}
