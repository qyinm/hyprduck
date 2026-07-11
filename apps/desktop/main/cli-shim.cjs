const fs = require("node:fs");
const path = require("node:path");

function ensureEtymaShellCommand(app) {
  if (!app.isPackaged && process.env.ETYMA_INSTALL_CLI_SHIM !== "1") {
    return { installed: false, reason: "development-mode" };
  }
  if (process.platform === "win32") {
    return { installed: false, reason: "unsupported-platform" };
  }

  const resolvedCliPath = resolveCliPath();
  const cliPath = resolvedCliPath ? path.resolve(resolvedCliPath) : null;
  if (!cliPath || !fs.existsSync(cliPath)) {
    return { installed: false, reason: "missing-cli" };
  }

  const binDir = path.join(app.getPath("home"), ".local", "bin");
  const shimPath = path.join(binDir, "etyma");
  const pathReady = isDirectoryOnPath(binDir);
  fs.mkdirSync(binDir, { recursive: true });

  const existing = safeLstat(shimPath);
  if (existing) {
    if (!existing.isSymbolicLink()) {
      console.warn(`etyma shell command already exists and was left unchanged: ${shimPath}`);
      return { installed: false, pathReady, reason: "existing-path" };
    }
    const currentTarget = fs.readlinkSync(shimPath);
    const resolvedTarget = path.resolve(path.dirname(shimPath), currentTarget);
    if (resolvedTarget === path.resolve(cliPath)) {
      return { installed: true, path: shimPath, pathReady };
    }
    if (!isManagedEtymaCliTarget(resolvedTarget)) {
      console.warn(`etyma shell command points elsewhere and was left unchanged: ${shimPath}`);
      return { installed: false, pathReady, reason: "existing-symlink" };
    }
    fs.unlinkSync(shimPath);
  }

  fs.symlinkSync(cliPath, shimPath);
  if (!pathReady) {
    console.warn(`etyma shell command installed at ${shimPath}, but ${binDir} is not on PATH`);
  }
  return { installed: true, path: shimPath, pathReady };
}

function resolveCliPath() {
  if (process.env.ETYMA_CLI_BIN) {
    return process.env.ETYMA_CLI_BIN;
  }

  const cliName = `etyma-${hostTriple()}`;
  const devPath = path.join(__dirname, "..", "resources", "binaries", cliName);
  if (fs.existsSync(devPath)) {
    return devPath;
  }

  const packagedResourcesPath = process.resourcesPath || path.join(__dirname, "..");
  const packagedPath = path.join(packagedResourcesPath, "binaries", cliName);
  if (fs.existsSync(packagedPath)) {
    return packagedPath;
  }

  return null;
}

function isManagedEtymaCliTarget(targetPath) {
  const normalized = path.normalize(targetPath);
  return (
    path.basename(normalized) === `etyma-${hostTriple()}` &&
    normalized.includes(`.app${path.sep}Contents${path.sep}Resources${path.sep}binaries${path.sep}`)
  );
}

function isDirectoryOnPath(directory) {
  const searchPath = process.env.PATH || "";
  return searchPath
    .split(path.delimiter)
    .filter(Boolean)
    .some((entry) => path.resolve(entry) === path.resolve(directory));
}

function safeLstat(targetPath) {
  try {
    return fs.lstatSync(targetPath);
  } catch (error) {
    if (error && error.code === "ENOENT") {
      return null;
    }
    throw error;
  }
}

function hostTriple() {
  if (process.env.ETYMA_TARGET_TRIPLE) {
    return process.env.ETYMA_TARGET_TRIPLE;
  }
  if (process.platform === "darwin" && process.arch === "arm64") return "aarch64-apple-darwin";
  if (process.platform === "darwin" && process.arch === "x64") return "x86_64-apple-darwin";
  if (process.platform === "linux" && process.arch === "x64") return "x86_64-unknown-linux-gnu";
  if (process.platform === "win32" && process.arch === "x64") return "x86_64-pc-windows-msvc";
  return `${process.arch}-${process.platform}`;
}

module.exports = {
  ensureEtymaShellCommand,
  hostTriple,
};
