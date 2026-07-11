const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");
const { pathToFileURL } = require("node:url");

const SOURCE_PREVIEW_PROTOCOL = "hyprduck-source";
const MAX_INLINE_TEXT_PREVIEW_BYTES = 2 * 1024 * 1024;

function registerSourcePreviewScheme(protocol) {
  protocol.registerSchemesAsPrivileged([
    {
      scheme: SOURCE_PREVIEW_PROTOCOL,
      privileges: {
        standard: true,
        secure: true,
        supportFetchAPI: true,
        corsEnabled: true,
        stream: true,
      },
    },
  ]);
}

function createSourcePreview({ app, protocol, net, shell, ensureHyprduckApplicationSupportPath }) {
  const sourcePreviewPaths = new Map();
  let sourcePreviewProtocolRegistered = false;

  function registerSourcePreviewProtocol() {
    if (sourcePreviewProtocolRegistered) {
      return;
    }
    protocol.handle(SOURCE_PREVIEW_PROTOCOL, async (request) => {
      const url = new URL(request.url);
      const token = url.hostname;
      const entry = sourcePreviewPaths.get(token);
      if (!entry || !fs.existsSync(entry.path)) {
        return new Response("Source preview not found.", { status: 404 });
      }
      return net.fetch(pathToFileURL(entry.path).toString());
    });
    sourcePreviewProtocolRegistered = true;
  }

  function resolveKnownWorkspacePath(candidatePath) {
    if (!candidatePath || typeof candidatePath !== "string") {
      throw new Error("Missing local artifact path.");
    }
    const storageRoot = path.resolve(ensureHyprduckApplicationSupportPath());
    const expandedPath = candidatePath.startsWith("~/")
      ? path.join(app.getPath("home"), candidatePath.slice(2))
      : candidatePath;
    const candidates = path.isAbsolute(expandedPath)
      ? [expandedPath]
      : [
          path.join(storageRoot, expandedPath),
          path.join(storageRoot, "default", expandedPath),
        ];
    const resolvedPath =
      candidates.map((candidate) => path.resolve(candidate)).find((candidate) => {
        const relativePath = path.relative(storageRoot, candidate);
        return (
          relativePath.length > 0 &&
          !relativePath.startsWith("..") &&
          !path.isAbsolute(relativePath) &&
          fs.existsSync(candidate)
        );
      }) ?? path.resolve(candidates[0]);
    const relativePath = path.relative(storageRoot, resolvedPath);
    if (
      relativePath.startsWith("..") ||
      path.isAbsolute(relativePath) ||
      relativePath.length === 0
    ) {
      throw new Error("Refusing to open a path outside the HyprDuck workspace.");
    }
    return resolvedPath;
  }

  function tryResolveKnownWorkspacePath(candidatePath) {
    try {
      return { path: resolveKnownWorkspacePath(candidatePath), error: null };
    } catch (error) {
      return { path: null, error: error.message };
    }
  }

  function resolveFirstKnownWorkspacePath(candidatePaths) {
    const errors = [];
    for (const candidatePath of candidatePaths) {
      if (!candidatePath) {
        continue;
      }
      const resolved = tryResolveKnownWorkspacePath(candidatePath);
      if (resolved.path && fs.existsSync(resolved.path)) {
        return resolved;
      }
      if (resolved.error) {
        errors.push(resolved.error);
      }
    }
    return {
      path: null,
      error: errors[0] ?? "No workspace-backed source file is available.",
    };
  }

  function createSourcePreviewUrl(safePath) {
    pruneSourcePreviewPaths();
    const token = crypto.randomUUID();
    sourcePreviewPaths.set(token, { path: safePath, createdAt: Date.now() });
    return `${SOURCE_PREVIEW_PROTOCOL}://${token}/${encodeURIComponent(path.basename(safePath))}`;
  }

  function pruneSourcePreviewPaths() {
    const cutoff = Date.now() - 60 * 60 * 1000;
    for (const [token, entry] of sourcePreviewPaths.entries()) {
      if (entry.createdAt < cutoff) {
        sourcePreviewPaths.delete(token);
      }
    }
    while (sourcePreviewPaths.size > 200) {
      const firstToken = sourcePreviewPaths.keys().next().value;
      if (!firstToken) {
        return;
      }
      sourcePreviewPaths.delete(firstToken);
    }
  }

  function isPdfPreview(format, filePath) {
    return format === "pdf" || path.extname(filePath).toLowerCase() === ".pdf";
  }

  function isTextPreview(format, filePath) {
    const extension = path.extname(filePath).toLowerCase();
    return (
      ["txt", "md", "markdown", "csv", "json", "yaml", "yml"].includes(format) ||
      [".txt", ".md", ".markdown", ".csv", ".json", ".yaml", ".yml"].includes(extension)
    );
  }

  function readOriginalPreview(candidatePaths, format) {
    const resolved = resolveFirstKnownWorkspacePath(candidatePaths);
    if (!resolved.path) {
      return {
        kind: "missing",
        previewUrl: null,
        text: null,
        truncated: false,
        error: resolved.error ?? "Original file is not available.",
      };
    }
    if (!fs.existsSync(resolved.path)) {
      return {
        kind: "missing",
        previewUrl: null,
        text: null,
        truncated: false,
        error: "Original file is missing.",
      };
    }
    if (isPdfPreview(format, resolved.path)) {
      return {
        kind: "pdf",
        previewUrl: createSourcePreviewUrl(resolved.path),
        text: null,
        truncated: false,
        error: null,
      };
    }
    if (isTextPreview(format, resolved.path)) {
      const stat = fs.statSync(resolved.path);
      const byteLimit = Math.min(stat.size, MAX_INLINE_TEXT_PREVIEW_BYTES);
      const buffer = Buffer.alloc(byteLimit);
      const fd = fs.openSync(resolved.path, "r");
      try {
        fs.readSync(fd, buffer, 0, byteLimit, 0);
      } finally {
        fs.closeSync(fd);
      }
      return {
        kind: "text",
        previewUrl: null,
        text: buffer.toString("utf8"),
        truncated: stat.size > MAX_INLINE_TEXT_PREVIEW_BYTES,
        error: null,
      };
    }
    return {
      kind: "unsupported",
      previewUrl: null,
      text: null,
      truncated: false,
      error: "Inline preview is not available for this file type.",
    };
  }

  function readMarkdownPreview(markdownPath) {
    const resolved = tryResolveKnownWorkspacePath(markdownPath);
    if (!resolved.path) {
      return {
        text: null,
        missing: true,
        error: resolved.error ?? "Parsed markdown is not available.",
      };
    }
    if (!fs.existsSync(resolved.path)) {
      return {
        text: null,
        missing: true,
        error: "Parsed markdown file is missing.",
      };
    }
    try {
      return {
        text: fs.readFileSync(resolved.path, "utf8"),
        missing: false,
        error: null,
      };
    } catch (error) {
      return {
        text: null,
        missing: true,
        error: error.message,
      };
    }
  }

  function readSourceDetail(args = {}) {
    const sourceId = String(args.sourceId ?? "");
    const originalPath = String(args.originalPath ?? "");
    const sourcePath = String(args.sourcePath ?? "");
    const markdownPath = String(args.markdownPath ?? "");
    const format = String(args.format ?? "").toLowerCase();
    const originalCandidate = originalPath || sourcePath;
    const original = readOriginalPreview([originalPath, sourcePath], format);
    const markdown = readMarkdownPreview(markdownPath);
    return {
      sourceId,
      fileName: path.basename(originalCandidate || markdownPath || sourceId || "Source"),
      format,
      originalPath,
      sourcePath,
      markdownPath,
      original,
      markdown,
    };
  }

  async function openLocalArtifact(outputPath, reveal) {
    const safePath = resolveKnownWorkspacePath(outputPath);
    if (reveal) {
      if (!fs.existsSync(safePath)) {
        throw new Error(`Cannot reveal missing local artifact: ${safePath}`);
      }
      if (fs.statSync(safePath).isDirectory()) {
        const error = await shell.openPath(safePath);
        if (error) {
          throw new Error(error);
        }
        return;
      }
      shell.showItemInFolder(safePath);
      return;
    }
    const error = await shell.openPath(safePath);
    if (error) {
      throw new Error(error);
    }
  }

  return {
    SOURCE_PREVIEW_PROTOCOL,
    registerSourcePreviewProtocol,
    resolveKnownWorkspacePath,
    readSourceDetail,
    openLocalArtifact,
  };
}

module.exports = {
  SOURCE_PREVIEW_PROTOCOL,
  registerSourcePreviewScheme,
  createSourcePreview,
};
