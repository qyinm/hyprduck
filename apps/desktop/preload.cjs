const { contextBridge, ipcRenderer } = require("electron");

const ALLOWED_EVENTS = new Set([
  "etyma://snapshot",
  "etyma://agent-terminal",
  "etyma://agent-chat",
]);

contextBridge.exposeInMainWorld("etyma", {
  invoke(command, args = {}) {
    return ipcRenderer.invoke("etyma:invoke", command, args);
  },
  listen(eventName, handler) {
    if (!ALLOWED_EVENTS.has(eventName)) {
      throw new Error(`Etyma event is not allowed: ${eventName}`);
    }
    const listener = (_event, payload) => handler({ payload });
    ipcRenderer.on(eventName, listener);
    return () => ipcRenderer.removeListener(eventName, listener);
  },
});
