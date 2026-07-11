const crypto = require("node:crypto");

class EngineRuntime {
  constructor(options) {
    this.spawnEngine = options.spawnEngine;
    this.child = null;
    this.stdoutBuffer = "";
    this.stderrBuffer = "";
    this.active = null;
    this.queue = [];
    this.stopping = false;
  }

  run(expectedCommand, request, options = {}) {
    return new Promise((resolve, reject) => {
      this.queue.push({
        id: uuidv7(),
        expectedCommand,
        request,
        onEvent: options.onEvent ?? null,
        resolve,
        reject,
      });
      this.pump();
    });
  }

  pump() {
    if (this.active || this.queue.length === 0) {
      return;
    }
    try {
      this.ensureStarted();
    } catch (error) {
      const next = this.queue.shift();
      next?.reject(error);
      return;
    }
    this.active = this.queue.shift();
    try {
      this.child.stdin.write(
        `${JSON.stringify({ id: this.active.id, ...this.active.request })}\n`,
      );
    } catch (error) {
      const active = this.active;
      this.active = null;
      active?.reject(new Error(`failed writing engine request: ${error.message}`));
      this.failRuntime("engine runtime stdin is unavailable");
      this.pump();
    }
  }

  ensureStarted() {
    if (this.child && !this.child.killed) {
      return;
    }
    this.stopping = false;
    const child = this.spawnEngine(["serve"]);
    this.child = child;
    this.stdoutBuffer = "";
    this.stderrBuffer = "";

    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");
    child.stdout.on("data", (chunk) => this.handleStdout(chunk));
    child.stderr.on("data", (chunk) => this.handleStderr(chunk));
    child.on("error", (error) => {
      if (this.child !== child) {
        return;
      }
      this.failRuntime(`failed to spawn etyma-engine: ${error.message}`);
    });
    child.on("close", (code) => {
      if (this.child !== child) {
        return;
      }
      const message = this.stopping
        ? "engine runtime stopped"
        : `etyma-engine runtime exited${code === null ? "" : ` with status ${code}`}`;
      this.child = null;
      this.stdoutBuffer = "";
      this.stderrBuffer = "";
      if (!this.stopping) {
        this.failRuntime(message);
      }
    });
  }

  handleStdout(chunk) {
    this.stdoutBuffer += chunk;
    const lines = this.stdoutBuffer.split(/\r?\n/);
    this.stdoutBuffer = lines.pop() ?? "";
    for (const line of lines) {
      if (!line.trim()) {
        continue;
      }
      this.completeActive(line);
    }
  }

  handleStderr(chunk) {
    this.stderrBuffer += chunk;
    const lines = this.stderrBuffer.split(/\r?\n/);
    this.stderrBuffer = lines.pop() ?? "";
    for (const line of lines) {
      if (!line.trim()) {
        continue;
      }
      this.handleRuntimeEvent(line);
    }
  }

  handleRuntimeEvent(line) {
    const active = this.active;
    if (!active) {
      return;
    }
    try {
      const message = JSON.parse(line);
      if (message.type === "event") {
        if (message.id === active.id) {
          active.onEvent?.(message.event);
        }
        return;
      }
    } catch {
      // Legacy one-shot engine mode writes raw parse events to stderr.
    }
    active.onEvent?.(line);
  }

  completeActive(line) {
    const active = this.active;
    if (!active) {
      return;
    }
    try {
      const response = JSON.parse(line);
      if (response.id !== active.id) {
        active.reject(
          new Error(`engine response id mismatch: expected ${active.id}, got ${response.id}`),
        );
        this.active = null;
        this.stop();
        return;
      } else if (response.type === "event") {
        active.onEvent?.(response.event);
        return;
      } else if (response.ok === false) {
        const error = new Error(response.error?.message ?? "engine command failed");
        error.code = response.error?.code ?? "runtime_error";
        active.reject(error);
        this.active = null;
      } else if (response.command !== active.expectedCommand) {
        active.reject(
          new Error(
            `engine response command mismatch: expected ${active.expectedCommand}, got ${response.command}`,
          ),
        );
        this.active = null;
      } else {
        active.resolve(response);
        this.active = null;
      }
    } catch (error) {
      active.reject(new Error(`failed decoding engine response: ${error.message}`));
      this.active = null;
    }
    this.pump();
  }

  failRuntime(message) {
    const error = new Error(message);
    if (this.active) {
      this.active.reject(error);
      this.active = null;
    }
    while (this.queue.length > 0) {
      this.queue.shift().reject(error);
    }
  }

  stop() {
    this.stopping = true;
    if (this.child) {
      this.child.kill();
      this.child = null;
    }
    this.failRuntime("engine runtime stopped");
  }
}

function runOneShotEngineCommand(expectedCommand, request, spawnEngine) {
  return new Promise((resolve, reject) => {
    const child = spawnEngine([]);
    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    child.on("error", (error) => {
      reject(new Error(`failed to spawn etyma-engine: ${error.message}`));
    });
    child.on("close", (code) => {
      if (code !== 0) {
        reject(new Error(lastNonEmptyLine(stderr) ?? `etyma-engine exited with status ${code}`));
        return;
      }
      try {
        const response = JSON.parse(stdout);
        if (response.ok === false) {
          reject(new Error(response.error?.message ?? "engine command failed"));
          return;
        }
        if (response.command !== expectedCommand) {
          reject(
            new Error(
              `engine response command mismatch: expected ${expectedCommand}, got ${response.command}`,
            ),
          );
          return;
        }
        resolve(response);
      } catch (error) {
        reject(new Error(`failed decoding engine response: ${error.message}`));
      }
    });
    child.stdin.end(JSON.stringify(request));
  });
}

function lastNonEmptyLine(value) {
  return value
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .pop();
}

function uuidv7() {
  const bytes = crypto.randomBytes(16);
  let timestamp = BigInt(Date.now());
  for (let index = 5; index >= 0; index -= 1) {
    bytes[index] = Number(timestamp & 0xffn);
    timestamp >>= 8n;
  }
  bytes[6] = 0x70 | (bytes[6] & 0x0f);
  bytes[8] = 0x80 | (bytes[8] & 0x3f);
  const hex = bytes.toString("hex");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

module.exports = {
  EngineRuntime,
  runOneShotEngineCommand,
};
