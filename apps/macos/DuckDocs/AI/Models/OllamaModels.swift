//
//  OllamaModels.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-01-31.
//

import Foundation

/// Ollama supported vision models (local and cloud)
enum OllamaModels {

    // MARK: - Local Models

    static let qwen3VL: [String] = [
        "qwen3-vl:latest",
        "qwen3-vl:2b",
        "qwen3-vl:4b",
        "qwen3-vl:8b",
        "qwen3-vl:30b",
        "qwen3-vl:32b",
        "qwen3-vl:235b",
    ]

    static let qwen25VL: [String] = [
        "qwen2.5vl:latest",
        "qwen2.5vl:3b",
        "qwen2.5vl:7b",
        "qwen2.5vl:32b",
        "qwen2.5vl:72b",
    ]

    static let gemma3: [String] = [
        "gemma3:latest",
        "gemma3:4b",
        "gemma3:12b",
        "gemma3:27b",
    ]

    static let llava: [String] = [
        "llava:latest",
        "llava:7b",
        "llava:13b",
        "llava-llama3:latest",
        "llava-llama3:8b",
        "llava-phi3:latest",
        "llava-phi3:3.8b",
        "bakllava:latest",
        "bakllava:7b",
    ]

    static let llamaVision: [String] = [
        "llama4:latest",
        "llama4:16x17b",
        "llama4:127x17b",
        "llama3.2-vision:latest",
        "llama3.2-vision:11b",
        "llama3.2-vision:90b",
    ]

    static let mistral: [String] = [
        "ministral-3:latest",
        "ministral-3:3b",
        "ministral-3:8b",
        "ministral-3:14b",
        "mistral-small3.1:latest",
        "mistral-small3.1:24b",
        "mistral-small3.2:latest",
        "mistral-small3.2:24b",
    ]

    static let specialized: [String] = [
        "deepseek-ocr:latest",
        "deepseek-ocr:3b",
        "devstral-small-2:latest",
        "devstral-small-2:24b",
        "translategemma:latest",
        "translategemma:4b",
        "translategemma:12b",
        "translategemma:27b",
        "granite3.2-vision:latest",
        "granite3.2-vision:2b",
        "moondream:latest",
        "moondream:1.8b",
    ]

    // MARK: - Cloud Models (require API key)

    static let cloud: [String] = [
        "qwen3-vl:235b-cloud",
        "qwen3-vl:235b-instruct-cloud",
        "gemma3:4b-cloud",
        "gemma3:12b-cloud",
        "gemma3:27b-cloud",
        "ministral-3:3b-cloud",
        "ministral-3:8b-cloud",
        "ministral-3:14b-cloud",
        "devstral-small-2:24b-cloud",
    ]

    // MARK: - All Models

    static var local: [String] {
        qwen3VL + qwen25VL + gemma3 + llava + llamaVision + mistral + specialized
    }

    static var all: [String] {
        local + cloud
    }

    static let defaultModel = "qwen3-vl:8b"
}
