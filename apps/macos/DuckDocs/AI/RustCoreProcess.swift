//
//  RustCoreProcess.swift
//  DuckDocs
//

import Foundation

enum RustCoreProcessError: LocalizedError {
    case engineNotFound
    case launchFailed(String)
    case resultMissing
    case invalidResult(String)

    var errorDescription: String? {
        switch self {
        case .engineNotFound:
            return "DuckDocs Rust engine binary could not be found."
        case .launchFailed(let message):
            return "DuckDocs Rust engine failed to launch: \(message)"
        case .resultMissing:
            return "DuckDocs Rust engine did not return a result."
        case .invalidResult(let message):
            return "DuckDocs Rust engine returned invalid output: \(message)"
        }
    }
}

final class RustCoreProcess {
    private var process: Process?

    func run(
        request: SchemaParseRequest,
        onEvent: @escaping @Sendable (SchemaProcessEvent) -> Void
    ) async throws -> SchemaParseResult {
        let executableURL = try resolveEngineURL()
        let stderrCollector = RustCoreStderrCollector()
        let process = Process()
        let stdin = Pipe()
        let stdout = Pipe()
        let stderr = Pipe()

        process.executableURL = executableURL
        process.standardInput = stdin
        process.standardOutput = stdout
        process.standardError = stderr

        self.process = process

        try process.run()

        let requestData = try JSONEncoder().encode(request)
        stdin.fileHandleForWriting.write(requestData)
        try? stdin.fileHandleForWriting.close()

        let eventTask = Task.detached(priority: .userInitiated) {
            for try await line in stderr.fileHandleForReading.bytes.lines {
                await stderrCollector.append(line)
                await MainActor.run {
                    guard let data = line.data(using: .utf8),
                          let event = try? JSONDecoder().decode(SchemaProcessEvent.self, from: data) else {
                        return
                    }
                    onEvent(event)
                }
            }
        }

        let stdoutTask = Task.detached(priority: .userInitiated) {
            try stdout.fileHandleForReading.readToEnd() ?? Data()
        }

        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            process.terminationHandler = { process in
                if process.terminationStatus == 0 {
                    continuation.resume()
                } else {
                    Task {
                        let detail = await stderrCollector.lastMessage() ?? "no stderr output"
                        continuation.resume(
                            throwing: RustCoreProcessError.launchFailed(
                                "exit status \(process.terminationStatus): \(detail)"
                            )
                        )
                    }
                }
            }
        }

        let outputData = try await stdoutTask.value
        eventTask.cancel()
        self.process = nil

        guard !outputData.isEmpty else {
            throw RustCoreProcessError.resultMissing
        }

        do {
            return try JSONDecoder().decode(SchemaParseResult.self, from: outputData)
        } catch {
            throw RustCoreProcessError.invalidResult(error.localizedDescription)
        }
    }

    func cancel() {
        process?.terminate()
        process = nil
    }

    func resolveEngineURLForAvailabilityCheck() throws -> URL {
        try resolveEngineURL()
    }

    private func resolveEngineURL() throws -> URL {
        let env = ProcessInfo.processInfo.environment
        if let explicit = env["DUCKDOCS_ENGINE_BIN"], FileManager.default.fileExists(atPath: explicit) {
            return URL(fileURLWithPath: explicit)
        }

        let candidates = [
            FileManager.default.currentDirectoryPath + "/target/debug/duckdocs-engine",
            FileManager.default.currentDirectoryPath + "/target/release/duckdocs-engine",
            Bundle.main.bundleURL.appendingPathComponent("Contents/Resources/duckdocs-engine").path
        ]

        if let match = candidates.first(where: { FileManager.default.fileExists(atPath: $0) }) {
            return URL(fileURLWithPath: match)
        }

        throw RustCoreProcessError.engineNotFound
    }
}

private actor RustCoreStderrCollector {
    private var lines: [String] = []

    func append(_ line: String) {
        lines.append(line)
    }

    func lastMessage() -> String? {
        lines.last(where: { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty })
    }
}
