const { contextBridge, ipcRenderer } = require("electron");

const ALLOWED_EVENTS = new Set([
  "hyprduck://snapshot",
  "hyprduck://agent-terminal",
]);

contextBridge.exposeInMainWorld("hyprduck", {
  invoke(command, args = {}) {
    return ipcRenderer.invoke("hyprduck:invoke", command, args);
  },
  listen(eventName, handler) {
    if (!ALLOWED_EVENTS.has(eventName)) {
      throw new Error(`HyprDuck event is not allowed: ${eventName}`);
    }
    const listener = (_event, payload) => handler({ payload });
    ipcRenderer.on(eventName, listener);
    return () => ipcRenderer.removeListener(eventName, listener);
  },
});
