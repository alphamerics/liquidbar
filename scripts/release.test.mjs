import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const script = fileURLToPath(new URL("./release.mjs", import.meta.url));

function fixture(t, projectFiles = false) {
  const dir = mkdtempSync(path.join(os.tmpdir(), "glassbar-release-test-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const repo = path.join(dir, "repo");
  const remote = path.join(dir, "remote.git");
  mkdirSync(path.join(repo, "scripts"), { recursive: true });
  mkdirSync(path.join(repo, "src-tauri"));
  copyFileSync(script, path.join(repo, "scripts", "release.mjs"));
  const put = (file, value) => writeFileSync(path.join(repo, file), value);
  put("package.json", '{\n  "name": "glassbar",\n  "version": "0.1.0"\n}\n');
  put("package-lock.json", '{\n  "name": "glassbar",\n  "version": "0.1.0",\n  "packages": {\n    "": {\n      "version": "0.1.0"\n    }\n  }\n}\n');
  put("src-tauri/tauri.conf.json", '{\n  "version": "0.1.0",\n  "plugins": { "updater": { "pubkey": "test-key" } }\n}\n');
  put("src-tauri/Cargo.toml", '[package]\nname = "glassbar"\nversion = "0.1.0"\n');
  put("src-tauri/Cargo.lock", 'version = 4\n\n[[package]]\nname = "glassbar"\nversion = "0.1.0"\n');
  if (projectFiles) {
    const root = path.dirname(path.dirname(script));
    for (const file of ["package.json", "package-lock.json", "src-tauri/tauri.conf.json", "src-tauri/Cargo.toml", "src-tauri/Cargo.lock"]) {
      copyFileSync(path.join(root, file), path.join(repo, file));
    }
    const configPath = path.join(repo, "src-tauri", "tauri.conf.json");
    const config = readFileSync(configPath, "utf8").replace("REPLACE_WITH_UPDATER_PUBLIC_KEY", "test-key");
    writeFileSync(configPath, config);
  }
  const git = (...args) => execFileSync("git", args, { cwd: repo, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
  execFileSync("git", ["init", "--bare", remote], { stdio: "ignore" });
  git("init", "-b", "main");
  git("config", "user.name", "Release Test");
  git("config", "user.email", "release-test@example.invalid");
  git("add", ".");
  git("commit", "-m", "Initial");
  git("remote", "add", "origin", remote);
  git("push", "-u", "origin", "main");
  const run = (...args) => spawnSync(process.execPath, ["scripts/release.mjs", ...args], { cwd: repo, encoding: "utf8" });
  return { repo, remote, git, put, run };
}

test("releases real project version files with matching commits and remote tags", (t) => {
  const f = fixture(t, true);
  const [major, minor, patch] = JSON.parse(readFileSync(path.join(f.repo, "package.json"))).version.split(".").map(Number);
  for (const [args, expected] of [[[], `${major}.${minor}.${patch + 1}`], [["minor"], `${major}.${minor + 1}.0`]]) {
    const result = f.run(...args);
    assert.equal(result.status, 0, result.stderr);
    const pkg = JSON.parse(readFileSync(path.join(f.repo, "package.json")));
    const lock = JSON.parse(readFileSync(path.join(f.repo, "package-lock.json")));
    const config = JSON.parse(readFileSync(path.join(f.repo, "src-tauri", "tauri.conf.json")));
    assert.equal(pkg.version, expected);
    assert.equal(lock.version, expected);
    assert.equal(lock.packages[""].version, expected);
    assert.equal(config.version, expected);
    assert.match(readFileSync(path.join(f.repo, "src-tauri", "Cargo.toml"), "utf8"), new RegExp(`version = "${expected.replaceAll(".", "\\.")}"`));
    assert.match(readFileSync(path.join(f.repo, "src-tauri", "Cargo.lock"), "utf8"), new RegExp(`name = "glassbar"\\r?\\nversion = "${expected.replaceAll(".", "\\.")}"`));
    assert.equal(f.git("status", "--porcelain"), "");
    assert.equal(f.git("rev-parse", `v${expected}^{}`), f.git("rev-parse", "HEAD"));
    assert.equal(f.git("ls-remote", "--refs", "origin", `refs/tags/v${expected}`).split("\t")[1], `refs/tags/v${expected}`);
    assert.equal(f.run("--verify-tag", `v${expected}`).status, 0);
  }
});

test("rejects a branch with local commits not present on its remote", (t) => {
  const f = fixture(t);
  f.put("extra.txt", "local commit");
  f.git("add", "extra.txt");
  f.git("commit", "-m", "Unpushed work");
  const result = f.run();
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /remote branch does not match HEAD/);
  assert.equal(JSON.parse(readFileSync(path.join(f.repo, "package.json"))).version, "0.1.0");
  assert.equal(f.git("tag", "--list"), "");
});

test("rejects dirty trees without changing the version or creating a tag", (t) => {
  const f = fixture(t);
  f.put("untracked.txt", "work");
  const result = f.run();
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /not clean/);
  assert.equal(JSON.parse(readFileSync(path.join(f.repo, "package.json"))).version, "0.1.0");
  assert.equal(f.git("tag", "--list"), "");
});

test("rejects missing updater keys, mismatched versions, and nonmatching tags", (t) => {
  const f = fixture(t);
  f.put("src-tauri/tauri.conf.json", '{\n  "version": "0.1.0",\n  "plugins": { "updater": { "pubkey": "REPLACE_WITH_UPDATER_PUBLIC_KEY" } }\n}\n');
  assert.match(f.run("--verify-tag", "v0.1.0").stderr, /Configure the updater public key/);
  f.put("src-tauri/tauri.conf.json", '{\n  "version": "0.1.1",\n  "plugins": { "updater": { "pubkey": "test-key" } }\n}\n');
  assert.match(f.run("--verify-tag", "v0.1.0").stderr, /Version mismatch/);
  f.put("src-tauri/tauri.conf.json", '{\n  "version": "0.1.0",\n  "plugins": { "updater": { "pubkey": "test-key" } }\n}\n');
  assert.match(f.run("--verify-tag", "v0.1.1").stderr, /does not match/);
});

test("rejects an existing remote release tag before changing files", (t) => {
  const f = fixture(t);
  f.git("tag", "v0.1.1");
  f.git("push", "origin", "refs/tags/v0.1.1");
  f.git("tag", "-d", "v0.1.1");
  const result = f.run();
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /already exists on origin/);
  assert.equal(JSON.parse(readFileSync(path.join(f.repo, "package.json"))).version, "0.1.0");
});
