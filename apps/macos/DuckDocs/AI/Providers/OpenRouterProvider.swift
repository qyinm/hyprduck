//
//  OpenRouterProvider.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-01-31.
//

import Foundation
import AppKit
import os.log

/// AI provider for OpenRouter API
final class OpenRouterProvider: AIProvider, Sendable {
    private static let logger = Logger(subsystem: "com.duckdocs", category: "OpenRouter")
    let providerType: AIProviderType = .openRouter
    let modelId: String
    private let apiKey: String
    private let baseURL: String
    private let maxTokens: Int

    init(config: AIProviderConfig, maxTokens: Int = 4096) {
        self.modelId = config.modelId
        self.apiKey = config.apiKey
        self.baseURL = config.effectiveBaseURL
        self.maxTokens = maxTokens
    }

    func analyzeImage(_ image: NSImage, prompt: String) async throws -> String {
        guard !apiKey.isEmpty else {
            throw AIProviderError.apiKeyMissing
        }

        guard let base64Image = ImageUtils.imageToBase64(image) else {
            throw AIProviderError.imageConversionFailed
        }

        let urlString = "\(baseURL)/chat/completions"
        guard let url = URL(string: urlString) else {
            throw AIProviderError.invalidURL
        }

        Self.logger.debug("Sending request to \(self.modelId, privacy: .public)")

        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.timeoutInterval = 120 // 2 minutes for vision APIs
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        request.setValue("DuckDocs/1.0", forHTTPHeaderField: "HTTP-Referer")
        request.setValue("DuckDocs", forHTTPHeaderField: "X-Title")

        let requestBody: [String: Any] = [
            "model": modelId,
            "max_tokens": maxTokens,
            "messages": [
                [
                    "role": "user",
                    "content": [
                        [
                            "type": "text",
                            "text": prompt
                        ],
                        [
                            "type": "image_url",
                            "image_url": [
                                "url": "data:image/jpeg;base64,\(base64Image)"
                            ]
                        ]
                    ]
                ]
            ]
        ]

        request.httpBody = try JSONSerialization.data(withJSONObject: requestBody)

        let (data, response) = try await performRequestWithRetry(request)

        guard let httpResponse = response as? HTTPURLResponse else {
            throw AIProviderError.invalidResponse
        }

        if httpResponse.statusCode != 200 {
            let responseString = String(data: data, encoding: .utf8) ?? "No response body"
            Self.logger.error("Error response (status \(httpResponse.statusCode)): \(responseString, privacy: .public)")

            if let errorJson = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
               let error = errorJson["error"] as? [String: Any],
               let message = error["message"] as? String {
                throw AIProviderError.serverError(httpResponse.statusCode, message)
            }
            throw AIProviderError.serverError(httpResponse.statusCode, responseString)
        }

        guard let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let choices = json["choices"] as? [[String: Any]],
              let firstChoice = choices.first,
              let message = firstChoice["message"] as? [String: Any],
              let content = message["content"] as? String else {
            throw AIProviderError.invalidResponse
        }

        Self.logger.info("Analysis complete: \(content.prefix(100), privacy: .public)...")
        return content
    }

    private func performRequestWithRetry(_ request: URLRequest, maxRetries: Int = 3) async throws -> (Data, URLResponse) {
        var lastError: Error?
        for attempt in 0..<maxRetries {
            do {
                return try await URLSession.shared.data(for: request)
            } catch {
                lastError = error
                // Don't retry on the last attempt
                if attempt < maxRetries - 1 {
                    // Exponential backoff: 1s, 2s, 4s
                    let delay = UInt64(pow(2.0, Double(attempt))) * 1_000_000_000
                    try await Task.sleep(nanoseconds: delay)
                }
            }
        }
        throw lastError ?? AIProviderError.networkError(NSError(domain: "DuckDocs", code: -1))
    }
}
