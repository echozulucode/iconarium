#!/usr/bin/env node
/**
 * bump-version.mjs — one version, three files.
 *
 * Iconarium's version lives in three places that must agree:
 *
 *   1. package.json                  "version"
 *   2. Cargo.toml                    [workspace.package] version  (svg-core and the app inherit it)
 *   3. src-tauri/tauri.conf.json     "version"
 *
 * #3 matters most at runtime: it is what `getVersion()` reports and what the updater compares
 * against `latest.json`. If it drifts, the app either nags forever or never sees an update. So
 * `just bump` is the sanctioned way to change any of them, and the release workflow re-checks all
 * three against the tag before it builds.
 *
 *   just bump 0.2.0          # explicit version
 *   just bump patch|minor|major
 *   just bump patch --tag    # also commit the three files and create an annotated tag
 *   just check-version 0.2.0 # verify only (what CI runs)
 *
 * `--tag` deliberately stops short of pushing: the tag push starts the release, and a mistyped
 * version should be fixable with `git tag -d` rather than by publishing something.
 */

import { readFile, writeFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");

// Surgical regexes instead of parse-and-reserialize, so the diff is the one changed line.
const FILES = [
  { path: "package.json", pattern: /("version"\s*:\s*)"([^"]+)"/ },
  { path: "Cargo.toml", pattern: /(\[workspace\.package\][\s\S]*?\nversion\s*=\s*)"([^"]+)"/ },
  { path: "src-tauri/tauri.conf.json", pattern: /("version"\s*:\s*)"([^"]+)"/ },
];

const SEMVER_RE = /^(\d+)\.(\d+)\.(\d+)(?:-([\w.-]+))?$/;

function parseSemver(value) {
  const m = SEMVER_RE.exec(value);
  return m ? { major: +m[1], minor: +m[2], patch: +m[3], pre: m[4] ?? null } : null;
}

function bumpSemver(current, kind) {
  const v = parseSemver(current);
  if (!v) throw new Error(`current version "${current}" is not semver`);
  if (kind === "patch") return `${v.major}.${v.minor}.${v.patch + 1}`;
  if (kind === "minor") return `${v.major}.${v.minor + 1}.0`;
  if (kind === "major") return `${v.major + 1}.0.0`;
  throw new Error(`unknown bump kind: ${kind}`);
}

async function readVersions() {
  const out = [];
  for (const { path, pattern } of FILES) {
    const raw = await readFile(join(repoRoot, path), "utf8");
    const m = pattern.exec(raw);
    if (!m) throw new Error(`no version found in ${path} — its structure may have changed`);
    out.push({ path, version: m[2] });
  }
  return out;
}

async function writeVersion(file, version) {
  const abs = join(repoRoot, file.path);
  const raw = await readFile(abs, "utf8");
  const updated = raw.replace(file.pattern, `$1"${version}"`);
  if (updated === raw && !raw.match(file.pattern)) throw new Error(`no version found in ${file.path}`);
  await writeFile(abs, updated);
}

function git(args) {
  const r = spawnSync("git", args, { cwd: repoRoot, stdio: "inherit", encoding: "utf8" });
  if (r.status !== 0) throw new Error(`git ${args.join(" ")} failed`);
}

async function check(expected) {
  const wanted = expected.replace(/^v/, "");
  if (!parseSemver(wanted)) {
    console.error(`--check needs a semver version, got "${expected}"`);
    return 1;
  }
  const versions = await readVersions();
  for (const { path, version } of versions) console.log(`  ${version === wanted ? "✓" : "✗"} ${path} — ${version}`);
  if (versions.some((v) => v.version !== wanted)) {
    console.error(`\nExpected every file to declare ${wanted}. Run \`just bump ${wanted}\` and commit the result.`);
    return 1;
  }
  console.log(`\nAll three files declare ${wanted}.`);
  return 0;
}

async function main() {
  const args = process.argv.slice(2);
  if (args.length === 0) {
    console.error("Usage: just bump <version | patch | minor | major> [--tag]\n       just check-version <version>");
    return 1;
  }
  if (args[0] === "--check") {
    if (!args[1]) return console.error("Usage: just check-version <version>"), 1;
    return check(args[1]);
  }
  const request = args[0];
  const shouldTag = args.includes("--tag");

  const versions = await readVersions();
  const distinct = [...new Set(versions.map((v) => v.version))];
  if (distinct.length > 1) {
    console.error("The three version files disagree before bumping:");
    for (const { path, version } of versions) console.error(`  ${path} — ${version}`);
    console.error("\nSet them to one version explicitly: just bump <version>");
    if (!parseSemver(request.replace(/^v/, ""))) return 1;
  }
  const current = versions[0].version;

  let next;
  if (["patch", "minor", "major"].includes(request)) next = bumpSemver(current, request);
  else if (parseSemver(request.replace(/^v/, ""))) next = request.replace(/^v/, "");
  else {
    console.error(`Invalid version "${request}". Expected semver (e.g. 0.2.0), or patch/minor/major.`);
    return 1;
  }
  if (next === current && distinct.length === 1) {
    console.error(`Already at ${next}. Nothing to do.`);
    return 1;
  }

  console.log(`Bumping ${current} → ${next}`);
  for (const f of FILES) {
    await writeVersion(f, next);
    console.log(`  ✓ ${f.path}`);
  }
  // Keep the lockfiles' own entries in step so `npm ci` and the release build don't dirty the tree.
  const win = process.platform === "win32";
  spawnSync("cargo", ["update", "--workspace", "--offline"], { cwd: repoRoot, stdio: "ignore", shell: win });
  spawnSync(win ? "npm.cmd" : "npm", ["install", "--package-lock-only", "--no-audit", "--no-fund"], {
    cwd: repoRoot,
    stdio: "ignore",
    shell: win,
  });

  if (shouldTag) {
    git(["add", ...FILES.map((f) => f.path), "Cargo.lock", "package-lock.json"]);
    git(["commit", "-m", `chore: bump to v${next}`]);
    git(["tag", "-a", `v${next}`, "-m", `v${next}`]);
    console.log(`\nTagged v${next}. Nothing is pushed yet — review, then:\n  git push && git push origin v${next}`);
  } else {
    console.log("\nDone. Review with: git diff, then:");
    console.log(`  git commit -am "chore: bump to v${next}" && git tag -a v${next} -m "v${next}"`);
    console.log(`  git push && git push origin v${next}`);
  }
  return 0;
}

main().then(
  (code) => process.exit(code),
  (err) => {
    console.error(err.stack ?? err.message ?? err);
    process.exit(1);
  },
);
