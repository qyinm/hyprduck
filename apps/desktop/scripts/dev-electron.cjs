const { spawn } = require("node:child_process");
const http = require("node:http");

const devUrl = "http://127.0.0.1:5173";

const vite = spawn("bun", ["run", "frontend:dev"], {
  stdio: "inherit",
  shell: true,
});
vite.on("error", (error) => {
  console.error("Failed to start frontend dev process via Bun:", error);
  shutdown(1);
});

let electron = null;
let shuttingDown = false;

waitForDevServer()
  .then(() => {
    electron = spawn("bun", ["run", "electron:dev"], {
      stdio: "inherit",
      shell: true,
      env: electronAppEnv(),
    });
    electron.on("error", (error) => {
      console.error("Failed to start Electron dev process via Bun:", error);
      shutdown(1);
    });
    electron.on("exit", (code) => {
      shutdown(code ?? 0);
    });
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
  if (electron && !electron.killed) {
    electron.kill();
  }
  if (!vite.killed) {
    vite.kill();
  }
  process.exit(code);
}
