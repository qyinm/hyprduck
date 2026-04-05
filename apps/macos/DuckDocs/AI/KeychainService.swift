//
//  KeychainService.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-01-31.
//

import Foundation
import Security

/// Errors that can occur during Keychain operations
enum KeychainError: LocalizedError {
    case saveFailed(OSStatus)
    case loadFailed(OSStatus)
    case deleteFailed(OSStatus)
    case unexpectedData

    var errorDescription: String? {
        switch self {
        case .saveFailed(let status):
            return "Failed to save to Keychain (status: \(status))"
        case .loadFailed(let status):
            return "Failed to load from Keychain (status: \(status))"
        case .deleteFailed(let status):
            return "Failed to delete from Keychain (status: \(status))"
        case .unexpectedData:
            return "Unexpected data format in Keychain"
        }
    }
}

/// Service for secure API key storage using macOS Keychain
enum KeychainService {

    /// Save an API key to the Keychain
    /// - Parameters:
    ///   - apiKey: The API key to save
    ///   - provider: The AI provider type
    /// - Throws: KeychainError if the save operation fails
    static func save(apiKey: String, for provider: AIProviderType) throws {
        let service = "com.duckdocs.\(provider.rawValue.lowercased())"
        let account = "apikey"

        guard let data = apiKey.data(using: .utf8) else {
            throw KeychainError.unexpectedData
        }

        // Try to delete existing item first
        let deleteQuery: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account
        ]
        SecItemDelete(deleteQuery as CFDictionary)

        // Add new item
        let addQuery: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecValueData as String: data,
            kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlocked
        ]

        let status = SecItemAdd(addQuery as CFDictionary, nil)

        guard status == errSecSuccess else {
            throw KeychainError.saveFailed(status)
        }
    }

    /// Load an API key from the Keychain
    /// - Parameter provider: The AI provider type
    /// - Returns: The API key if found, nil otherwise
    static func load(for provider: AIProviderType) -> String? {
        let service = "com.duckdocs.\(provider.rawValue.lowercased())"
        let account = "apikey"

        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne
        ]

        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)

        guard status == errSecSuccess,
              let data = result as? Data,
              let apiKey = String(data: data, encoding: .utf8) else {
            return nil
        }

        return apiKey
    }

    /// Delete an API key from the Keychain
    /// - Parameter provider: The AI provider type
    /// - Throws: KeychainError if the delete operation fails
    static func delete(for provider: AIProviderType) throws {
        let service = "com.duckdocs.\(provider.rawValue.lowercased())"
        let account = "apikey"

        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account
        ]

        let status = SecItemDelete(query as CFDictionary)

        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw KeychainError.deleteFailed(status)
        }
    }
}
