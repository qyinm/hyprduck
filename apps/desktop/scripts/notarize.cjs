const path = require("node:path");
const { execFileSync } = require("node:child_process");
const { notarize } = require("@electron/notarize");

exports.default = async function notarizeApp(context) {
  if (context.electronPlatformName !== "darwin") {
    return;
  }

  const appleId = process.env.APPLE_ID;
  const appleIdPassword = process.env.APPLE_APP_SPECIFIC_PASSWORD;
  const teamId = process.env.APPLE_TEAM_ID;
  const appName = context.packager.appInfo.productFilename;
  const appPath = path.join(context.appOutDir, `${appName}.app`);

  resignNodePtySpawnHelper(appPath);

  if (!appleId || !appleIdPassword || !teamId) {
    console.log("[notarize] Skipping notarization because APPLE_ID / APPLE_APP_SPECIFIC_PASSWORD / APPLE_TEAM_ID are not all set.");
    return;
  }

  console.log(`[notarize] Submitting ${appPath}`);
  await notarize({
    tool: "notarytool",
    appPath,
    appleId,
    appleIdPassword,
    teamId,
  });
};

function resignNodePtySpawnHelper(appPath) {
  const identity = findDeveloperIdIdentity(process.env.CSC_NAME);
  if (!identity) {
    console.log("[sign] Skipping node-pty helper re-sign because no Developer ID Application identity is available.");
    return;
  }

  const helperPath = path.join(
    appPath,
    "Contents",
    "Resources",
    "app.asar.unpacked",
    "node_modules",
    "node-pty",
    "prebuilds",
    "darwin-arm64",
    "spawn-helper",
  );
  const entitlementsPath = path.join(
    __dirname,
    "..",
    "build",
    "macos",
    "entitlements.mac.plist",
  );

  console.log(`[sign] Re-signing ${helperPath}`);
  execFileSync("codesign", [
    "--force",
    "--options",
    "runtime",
    "--sign",
    identity,
    helperPath,
  ]);

  console.log(`[sign] Re-sealing ${appPath}`);
  execFileSync("codesign", [
    "--force",
    "--deep",
    "--options",
    "runtime",
    "--entitlements",
    entitlementsPath,
    "--sign",
    identity,
    appPath,
  ]);
}

function findDeveloperIdIdentity(preferredName) {
  const identities = listCodeSigningIdentities();
  if (preferredName) {
    const exact = identities.find(
      (identity) =>
        identity === preferredName ||
        identity === `Developer ID Application: ${preferredName}`,
    );
    if (exact) {
      return exact;
    }
  }
  return identities.find((identity) =>
    identity.startsWith("Developer ID Application:"),
  );
}

function listCodeSigningIdentities() {
  try {
    return execFileSync("security", ["find-identity", "-v", "-p", "codesigning"], {
      encoding: "utf8",
    })
      .split("\n")
      .map((line) => line.match(/"([^"]+)"/)?.[1])
      .filter(Boolean);
  } catch {
    return [];
  }
}
