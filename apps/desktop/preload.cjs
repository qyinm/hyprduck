const { contextBridge, ipcRenderer } = require("electron");

const ALLOWED_EVENTS = new Set(["duckdocs://snapshot"]);

contextBridge.exposeInMainWorld("duckdocs", {
  invoke(command, args = {}) {
    return ipcRenderer.invoke("duckdocs:invoke", command, args);
  },
  listen(eventName, handler) {
    if (!ALLOWED_EVENTS.has(eventName)) {
      throw new Error(`DuckDocs event is not allowed: ${eventName}`);
    }
    const listener = (_event, payload) => handler({ payload });
    ipcRenderer.on(eventName, listener);
    return () => ipcRenderer.removeListener(eventName, listener);
  },
});
