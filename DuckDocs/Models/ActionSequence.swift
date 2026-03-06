//
//  ActionSequence.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-01-30.
//

import Foundation

/// Legacy action-sequence model retained for the recording/playback subsystem.
struct ActionSequence: Codable, Identifiable, Hashable {
    static func == (lhs: ActionSequence, rhs: ActionSequence) -> Bool {
        lhs.id == rhs.id
    }

    func hash(into hasher: inout Hasher) {
        hasher.combine(id)
    }
    /// Unique identifier
    let id: UUID

    /// User-defined name for this sequence
    var name: String

    /// When this sequence was created
    let createdAt: Date

    /// The recorded actions
    var actions: [Action]

    /// Total duration based on delay actions
    var totalDuration: TimeInterval {
        actions.reduce(0) { total, action in
            if case .delay(let seconds) = action {
                return total + seconds
            }
            return total
        }
    }

    /// Number of non-delay actions
    var actionCount: Int {
        actions.filter { action in
            if case .delay = action {
                return false
            }
            return true
        }.count
    }

    init(id: UUID = UUID(), name: String, createdAt: Date = Date(), actions: [Action] = []) {
        self.id = id
        self.name = name
        self.createdAt = createdAt
        self.actions = actions
    }
}

// MARK: - Persistence

extension ActionSequence {
    /// Directory for storing action sequences
    static var storageDirectory: URL {
        let appSupport = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        let duckDocsDir = appSupport.appendingPathComponent("DuckDocs", isDirectory: true)
        let sequencesDir = duckDocsDir.appendingPathComponent("Sequences", isDirectory: true)

        try? FileManager.default.createDirectory(at: sequencesDir, withIntermediateDirectories: true)

        return sequencesDir
    }

    /// File URL for this sequence
    var fileURL: URL {
        Self.storageDirectory.appendingPathComponent("\(id.uuidString).json")
    }

    /// Save to disk
    func save() throws {
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]

        let data = try encoder.encode(self)
        try data.write(to: fileURL)
    }

    /// Load from disk
    static func load(id: UUID) throws -> ActionSequence {
        let url = storageDirectory.appendingPathComponent("\(id.uuidString).json")
        let data = try Data(contentsOf: url)

        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601

        return try decoder.decode(ActionSequence.self, from: data)
    }

    /// Load all sequences from disk
    static func loadAll() -> [ActionSequence] {
        let fileManager = FileManager.default

        guard let files = try? fileManager.contentsOfDirectory(at: storageDirectory, includingPropertiesForKeys: nil) else {
            return []
        }

        return files
            .filter { $0.pathExtension == "json" }
            .compactMap { url -> ActionSequence? in
                guard let data = try? Data(contentsOf: url) else { return nil }
                let decoder = JSONDecoder()
                decoder.dateDecodingStrategy = .iso8601
                return try? decoder.decode(ActionSequence.self, from: data)
            }
            .sorted { $0.createdAt > $1.createdAt }
    }

    /// Delete from disk
    func delete() throws {
        try FileManager.default.removeItem(at: fileURL)
    }
}
