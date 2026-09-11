import { mkdir, readFile, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const directory = dirname(fileURLToPath(import.meta.url));

async function saveOnce(path, value) {
  await mkdir(dirname(path), { recursive: true, mode: 0o700 });
  try {
    await writeFile(path, JSON.stringify(value, null, 2) + "\n", { flag: "wx", mode: 0o600 });
    console.log(`Created ${path}; credential values hidden.`);
  } catch (error) {
    if (error.code !== "EEXIST") throw error;
    console.log(`Kept existing ${path}.`);
  }
}

// Copy only the requested providers, never unrelated tokens, extensions, or history.
const pi = JSON.parse(await readFile(resolve(homedir(), ".pi/agent/auth.json"), "utf8"));
if (!pi["openai-codex"]) throw new Error("Log into OpenAI Codex in host pi first.");
await saveOnce(resolve(directory, "auth/pi/auth.json"), { "openai-codex": pi["openai-codex"] });
await saveOnce(resolve(directory, "auth/pi/settings.json"), { enableInstallTelemetry: false });

const claude = JSON.parse(await readFile(resolve(homedir(), ".claude/.credentials.json"), "utf8"));
if (!claude.claudeAiOauth) throw new Error("Log into Claude Code on the host first.");
await saveOnce(resolve(directory, "auth/claude/.credentials.json"), { claudeAiOauth: claude.claudeAiOauth });
await saveOnce(resolve(directory, "auth/claude/.claude.json"), { hasCompletedOnboarding: true });
console.log("Dedicated auth copies ready. Refreshes persist here; host auth files are not mounted.");
