const SNAPSHOT_EVENT = "etyma://snapshot";
const MAX_PROGRESS_LOG = 80;

function createSnapshotState({ getMainWindow }) {
  const snapshot = {
    activeJob: null,
    progressLog: [],
    lastResult: null,
    lastProjectId: null,
    lastWorkspaceId: null,
    lastSourceId: null,
    lastSourceManifestPath: null,
    workspaceRevision: 0,
  };

  function publishSnapshot() {
    const mainWindow = getMainWindow();
    if (mainWindow && !mainWindow.isDestroyed()) {
      mainWindow.webContents.send(SNAPSHOT_EVENT, snapshot);
    }
  }

  function pushProgressEntry(phase, message) {
    snapshot.progressLog.unshift({
      phase,
      message,
      timestamp: String(Math.floor(Date.now() / 1000)),
    });
    snapshot.progressLog = snapshot.progressLog.slice(0, MAX_PROGRESS_LOG);
  }

  function markFailed(message) {
    if (snapshot.activeJob) {
      snapshot.activeJob.status = "failed";
      snapshot.activeJob.progressPercent = 100;
      snapshot.activeJob.lastMessage = message;
    }
    pushProgressEntry("failed", message);
    publishSnapshot();
    snapshot.activeJob = null;
  }

  function applyProgressEvent(event) {
    if (!snapshot.activeJob) {
      return;
    }
    snapshot.activeJob.status = "parsing";
    switch (event.type) {
      case "queued":
        snapshot.activeJob.progressPercent = 6;
        snapshot.activeJob.lastMessage = "Queued parse request";
        pushProgressEntry("queued", "Queued parse request");
        break;
      case "document_opened":
        snapshot.activeJob.progressPercent = 12;
        snapshot.activeJob.lastMessage = `Opened ${event.format}`;
        pushProgressEntry("opened", `Opened ${event.format}`);
        break;
      case "converting_pages":
        snapshot.activeJob.progressPercent = scaledProgress(event.current, event.total, 15, 48);
        snapshot.activeJob.lastMessage = `Preparing page ${event.current} of ${event.total}`;
        pushProgressEntry("converting", snapshot.activeJob.lastMessage);
        break;
      case "parsing":
        snapshot.activeJob.progressPercent = scaledProgress(event.current, event.total, 48, 88);
        snapshot.activeJob.lastMessage = `Parsing page ${event.current} of ${event.total}`;
        pushProgressEntry("parsing", snapshot.activeJob.lastMessage);
        break;
      case "packaging":
        snapshot.activeJob.status = "packaging";
        snapshot.activeJob.progressPercent = 94;
        snapshot.activeJob.lastMessage = "Saving markdown package";
        pushProgressEntry("packaging", "Saving markdown package");
        break;
      case "completed":
        snapshot.activeJob.status = "packaging";
        snapshot.activeJob.progressPercent = 100;
        snapshot.activeJob.lastMessage = "Parse completed";
        pushProgressEntry("completed", "Parse completed");
        break;
      case "failed":
        snapshot.activeJob.status = "failed";
        snapshot.activeJob.lastMessage = event.message;
        pushProgressEntry("failed", event.message);
        break;
    }
    publishSnapshot();
  }

  function applyRuntimeProgressLine(line) {
    try {
      const event = typeof line === "string" ? JSON.parse(line) : line;
      applyProgressEvent(event);
    } catch {
      // Non-event stderr is ignored; engine failures still arrive on stdout.
    }
  }

  function scaledProgress(current, total, start, end) {
    if (!total) return start;
    const pct = Math.max(0, Math.min(1, current / total));
    return start + Math.round((end - start) * pct);
  }

  function nextJobId() {
    return `job-${Date.now()}`;
  }

  return {
    SNAPSHOT_EVENT,
    snapshot,
    publishSnapshot,
    pushProgressEntry,
    markFailed,
    applyProgressEvent,
    applyRuntimeProgressLine,
    nextJobId,
  };
}

module.exports = {
  SNAPSHOT_EVENT,
  createSnapshotState,
};
