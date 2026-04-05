//
//  ActionRecorder.swift
//  DuckDocs
//
//  Created by DuckDocs on 2026-01-30.
//

import Foundation

/// Legacy recording subsystem retained for future product exploration.
@Observable
@MainActor
final class ActionRecorder {
    /// Current recording state
    enum State {
        case idle
        case recording
        case paused
    }

    /// Current state
    private(set) var state: State = .idle

    /// Actions recorded so far
    private(set) var recordedActions: [Action] = []

    /// Number of actions recorded
    var actionCount: Int { recordedActions.count }

    /// Recording start time
    private var startTime: Date?

    /// Event monitor instance
    @ObservationIgnored
    private var eventMonitor: EventMonitor?

    /// Callback when recording completes
    var onRecordingComplete: ((ActionSequence) -> Void)?

    /// Callback when an action is recorded
    var onActionRecorded: ((Action) -> Void)?

    /// Callback on error
    var onError: ((Error) -> Void)?

    /// Sequence name for the recording
    private var sequenceName: String = "Untitled Recording"

    init() {}

    /// Start recording with a given name
    nonisolated func startRecording(name: String = "Untitled Recording") {
        Task { @MainActor in
            await startRecordingOnMain(name: name)
        }
    }

    @MainActor
    private func startRecordingOnMain(name: String) async {
        guard state == .idle else { return }

        sequenceName = name
        recordedActions = []
        startTime = Date()

        // Create and configure event monitor
        let monitor = EventMonitor()
        monitor.onActionCaptured = { [weak self] action in
            Task { @MainActor in
                self?.handleAction(action)
            }
        }
        monitor.onError = { [weak self] error in
            Task { @MainActor in
                self?.onError?(error)
            }
        }
        eventMonitor = monitor

        state = .recording

        // Start monitoring on background thread
        do {
            try monitor.start()
        } catch {
            state = .idle
            onError?(error)
        }
    }

    /// Pause recording
    func pauseRecording() {
        guard state == .recording else { return }
        eventMonitor?.stop()
        state = .paused
    }

    /// Resume recording
    func resumeRecording() {
        guard state == .paused else { return }

        do {
            try eventMonitor?.start()
            state = .recording
        } catch {
            onError?(error)
        }
    }

    /// Stop recording and return the recorded sequence
    func stopRecording() -> ActionSequence? {
        guard state == .recording || state == .paused else { return nil }

        eventMonitor?.stop()
        eventMonitor = nil
        state = .idle

        guard !recordedActions.isEmpty else { return nil }

        let sequence = ActionSequence(
            name: sequenceName,
            createdAt: startTime ?? Date(),
            actions: recordedActions
        )

        onRecordingComplete?(sequence)
        return sequence
    }

    /// Cancel recording without saving
    func cancelRecording() {
        eventMonitor?.stop()
        eventMonitor = nil
        recordedActions = []
        state = .idle
    }

    /// Add a manual delay action
    func addDelay(_ seconds: Double) {
        guard state == .recording || state == .paused else { return }
        handleAction(.delay(seconds: seconds))
    }

    // MARK: - Private

    private func handleAction(_ action: Action) {
        recordedActions.append(action)
        onActionRecorded?(action)
    }
}

// MARK: - Recording Session Info

extension ActionRecorder {
    /// Duration since recording started
    var recordingDuration: TimeInterval {
        guard let start = startTime else { return 0 }
        return Date().timeIntervalSince(start)
    }

    /// Formatted duration string
    var formattedDuration: String {
        let duration = recordingDuration
        let minutes = Int(duration) / 60
        let seconds = Int(duration) % 60
        return String(format: "%02d:%02d", minutes, seconds)
    }
}
