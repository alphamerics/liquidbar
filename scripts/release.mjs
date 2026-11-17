import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const files = [
  "package.json",
  "package-lock.json",
  "src-tauri/tauri.conf.json",
  "src-tauri/Cargo.toml",
  "src-tauri/Cargo.lock",
];

function git(...args) {
  return execFileSync("git", args, { cwd: root, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
}

function read(relativePath) {
  return readFileSync(path.join(root, relativePath), "utf8");
}

function replaceOnce(text, regex, version, file) {
  let matches = 0;
  const result = text.replace(regex, (_, before, oldVersion, after) => {
    matches += 1;
    return `${before}${version}${after}`;
  });
  if (matches !== 1) throw new Error(`Could not find exactly one version in ${file}.`);
  return result;
}

function currentVersions() {
  const texts = Object.fromEntries(files.map((file) => [file, read(file)]));
  const pkg = JSON.parse(texts["package.json"]);
  const lock = JSON.parse(texts["package-lock.json"]);
  const config = JSON.parse(texts["src-tauri/tauri.conf.json"]);
  const cargo = texts["src-tauri/Cargo.toml"].match(/^\[package\]\r?\nname = "glassbar"\r?\nversion = "([^"]+)"/m);
  const cargoLock = texts["src-tauri/Cargo.lock"].match(/^\[\[package\]\]\r?\nname = "glassbar"\r?\nversion = "([^"]+)"/m);
  const version = pkg.version;
  if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version ?? "")) {
    throw new Error("The current package version must be a stable major.minor.patch version.");
  }
  if ([lock.version, lock.packages?.[""]?.version, config.version, cargo?.[1], cargoLock?.[1]].some((value) => value !== version)) {
    throw new Error("Version mismatch between package.json, package-lock.json, Tauri config, Cargo.toml, and Cargo.lock.");
  }
  if (version.split(".").some((part) => Number(part) > 65535)) {
    throw new Error("Each version component must fit the Windows identity package limit (65535).");
  }
  if (typeof config.plugins?.updater?.pubkey !== "string" ||
      !config.plugins.updater.pubkey.trim() ||
      config.plugins.updater.pubkey.includes("REPLACE_WITH")) {
    throw new Error("Configure the updater public key in src-tauri/tauri.conf.json before releasing.");
  }
  return { version, texts };
}

function versionFiles(texts, next) {
  return {
    "package.json": replaceOnce(texts["package.json"], /^(  "version": ")([^"]+)(")/m, next, "package.json"),
    "package-lock.json": replaceOnce(
      replaceOnce(texts["package-lock.json"], /^(  "version": ")([^"]+)(")/m, next, "package-lock.json"),
      /("packages":\s*\{\s*"":[\s\S]*?"version": ")([^"]+)(")/, next, "package-lock.json root package",
    ),
    "src-tauri/tauri.conf.json": replaceOnce(texts["src-tauri/tauri.conf.json"], /^(  "version": ")([^"]+)(")/m, next, "tauri.conf.json"),
    "src-tauri/Cargo.toml": replaceOnce(texts["src-tauri/Cargo.toml"], /(^\[package\]\r?\nname = "glassbar"\r?\nversion = ")([^"]+)(")/m, next, "Cargo.toml"),
    "src-tauri/Cargo.lock": replaceOnce(texts["src-tauri/Cargo.lock"], /(^\[\[package\]\]\r?\nname = "glassbar"\r?\nversion = ")([^"]+)(")/m, next, "Cargo.lock"),
  };
}

function nextVersion(current, increment) {
  const parts = current.split(".").map(Number);
  const index = { major: 0, minor: 1, patch: 2 }[increment];
  if (index === undefined) throw new Error("Usage: npm run release -- [patch|minor|major]");
  parts[index] += 1;
  for (let i = index + 1; i < parts.length; i += 1) parts[i] = 0;
  if (parts.some((part) => part > 65535)) throw new Error("Next version exceeds the Windows identity package limit.");
  return parts.join(".");
}

function release(increment) {
  if (git("status", "--porcelain", "--untracked-files=normal")) {
    throw new Error("Working tree is not clean. Commit or stash all changes before releasing.");
  }
  const branch = git("branch", "--show-current");
  if (!branch) throw new Error("Release from a branch, not a detached HEAD.");
  const remote = git("config", "--get", `branch.${branch}.remote`);
  const remoteBranch = git("config", "--get", `branch.${branch}.merge`);
  if (!remote || remote === "." || !remoteBranch.startsWith("refs/heads/")) {
    throw new Error("The current branch must track a remote branch.");
  }
  const { version, texts } = currentVersions();
  const next = nextVersion(version, increment);
  const tag = `v${next}`;
  const updated = versionFiles(texts, next);
  if (git("tag", "--list", tag)) throw new Error(`Tag ${tag} already exists locally.`);
  const refs = git("ls-remote", "--refs", remote, remoteBranch, `refs/tags/${tag}`).split("\n").filter(Boolean);
  const remoteHead = refs.find((line) => line.endsWith(`\t${remoteBranch}`))?.split("\t")[0];
  if (remoteHead !== git("rev-parse", "HEAD")) {
    throw new Error("The remote branch does not match HEAD. Synchronize and push your branch before releasing.");
  }
  if (refs.some((line) => line.endsWith(`\trefs/tags/${tag}`))) {
    throw new Error(`Tag ${tag} already exists on ${remote}.`);
  }
  for (const [file, contents] of Object.entries(updated)) writeFileSync(path.join(root, file), contents);
  console.log(`Bumping ${version} to ${next}.`);
  git("add", "--", ...files);
  git("commit", "-m", `chore: release ${tag}`, "-m", "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>");
  git("tag", "-a", tag, "-m", `Release ${tag}`);
  console.log(`Pushing ${branch} and ${tag} to ${remote} atomically.`);
  git("push", "--atomic", remote, `HEAD:${remoteBranch}`, `refs/tags/${tag}:refs/tags/${tag}`);
  console.log(`Release ${tag} pushed. Check the Release workflow in GitHub Actions.`);
}

try {
  const args = process.argv.slice(2);
  if (args[0] === "--verify-tag" && args.length === 2) {
    const { version } = currentVersions();
    if (args[1] !== `v${version}`) throw new Error(`Tag ${args[1]} does not match version ${version}.`);
    console.log(`Verified release ${args[1]}.`);
  } else {
    if (args.length > 1 || (args.length === 1 && !["patch", "minor", "major"].includes(args[0]))) {
      throw new Error("Usage: npm run release -- [patch|minor|major]");
    }
    release(args[0] ?? "patch");
  }
} catch (error) {
  const detail = error.stderr?.toString().trim() || error.message;
  console.error(`Release failed: ${detail}`);
  console.error("If versioning or pushing began, inspect git status, the release commit, and the tag before retrying.");
  process.exitCode = 1;
}
