//
//  RustCoreConfigurationStore.swift
//  DuckDocs
//

import Foundation

struct RustCoreConfiguration: Codable, Equatable {
    var provider: String
    var modelID: String
    var apiKey: String
    var baseURL: String?

    enum CodingKeys: String, CodingKey {
        case provider
        case modelID = "model_id"
        case apiKey = "api_key"
        case baseURL = "base_url"
    }
}

struct RustCoreConfigurationStore {
    let fileURL: URL

    init(fileURL: URL? = nil) {
        if let fileURL {
            self.fileURL = fileURL
        } else if let configDir = ProcessInfo.processInfo.environment["DUCKDOCS_CONFIG_DIR"], !configDir.isEmpty {
            self.fileURL = URL(fileURLWithPath: configDir, isDirectory: true)
                .appendingPathComponent("engine-config.json")
        } else {
            let home = FileManager.default.homeDirectoryForCurrentUser
            self.fileURL = home.appendingPathComponent(".duckdocs/engine-config.json")
        }
    }

    func load() -> RustCoreConfiguration? {
        guard let data = try? Data(contentsOf: fileURL) else { return nil }
        return try? JSONDecoder().decode(RustCoreConfiguration.self, from: data)
    }

    func save(_ configuration: RustCoreConfiguration) {
        do {
            try FileManager.default.createDirectory(at: fileURL.deletingLastPathComponent(), withIntermediateDirectories: true)
            let data = try JSONEncoder().encode(configuration)
            try data.write(to: fileURL)
        } catch {
            NSLog("Failed to persist Rust core config: \(error.localizedDescription)")
        }
    }

    func importIfNeeded(from legacyConfig: AIProviderConfig) {
        guard load() == nil else { return }
        save(RustCoreConfiguration(
            provider: legacyConfig.providerType.rustProviderSlug,
            modelID: legacyConfig.modelId,
            apiKey: legacyConfig.apiKey,
            baseURL: legacyConfig.baseURL
        ))
    }
}

extension AIProviderType {
    var rustProviderSlug: String {
        switch self {
        case .openRouter: return "open_router"
        case .openAI: return "open_ai"
        case .anthropic: return "anthropic"
        case .ollama: return "ollama"
        }
    }

    init?(rustProviderSlug: String) {
        switch rustProviderSlug {
        case "open_router": self = .openRouter
        case "open_ai": self = .openAI
        case "anthropic": self = .anthropic
        case "ollama": self = .ollama
        default: return nil
        }
    }
}
