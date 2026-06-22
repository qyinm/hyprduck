const { spawn } = require("node:child_process");
const fs = require("node:fs");
const http = require("node:http");
const path = require("node:path");

const devUrl = "http://127.0.0.1:5173";
const desktopRoot = path.resolve(__dirname, "..");
const repoRoot = path.resolve(desktopRoot, "..", "..");

const vite = spawn("bun", ["run", "frontend:dev"], {
  stdio: "inherit",
  shell: true,
});
vite.on("error", (error) => {
  console.error("Failed to start frontend dev process via Bun:", error);
  shutdown(1);
});

let electron = null;
let electronRestarting = false;
let rustSync = null;
let rustSyncQueued = false;
let rustSyncTimer = null;
const rustWatchers = [];
let shuttingDown = false;

startRustEngineWatcher();

waitForDevServer()
  .then(() => {
    startElectron();
  })
  .catch((error) => {
    console.error(error);
    shutdown(1);
  });

vite.on("exit", (code) => {
  if (!shuttingDown) {
    shutdown(code ?? 0);
  }
});

process.on("SIGINT", () => shutdown(130));
process.on("SIGTERM", () => shutdown(143));

function startElectron() {
  if (shuttingDown) {
    return;
  }
  electronRestarting = false;
  electron = spawn("bun", ["run", "electron:dev"], {
    stdio: "inherit",
    shell: true,
    env: electronAppEnv(),
    detached: process.platform !== "win32",
  });
  electron.on("error", (error) => {
    console.error("Failed to start Electron dev process via Bun:", error);
    shutdown(1);
  });
  electron.on("exit", (code) => {
    electron = null;
    if (electronRestarting && !shuttingDown) {
      startElectron();
      return;
    }
    shutdown(code ?? 0);
  });
}

function restartElectron(reason) {
  if (shuttingDown) {
    return;
  }
  if (!electron || electron.killed) {
    startElectron();
    return;
  }
  console.log(`[desktop-dev] Restarting Electron after ${reason}.`);
  electronRestarting = true;
  terminateProcessTree(electron);
}

function startRustEngineWatcher() {
  const watchTargets = [
    path.join(repoRoot, "crates"),
    path.join(repoRoot, "Cargo.toml"),
    path.join(repoRoot, "Cargo.lock"),
  ];
  for (const target of watchTargets) {
    if (!fs.existsSync(target)) {
      continue;
    }
    const stat = fs.statSync(target);
    try {
      const watcher = fs.watch(
        target,
        { recursive: stat.isDirectory() },
        (_eventType, filename) => {
          if (!shouldSyncRustEngine(filename)) {
            return;
          }
          scheduleRustEngineSync(filename ? String(filename) : target);
        },
      );
      watcher.on("error", (error) => {
        console.error(`[desktop-dev] Rust watcher failed for ${target}:`, error);
      });
      rustWatchers.push(watcher);
    } catch (error) {
      console.error(`[desktop-dev] Failed to watch ${target}:`, error);
    }
  }
}

function shouldSyncRustEngine(filename) {
  if (!filename) {
    return true;
  }
  const normalized = String(filename);
  if (
    normalized.includes("target/") ||
    normalized.includes(".git/") ||
    normalized.endsWith("~") ||
    normalized.endsWith(".swp") ||
    normalized.endsWith(".tmp")
  ) {
    return false;
  }
  return (
    normalized.endsWith(".rs") ||
    normalized.endsWith(".toml") ||
    normalized.endsWith(".lock")
  );
}

function scheduleRustEngineSync(reason) {
  if (shuttingDown) {
    return;
  }
  clearTimeout(rustSyncTimer);
  rustSyncTimer = setTimeout(() => syncRustEngine(reason), 500);
}

function syncRustEngine(reason) {
  if (shuttingDown) {
    return;
  }
  if (rustSync) {
    rustSyncQueued = true;
    return;
  }
  console.log(`[desktop-dev] Rust change detected (${reason}); rebuilding engine.`);
  rustSync = spawn("bun", ["run", "sync:engine:debug"], {
    cwd: desktopRoot,
    stdio: "inherit",
    shell: true,
  });
  rustSync.on("error", (error) => {
    rustSync = null;
    console.error("[desktop-dev] Failed to run engine sync:", error);
  });
  rustSync.on("exit", (code) => {
    rustSync = null;
    if (code === 0) {
      restartElectron("engine rebuild");
    } else {
      console.error(`[desktop-dev] Engine sync exited with status ${code ?? "unknown"}.`);
    }
    if (rustSyncQueued) {
      rustSyncQueued = false;
      syncRustEngine("queued Rust change");
    }
  });
}

function waitForDevServer(deadline = Date.now() + 30000) {
  return new Promise((resolve, reject) => {
    const attempt = () => {
      const request = http.get(devUrl, (response) => {
        response.resume();
        resolve();
      });
      request.on("error", () => {
        if (Date.now() > deadline) {
          reject(new Error(`Timed out waiting for ${devUrl}`));
          return;
        }
        setTimeout(attempt, 250);
      });
      request.setTimeout(1000, () => {
        request.destroy(new Error("dev server probe timed out"));
      });
    };
    attempt();
  });
}

function electronAppEnv() {
  const env = {
    ...process.env,
    VITE_DEV_SERVER_URL: devUrl,
  };
  delete env.ELECTRON_RUN_AS_NODE;
  return env;
}

function shutdown(code) {
  if (shuttingDown) {
    return;
  }
  shuttingDown = true;
  clearTimeout(rustSyncTimer);
  for (const watcher of rustWatchers) {
    watcher.close();
  }
  if (electron && !electron.killed) {
    terminateProcessTree(electron);
  }
  if (rustSync && !rustSync.killed) {
    rustSync.kill();
  }
  if (!vite.killed) {
    vite.kill();
  }
  process.exit(code);
}

function terminateProcessTree(child) {
  if (!child || child.killed) {
    return;
  }
  if (process.platform !== "win32") {
    try {
      process.kill(-child.pid, "SIGTERM");
      return;
    } catch {
      // Fall through to killing the direct child.
    }
  }
  child.kill();
}
