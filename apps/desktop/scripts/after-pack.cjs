const fs = require("node:fs");
const path = require("node:path");

exports.default = async function afterPack(context) {
  if (context.electronPlatformName !== "darwin") {
    return;
  }

  const appName = context.packager.appInfo.productFilename;
  const unpackedNodePtyDir = path.join(
    context.appOutDir,
    `${appName}.app`,
    "Contents",
    "Resources",
    "app.asar.unpacked",
    "node_modules",
    "node-pty",
    "prebuilds",
  );
  fs.rmSync(path.join(unpackedNodePtyDir, "darwin-x64"), {
    force: true,
    recursive: true,
  });

  const helperPath = path.join(
    unpackedNodePtyDir,
    "darwin-arm64",
    "spawn-helper",
  );
  if (fs.existsSync(helperPath)) {
    fs.chmodSync(helperPath, 0o755);
  }
};
