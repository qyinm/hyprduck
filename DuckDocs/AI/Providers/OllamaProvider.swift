//
//  OllamaProvider.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-01-31.
//

import Foundation
import AppKit
import os.log

/// AI provider for Ollama (local or cloud)
final class OllamaProvider: AIProvider, Sendable {
    private static let logger = Logger(subsystem: "com.duckdocs", category: "Ollama")
    let providerType: AIProviderType = .ollama
    let modelId: String
    private let baseURL: String
    private let apiKey: String

    /// Whether this is using Ollama Cloud
    var isCloudMode: Bool {
        !apiKey.isEmpty || baseURL.contains("ollama.com")
    }

    init(config: AIProviderConfig) {
        self.modelId = config.modelId
        self.apiKey = config.apiKey
        // If API key is provided but using default local URL, switch to cloud URL
        if !config.apiKey.isEmpty && config.effectiveBaseURL == "http://127.0.0.1:11434" {
            self.baseURL = "https://ollama.com"
        } else {
            self.baseURL = config.effectiveBaseURL
        }
    }

    func analyzeImage(_ image: NSImage, prompt: String) async throws -> String {
        guard let base64Image = ImageUtils.imageToBase64(image) else {
            throw AIProviderError.imageConversionFailed
        }

        // Use chat API for cloud models, generate API for local
        let endpoint = isCloudMode ? "/api/chat" : "/api/generate"
        let urlString = "\(baseURL)\(endpoint)"
        guard let url = URL(string: urlString) else {
            throw AIProviderError.invalidURL
        }

        let modeLabel = isCloudMode ? "Cloud" : "Local"
        Self.logger.debug("\(modeLabel, privacy: .public) mode: Sending request to \(self.modelId, privacy: .public)")

        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        // Cloud models can take a long time to respond (especially large ones)
        request.timeoutInterval = isCloudMode ? 300 : 120 // 5 min for cloud, 2 min for local

        // Add Authorization header for cloud mode
        if !apiKey.isEmpty {
            request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        }

        let requestBody: [String: Any]
        if isCloudMode {
            // Cloud uses chat API format
            requestBody = [
                "model": modelId,
                "messages": [
                    [
                        "role": "user",
                        "content": prompt,
                        "images": [base64Image]
                    ]
                ],
                "stream": false
            ]
        } else {
            // Local uses generate API format
            requestBody = [
                "model": modelId,
                "prompt": prompt,
                "images": [base64Image],
                "stream": false
            ]
        }

        request.httpBody = try JSONSerialization.data(withJSONObject: requestBody)

        let (data, response): (Data, URLResponse)
        do {
            (data, response) = try await URLSession.shared.data(for: request)
        } catch {
            if !isCloudMode {
                throw AIProviderError.requestFailed(
                    "Cannot connect to Ollama at \(baseURL). Make sure the Ollama app or server is running."
                )
            }
            throw AIProviderError.networkError(error)
        }

        guard let httpResponse = response as? HTTPURLResponse else {
            throw AIProviderError.invalidResponse
        }

        if httpResponse.statusCode != 200 {
            let responseString = String(data: data, encoding: .utf8) ?? "No response body"
            Self.logger.error("Error response (status \(httpResponse.statusCode)): \(responseString, privacy: .public)")

            // Check if Ollama is not running (local mode)
            if !isCloudMode && (responseString.contains("connection refused") || responseString.isEmpty) {
                throw AIProviderError.requestFailed("Cannot connect to Ollama. Make sure Ollama is running at \(baseURL)")
            }

            // Check for auth errors (cloud mode)
            if httpResponse.statusCode == 401 {
                throw AIProviderError.requestFailed("Authentication failed. Check your Ollama API key at ollama.com/settings/keys")
            }

            if let errorJson = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
               let error = errorJson["error"] as? String {
                throw AIProviderError.serverError(httpResponse.statusCode, error)
            }
            throw AIProviderError.serverError(httpResponse.statusCode, responseString)
        }

        // Parse response based on API type
        guard let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw AIProviderError.invalidResponse
        }

        let responseText: String
        if isCloudMode {
            // Chat API response format
            guard let message = json["message"] as? [String: Any],
                  let content = message["content"] as? String else {
                throw AIProviderError.invalidResponse
            }
            responseText = content
        } else {
            // Generate API response format
            guard let response = json["response"] as? String else {
                throw AIProviderError.invalidResponse
            }
            responseText = response
        }

        Self.logger.info("\(modeLabel, privacy: .public) mode: Analysis complete: \(responseText.prefix(100), privacy: .public)...")
        return responseText
    }
}
