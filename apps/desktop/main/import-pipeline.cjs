const fs = require("node:fs");
const path = require("node:path");

function createImportPipeline({
  snapshot,
  pushProgressEntry,
  publishSnapshot,
  markFailed,
  applyRuntimeProgressLine,
  nextJobId,
  runEngineCommand,
  runOneShotEngineCommand,
  resetEngineRuntime,
  ensureHyprduckApplicationSupportPath,
  resolveKnownWorkspacePath,
}) {
  let graphRebuildQueue = Promise.resolve();

  async function applyWorkspaceCorrection(correction) {
    const response = await runEngineCommand("apply_correction", {
      command: "apply_correction",
      payload: correction,
    });
    const project = response.data.project;
    snapshot.lastProjectId = project.summary.projectId;
    pushProgressEntry("correction_applied", `Applied correction in ${project.summary.title}`);
    publishSnapshot();
    return project;
  }

  async function startParse(request) {
    if (snapshot.activeJob) {
      throw new Error("an import is already running");
    }

    const outputName = path.basename(request.path, path.extname(request.path)) || "document";
    const storageRoot = ensureHyprduckApplicationSupportPath();
    const parseRequest = {
      version: "1",
      template: "General",
      input: {
        path: request.path,
        format: formatForEngine(request.format),
      },
      options: {
        preserve_images: true,
        emit_structured_json: false,
        emit_svg: false,
        language_hints: [],
        debug_request_path: null,
        debug_result_path: null,
      },
      output: {
        root_dir: storageRoot,
        name: outputName,
        workspace_id: "default",
        source_id: null,
      },
    };

    snapshot.activeJob = {
      jobId: nextJobId(),
      filePath: request.path,
      format: request.format,
      status: "imported",
      progressPercent: 4,
      lastMessage: "Queued parse request",
    };
    snapshot.progressLog = [];
    publishSnapshot();

    try {
      const response = await runEngineCommand(
        "parse",
        { command: "parse", payload: parseRequest },
        { onEvent: applyRuntimeProgressLine },
      );
      const data = response?.data;
      const result = data?.result;
      if (!result) {
        markFailed("engine returned success response but missing result payload");
        return;
      }

      snapshot.lastResult = {
        savedOutputPath: data.saved_output_path ?? null,
        successCount: result.success_count ?? 0,
        failedCount: result.failed_count ?? 0,
        markdown: result.markdown,
      };
      snapshot.lastProjectId = null;
      snapshot.lastWorkspaceId = data.source_manifest?.workspace_id ?? null;
      snapshot.lastSourceId = data.source_manifest?.source_id ?? null;
      snapshot.lastSourceManifestPath = data.source_manifest?.manifest_path ?? null;
      pushProgressEntry(
        "completed",
        data.saved_output_path ?? "Parse completed without a saved output path",
      );

      if (data.saved_output_path) {
        try {
          if (snapshot.activeJob) {
            snapshot.activeJob.status = "packaging";
            snapshot.activeJob.progressPercent = 100;
            snapshot.activeJob.lastMessage = "Packaging citation evidence";
            pushProgressEntry("packaging", "Packaging citation evidence");
            publishSnapshot();
          }
          const sourceManifest = data.source_manifest ?? null;
          const project = await compileWorkspaceProject(
            data.saved_output_path,
            request.path,
            sourceManifest,
            { skipGraphGeneration: true },
          );
          snapshot.lastProjectId = project.projectId;
          snapshot.lastWorkspaceId = project.workspaceId ?? snapshot.lastWorkspaceId;
          snapshot.lastSourceId = project.sourceId ?? snapshot.lastSourceId;
          snapshot.workspaceRevision += 1;
          pushProgressEntry("compile", `Compiled knowledge workspace ${project.projectId}`);
          if (snapshot.activeJob) {
            snapshot.activeJob.status = "citation_ready";
            snapshot.activeJob.progressPercent = 94;
            snapshot.activeJob.lastMessage = "Citation-ready evidence is available";
            pushProgressEntry("citation_ready", "Citation-ready evidence is available");
          }
          let graphRebuildQueued = false;
          if (sourceManifest) {
            graphRebuildQueued = true;
            if (snapshot.activeJob) {
              snapshot.activeJob.status = "citation_ready";
              snapshot.activeJob.progressPercent = 96;
              snapshot.activeJob.lastMessage = "Preparing context graph";
            }
            pushProgressEntry("context", "Preparing context graph");
            enqueueWorkspaceGraphRebuild(
              data.saved_output_path,
              request.path,
              sourceManifest,
              snapshot.activeJob?.jobId ?? null,
            );
          }
          const graphGenerationMessage = graphGenerationNonBlockingMessage(project);
          if (graphGenerationMessage) {
            pushProgressEntry("compile", graphGenerationMessage);
          }
          if (graphRebuildQueued) {
            publishSnapshot();
            return;
          }
        } catch (error) {
          snapshot.lastProjectId = null;
          markFailed(`Knowledge workspace compile failed: ${error.message}`);
          return;
        }
      }

      snapshot.activeJob = null;
      publishSnapshot();
    } catch (error) {
      if (!snapshot.activeJob) {
        return;
      }
      markFailed(error.message);
    }
  }

  async function retryFailedPages() {
    if (snapshot.activeJob) {
      throw new Error("an import is already running");
    }
    if (!snapshot.lastSourceManifestPath) {
      throw new Error("No source manifest is available for failed-page retry.");
    }

    const manifestPath = resolveKnownWorkspacePath(snapshot.lastSourceManifestPath);
    const sourceManifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
    const failedPages = (sourceManifest.pages ?? []).filter((page) => page.error_message);
    if (failedPages.length === 0) {
      throw new Error("No failed pages are available to retry.");
    }

    const sourcePath = sourceManifest.original_path || sourceManifest.source_path;
    snapshot.activeJob = {
      jobId: nextJobId(),
      filePath: sourcePath,
      format: sourceManifest.format,
      status: "imported",
      progressPercent: 4,
      lastMessage: `Queued retry for ${failedPages.length} failed page${
        failedPages.length === 1 ? "" : "s"
      }`,
    };
    snapshot.progressLog = [];
    pushProgressEntry("retry", snapshot.activeJob.lastMessage);
    publishSnapshot();

    try {
      const parseResponse = await runEngineCommand(
        "parse",
        {
          command: "parse",
          payload: {
            version: "1",
            template: "General",
            input: {
              path: sourcePath,
              format: formatForEngine(sourceManifest.format),
            },
            options: {
              preserve_images: true,
              emit_structured_json: false,
              emit_svg: false,
              language_hints: [],
              debug_request_path: null,
              debug_result_path: null,
            },
            output: null,
          },
        },
        { onEvent: applyRuntimeProgressLine },
      );
      const parsedPages = parseResponse.data?.result?.pages ?? [];
      const retryPages = failedPages.map((failedPage) => {
        const parsedPage = parsedPages.find((page) => page.index === failedPage.index);
        const markdown = parsedPage?.markdown ?? null;
        const plainText = parsedPage?.plain_text ?? null;
        const retryError =
          parsedPage?.error_message ??
          (!markdown && !plainText ? "retry produced no page artifact" : null);
        return {
          pageIndex: failedPage.index,
          markdown,
          plainText,
          imageAssetPath: null,
          errorMessage: retryError,
        };
      });

      if (snapshot.activeJob) {
        snapshot.activeJob.progressPercent = 94;
        snapshot.activeJob.lastMessage = "Updating failed page artifacts";
        pushProgressEntry("retry", "Updating failed page artifacts");
        publishSnapshot();
      }

      const retryResponse = await runEngineCommand("retry_failed_pages", {
        command: "retry_failed_pages",
        payload: {
          sourceManifestPath: manifestPath,
          pages: retryPages,
        },
      });
      const retryData = retryResponse.data;
      const updatedManifest = retryData.sourceManifest;
      const remainingFailedCount = retryData.remainingFailedCount ?? 0;
      const markdown = fs.existsSync(updatedManifest.markdown_path)
        ? fs.readFileSync(updatedManifest.markdown_path, "utf8")
        : "";

      snapshot.lastResult = {
        savedOutputPath: updatedManifest.markdown_path ?? null,
        successCount: (updatedManifest.pages ?? []).length - remainingFailedCount,
        failedCount: remainingFailedCount,
        markdown,
      };
      snapshot.lastProjectId = null;
      snapshot.lastWorkspaceId = updatedManifest.workspace_id ?? snapshot.lastWorkspaceId;
      snapshot.lastSourceId = updatedManifest.source_id ?? snapshot.lastSourceId;
      snapshot.lastSourceManifestPath = updatedManifest.manifest_path ?? manifestPath;
      pushProgressEntry(
        "retry",
        `Retried ${retryData.retriedPageCount ?? 0} failed page${
          retryData.retriedPageCount === 1 ? "" : "s"
        }; ${remainingFailedCount} still failed`,
      );

      if (snapshot.activeJob) {
        snapshot.activeJob.status = "packaging";
        snapshot.activeJob.progressPercent = 100;
        snapshot.activeJob.lastMessage = "Packaging citation evidence after retry";
        pushProgressEntry("packaging", "Packaging citation evidence after retry");
        publishSnapshot();
      }
      const project = await compileWorkspaceProject(
        updatedManifest.markdown_path,
        updatedManifest.source_path,
        updatedManifest,
        { skipGraphGeneration: true },
      );
      snapshot.lastProjectId = project.projectId;
      snapshot.lastWorkspaceId = project.workspaceId ?? snapshot.lastWorkspaceId;
      snapshot.lastSourceId = project.sourceId ?? snapshot.lastSourceId;
      snapshot.workspaceRevision += 1;
      pushProgressEntry("compile", `Compiled knowledge workspace ${project.projectId}`);
      if (snapshot.activeJob) {
        snapshot.activeJob.status = "citation_ready";
        snapshot.activeJob.progressPercent = 94;
        snapshot.activeJob.lastMessage = "Citation-ready evidence is available";
        pushProgressEntry("citation_ready", "Citation-ready evidence is available after retry");
      }

      if (snapshot.activeJob) {
        snapshot.activeJob.status = "citation_ready";
        snapshot.activeJob.progressPercent = 96;
        snapshot.activeJob.lastMessage = "Preparing context graph";
      }
      pushProgressEntry("context", "Preparing context graph");
      enqueueWorkspaceGraphRebuild(
        updatedManifest.markdown_path,
        updatedManifest.source_path,
        updatedManifest,
        snapshot.activeJob?.jobId ?? null,
      );
      publishSnapshot();
    } catch (error) {
      if (!snapshot.activeJob) {
        return;
      }
      markFailed(error.message);
    }
  }

  async function cancelParse() {
    if (!snapshot.activeJob) {
      return;
    }
    resetEngineRuntime();
    markFailed("Parse canceled");
  }

  async function compileWorkspaceProject(
    sourceMarkdownPath,
    sourceDocumentPath,
    sourceManifest,
    options = {},
  ) {
    const request = {
      command: "compile_project",
      payload: {
        source_markdown_path: sourceMarkdownPath,
        source_document_path: sourceDocumentPath ?? null,
        source_manifest_path: sourceManifest?.manifest_path ?? null,
        workspace_id: sourceManifest?.workspace_id ?? null,
        source_id: sourceManifest?.source_id ?? null,
        skip_graph_generation: options.skipGraphGeneration ? true : null,
      },
    };
    const response =
      options.useRuntime === false
        ? await runOneShotEngineCommand("compile_project", request)
        : await runEngineCommand("compile_project", request);
    return {
      projectId: response.data.project_id,
      workspaceId: response.data.workspace_id,
      sourceId: response.data.source_id,
      graphGenerationStatus: response.data.graph_generation_status ?? null,
      graphGenerationSkippedReason: response.data.graph_generation_skipped_reason ?? null,
      graphGenerationErrorMessage: response.data.graph_generation_error_message ?? null,
    };
  }

  function enqueueWorkspaceGraphRebuild(
    sourceMarkdownPath,
    sourceDocumentPath,
    sourceManifest,
    activeJobId,
  ) {
    graphRebuildQueue = graphRebuildQueue
      .catch(() => {})
      .then(() =>
        runWorkspaceGraphRebuild(
          sourceMarkdownPath,
          sourceDocumentPath,
          sourceManifest,
          activeJobId,
        ),
      );
  }

  async function runWorkspaceGraphRebuild(
    sourceMarkdownPath,
    sourceDocumentPath,
    sourceManifest,
    activeJobId,
  ) {
    updateActiveGraphRebuildJob(activeJobId, {
      status: "citation_ready",
      progressPercent: 96,
      lastMessage: "Preparing context graph",
    });
    pushProgressEntry("graph", `Rebuilding workspace graph for ${sourceManifest.output_name}`);
    publishSnapshot();
    try {
      const project = await compileWorkspaceProject(
        sourceMarkdownPath,
        sourceDocumentPath,
        sourceManifest,
        { skipGraphGeneration: false, useRuntime: false },
      );
      snapshot.lastProjectId = project.projectId;
      snapshot.lastWorkspaceId = project.workspaceId ?? snapshot.lastWorkspaceId;
      snapshot.lastSourceId = project.sourceId ?? snapshot.lastSourceId;
      if (isGraphGenerationBlockingFailure(project.graphGenerationStatus)) {
        const message = graphGenerationFailureMessage(project);
        pushProgressEntry("graph", message);
        updateActiveGraphRebuildJob(activeJobId, {
          status: "failed",
          progressPercent: 100,
          lastMessage: message,
        });
        publishSnapshot();
        clearActiveGraphRebuildJob(activeJobId);
        return;
      }
      snapshot.workspaceRevision += 1;
      pushProgressEntry(
        "graph",
        graphGenerationNonBlockingMessage(project) ?? "Workspace graph rebuild completed",
      );
      updateActiveGraphRebuildJob(activeJobId, {
        status: "context_ready",
        progressPercent: 100,
        lastMessage: "Workspace graph rebuild completed",
      });
      publishSnapshot();
      clearActiveGraphRebuildJob(activeJobId);
    } catch (error) {
      pushProgressEntry("graph", `Workspace graph rebuild failed: ${error.message}`);
      updateActiveGraphRebuildJob(activeJobId, {
        status: "failed",
        progressPercent: 100,
        lastMessage: `Workspace graph rebuild failed: ${error.message}`,
      });
      publishSnapshot();
      clearActiveGraphRebuildJob(activeJobId);
    }
  }

  function updateActiveGraphRebuildJob(activeJobId, patch) {
    if (!activeJobId || snapshot.activeJob?.jobId !== activeJobId) {
      return;
    }
    snapshot.activeJob = {
      ...snapshot.activeJob,
      ...patch,
    };
  }

  function clearActiveGraphRebuildJob(activeJobId) {
    if (!activeJobId || snapshot.activeJob?.jobId !== activeJobId) {
      return;
    }
    snapshot.activeJob = null;
    publishSnapshot();
  }

  function isGraphGenerationBlockingFailure(status) {
    return status === "failed";
  }

  function graphGenerationFailureMessage(project) {
    if (project.graphGenerationErrorMessage) {
      return `Knowledge graph generation failed: ${project.graphGenerationErrorMessage}`;
    }
    if (project.graphGenerationSkippedReason) {
      return `Knowledge graph generation skipped: ${project.graphGenerationSkippedReason}`;
    }
    return `Knowledge graph generation failed with status: ${project.graphGenerationStatus}`;
  }

  function graphGenerationNonBlockingMessage(project) {
    if (!project.graphGenerationStatus) {
      return null;
    }
    if (project.graphGenerationStatus === "skipped") {
      return project.graphGenerationSkippedReason
        ? `Knowledge graph generation skipped: ${project.graphGenerationSkippedReason}`
        : "Knowledge graph generation skipped";
    }
    if (project.graphGenerationStatus === "empty") {
      return "Knowledge graph generation completed with no workspace changes";
    }
    if (project.graphGenerationStatus === "rebuilt") {
      return "Workspace graph rebuild completed";
    }
    if (project.graphGenerationStatus === "partially_applied") {
      return project.graphGenerationErrorMessage
        ? `Knowledge graph generation partially applied: ${project.graphGenerationErrorMessage}`
        : "Knowledge graph generation partially applied";
    }
    return null;
  }

  function detectFormat(filePath) {
    const ext = path.extname(filePath).slice(1).toLowerCase();
    if (["pdf", "docx", "doc"].includes(ext)) return ext;
    if (["png", "jpg", "jpeg", "webp"].includes(ext)) return "image";
    return null;
  }

  function formatForEngine(format) {
    switch (String(format).toLowerCase()) {
      case "pdf":
        return "pdf";
      case "docx":
        return "docx";
      case "doc":
        return "doc";
      case "image":
        return "image";
      default:
        throw new Error(`unsupported format: ${format}`);
    }
  }

  return {
    applyWorkspaceCorrection,
    startParse,
    retryFailedPages,
    cancelParse,
    compileWorkspaceProject,
    detectFormat,
  };
}

module.exports = {
  createImportPipeline,
};
