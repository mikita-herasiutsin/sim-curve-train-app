import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { posix, resolve } from "node:path";
import process from "node:process";
import { pathToFileURL } from "node:url";

const REPO_URL =
  /^https:\/\/github\.com\/mikita-herasiutsin\/sim-curve-train-app\/(?:blob|tree)\/main\/(.+)$/;
const URI_SCHEME = /^[a-z][a-z0-9+.-]*:/i;
const HEADING = /^[ ]{0,3}#{1,6}\s+(.*?)(?:\s+#+)?\s*$/;
const ENV_VAR = /\bSCT_[A-Z][A-Z0-9_]*\b/g;
const NPM_RUN = /\bnpm\s+run\s+([A-Za-z0-9_:-]+)/g;

function git(args, cwd) {
  return execFileSync("git", args, { encoding: "utf8", cwd });
}

function normalize(text) {
  return text.replace(/\r\n/g, "\n");
}

function blank(line) {
  return " ".repeat(line.length);
}

// GitHub heading anchor; counts (a Map) numbers repeated headings in one file.
export function githubSlug(text, counts = new Map()) {
  const base = text
    .toLowerCase()
    .trim()
    .replace(/[^\p{L}0-9\s_-]/gu, "")
    .replace(/\s/g, "-");
  const seen = counts.get(base) ?? 0;
  counts.set(base, seen + 1);
  return seen === 0 ? base : `${base}-${seen}`;
}

// Blanks fenced code blocks with spaces, keeping every line number.
function maskFences(text) {
  const out = [];
  let fence = null;

  for (const line of text.split("\n")) {
    if (!fence) {
      const match = line.match(/^[ ]{0,3}(`{3,}|~{3,})/);
      if (match) {
        fence = { char: match[1][0], length: match[1].length };
        out.push(blank(line));
        continue;
      }
      out.push(line);
    } else {
      const match = line.match(/^[ ]{0,3}(`{3,}|~{3,})\s*$/);
      if (match && match[1][0] === fence.char && match[1].length >= fence.length) {
        fence = null;
      }
      out.push(blank(line));
    }
  }

  return out.join("\n");
}

// Blanks inline code spans. A span cannot cross a blank line, so each paragraph is masked alone.
function maskInlineCode(text) {
  // The capture group keeps the blank-line separators at odd indexes.
  return text
    .split(/(\n[ \t]*\n)/)
    .map((part, i) =>
      i % 2 === 0 ? part.replace(/(`+)([\s\S]*?)\1/g, (m) => m.replace(/[^\n]/g, " ")) : part,
    )
    .join("");
}

function maskCode(text) {
  return maskInlineCode(maskFences(text));
}

// Counts lines from start of string to index.
function lineAt(text, index) {
  let line = 1;
  for (let i = 0; i < index; i++) {
    if (text[i] === "\n") line++;
  }
  return line;
}

// Returns the link target from the text after "(" or a reference colon.
function destination(raw) {
  if (raw.startsWith("<")) {
    const end = raw.indexOf(">");
    return end > 0 ? raw.slice(1, end) : null;
  }
  return raw.split(/\s+/)[0] || null;
}

// Extracts inline, nested and reference links outside code blocks and spans.
export function extractLinks(text) {
  const clean = maskCode(normalize(text));
  const links = [];

  clean.split("\n").forEach((line, i) => {
    const ref = line.match(/^[ ]{0,3}\[([^\]]+)\]:\s*(<[^>]*>|\S+)/);
    const target = ref && destination(ref[2]);
    if (target) links.push({ text: ref[1], target, line: i + 1 });
  });

  for (let i = 0; i < clean.length; i++) {
    if (clean[i] !== "[") continue;
    let depth = 1;
    let j = i + 1;
    while (j < clean.length && depth > 0) {
      if (clean[j] === "[") depth++;
      else if (clean[j] === "]") depth--;
      j++;
    }
    if (depth !== 0 || clean[j] !== "(") continue;

    let parenDepth = 1;
    let k = j + 1;
    while (k < clean.length && parenDepth > 0) {
      if (clean[k] === "(") parenDepth++;
      else if (clean[k] === ")") parenDepth--;
      k++;
    }
    if (parenDepth !== 0) continue;

    const target = destination(clean.slice(j + 1, k - 1).trim());
    if (target) {
      links.push({ text: clean.slice(i + 1, j - 1), target, line: lineAt(clean, i) });
    }
    // Keep scanning inside the link text, so an image in [![alt](img.png)](doc.md) is found too.
  }

  return links;
}

const AI_WORDS = [
  "additionally",
  "crucial",
  "delve",
  "delves",
  "delving",
  "enduring",
  "enhance",
  "enhances",
  "enhanced",
  "fostering",
  "garner",
  "interplay",
  "intricate",
  "pivotal",
  "showcase",
  "showcases",
  "showcased",
  "tapestry",
  "testament",
  "underscore",
  "underscores",
  "underscored",
  "vibrant",
  "utilize",
  "utilizes",
  "utilise",
  "utilized",
  "utilizing",
  "leverage",
  "leverages",
  "leveraging",
  "leveraged",
  "facilitate",
  "facilitates",
  "facilitated",
  "facilitating",
  "seamless",
  "seamlessly",
  "boast",
  "boasts",
  "boasted",
];

const FILLER_PHRASES = [
  "in order to",
  "due to the fact",
  "it is important to note",
  "serves as",
  "stands as",
];

const PROSE_RULES = [
  { rule: "em-dash", regex: /—/g },
  { rule: "en-dash", regex: /–/g },
  { rule: "curly-quote", regex: /[‘’“”]/g },
  {
    rule: "ai-vocabulary",
    regex: new RegExp(`\\b(?:${AI_WORDS.join("|")})\\b`, "gi"),
  },
  {
    rule: "filler",
    regex: new RegExp(
      `\\b(?:${FILLER_PHRASES.map((p) => p.replace(/\s+/g, "\\s+")).join("|")})\\b`,
      "gi",
    ),
  },
];

// Finds prose rule violations in text, skipping fences, inline code and prose-ok.
export function proseIssues(text) {
  const raw = normalize(text);
  const masked = maskCode(raw);
  const originalLines = raw.split("\n");
  const maskedLines = masked.split("\n");
  const issues = [];

  for (let idx = 0; idx < originalLines.length; idx++) {
    const originalLine = originalLines[idx];
    const maskedLine = maskedLines[idx];
    if (originalLine.includes("prose-ok")) continue;
    if (maskedLine.trim() === "") continue;

    const lineIssues = [];
    for (const { rule, regex } of PROSE_RULES) {
      regex.lastIndex = 0;
      let match;
      while ((match = regex.exec(maskedLine)) !== null) {
        const matchStart = match.index;
        const matchEnd = match.index + match[0].length;
        const start = Math.max(0, matchStart - 30);
        const end = Math.min(originalLine.length, matchEnd + 30);
        lineIssues.push({
          line: idx + 1,
          rule,
          excerpt: originalLine.slice(start, end),
          index: matchStart,
        });
      }
    }
    lineIssues.sort((a, b) => a.index - b.index);
    for (const issue of lineIssues) {
      issues.push({ line: issue.line, rule: issue.rule, excerpt: issue.excerpt });
    }
  }

  return issues;
}

// Keeps the comment lines of a source file and blanks the code lines.
// A "/* */" block is tracked across lines, so a continuation line counts as a comment.
function commentLines(text) {
  let inBlock = false;
  return normalize(text)
    .split("\n")
    .map((line) => {
      const trimmed = line.trimStart();
      if (inBlock) {
        if (line.includes("*/")) inBlock = false;
        return line;
      }
      if (trimmed.startsWith("//") || trimmed.startsWith("<!--")) return line;
      if (trimmed.startsWith("/*")) {
        inBlock = !trimmed.includes("*/", 2);
        return line;
      }
      return "";
    });
}

// Returns the reason from a "Docs: none, <your reason>" line in the PR body, or null.
export function parseOptOut(body = process.env.PR_BODY) {
  const match = body?.match(/^[ \t]*Docs:[ \t]*none\b[ \t,:;.-]*([^\r\n]+)$/im);
  const reason = match?.[1].trim() ?? "";
  if (reason.length < 10 || reason.includes("<reason>") || reason === "REASON") return null;
  return reason;
}

// Reads the "scripts" block of package.json at a git revision, with keys sorted.
function scriptsAt(rev, repoRoot) {
  const scripts = JSON.parse(git(["show", `${rev}:package.json`], repoRoot)).scripts ?? {};
  return JSON.stringify(
    Object.keys(scripts)
      .sort()
      .map((key) => [key, scripts[key]]),
  );
}

// True when the "scripts" block of package.json differs between the merge base and HEAD.
function packageScriptsDiffer(ctx) {
  if (!ctx?.baseRef) return false;
  try {
    const base = git(["merge-base", ctx.baseRef, "HEAD"], ctx.repoRoot).trim();
    return scriptsAt(base, ctx.repoRoot) !== scriptsAt("HEAD", ctx.repoRoot);
  } catch (error) {
    throw new Error(`cannot compare package.json scripts with ${ctx.baseRef}: ${error.message}`, {
      cause: error,
    });
  }
}

// Code paths and the docs a pull request must change with them. "any" means any *.md file.
export const RULES = [
  {
    name: "preset format",
    match: (f) => f === "crates/core/src/preset.rs" || f.startsWith("presets/"),
    docs: ["docs/preset-format.md", "README.md"],
  },
  {
    name: "extractor",
    match: (f) => f.startsWith("crates/telemetry-extract/") && !f.endsWith(".md"),
    docs: ["crates/telemetry-extract/README.md"],
  },
  {
    name: "release",
    match: (f) => f === ".github/workflows/release.yml" || f === "src-tauri/tauri.conf.json",
    docs: ["docs/releasing.md"],
  },
  {
    name: "smoke",
    match: (f) => f === "scripts/smoke.mjs",
    docs: ["README.md"],
  },
  {
    name: "quality gates",
    match: (f) =>
      f === "quality.config.json" ||
      f === "scripts/quality.mjs" ||
      f === "stryker.config.mjs" ||
      f === ".cargo/mutants.toml" ||
      f.startsWith("tools/complexity/"),
    docs: ["CONTRIBUTING.md"],
  },
  {
    name: "npm scripts",
    match: (f, ctx) => f === "package.json" && packageScriptsDiffer(ctx),
    docs: ["README.md", "CONTRIBUTING.md"],
  },
  {
    name: "app code",
    match: (f) =>
      f.startsWith("src/") || f.startsWith("src-tauri/src/") || /^crates\/[^/]+\/src\//.test(f),
    docs: "any",
  },
];

// Returns the rules whose code changed without any of their docs.
export function impactFailures(changedFiles, ctx) {
  const failures = [];
  for (const rule of RULES) {
    const file = changedFiles.find((f) => rule.match(f, ctx));
    if (!file) continue;
    const satisfied =
      rule.docs === "any"
        ? changedFiles.some((f) => f.endsWith(".md"))
        : rule.docs.some((doc) => changedFiles.includes(doc));
    if (!satisfied) failures.push({ name: rule.name, file, docs: rule.docs });
  }
  return failures;
}

// Heading slugs of a Markdown text, outside fenced code blocks.
function headingSlugs(text) {
  const slugs = new Set();
  const counts = new Map();
  for (const line of maskFences(text).split("\n")) {
    const match = line.match(HEADING);
    if (!match) continue;
    const plain = match[1]
      .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
      .replace(/\*/g, "")
      .replace(/(^|[^\p{L}\p{N}])_+/gu, "$1")
      .replace(/_+(?=[^\p{L}\p{N}]|$)/gu, "");
    slugs.add(githubSlug(plain, counts));
  }
  return slugs;
}

function safeDecode(text, decode) {
  try {
    return decode(text);
  } catch {
    return text;
  }
}

// Returns the problem with a link from docFile, or null when it resolves.
function linkProblem(docFile, target, { exists, headingsOf }) {
  let path = target;
  let repoRelative = false;
  const repo = path.match(REPO_URL);
  if (repo) {
    path = repo[1];
    repoRelative = true;
  } else if (URI_SCHEME.test(path)) {
    return null;
  }

  const hash = path.indexOf("#");
  const anchor = hash >= 0 ? path.slice(hash + 1) : null;
  const beforeHash = hash >= 0 ? path.slice(0, hash) : path;
  const query = beforeHash.indexOf("?");
  const pathPart = safeDecode(query >= 0 ? beforeHash.slice(0, query) : beforeHash, decodeURI);

  let resolved;
  if (pathPart === "") resolved = docFile;
  else if (repoRelative) resolved = posix.normalize(pathPart);
  else resolved = posix.normalize(posix.join(posix.dirname(docFile), pathPart));

  if (!exists(resolved)) return `target does not exist: ${target}`;

  if (anchor !== null && (pathPart === "" || resolved.endsWith(".md"))) {
    const slugs = headingsOf(resolved);
    if (!slugs.has(anchor) && !slugs.has(safeDecode(anchor, decodeURIComponent))) {
      return `anchor #${anchor} not found in ${resolved}`;
    }
  }
  return null;
}

// Collects the first location of each SCT_* variable in the given files.
function envVars(files, text) {
  const found = new Map();
  for (const file of files) {
    text(file)
      .split("\n")
      .forEach((line, i) => {
        for (const match of line.matchAll(ENV_VAR)) {
          if (!found.has(match[0])) found.set(match[0], { file, line: i + 1 });
        }
      });
  }
  return found;
}

// Runs the static checks on the given files and returns the problems, sorted by file and line.
// Each file is read once. read(path) returns text, exists(path) returns a boolean, and scripts
// is the Set of npm script names in package.json.
export function checkFiles({ docs, prose, code, read, exists, scripts }) {
  const problems = [];
  const cache = new Map();
  const text = (path) => {
    if (!cache.has(path)) cache.set(path, normalize(read(path)));
    return cache.get(path);
  };
  const headings = new Map();
  const headingsOf = (path) => {
    if (!headings.has(path)) headings.set(path, headingSlugs(text(path)));
    return headings.get(path);
  };
  const add = (file, line, rule, message) => problems.push({ file, line, rule, message });

  // Links, then npm scripts.
  for (const docFile of docs) {
    for (const link of extractLinks(text(docFile))) {
      const message = linkProblem(docFile, link.target, { exists, headingsOf });
      if (message) add(docFile, link.line, "link", message);
    }

    text(docFile)
      .split("\n")
      .forEach((line, i) => {
        for (const match of line.matchAll(NPM_RUN)) {
          if (!scripts.has(match[1])) {
            add(docFile, i + 1, "npm-script", `unknown script "${match[1]}"`);
          }
        }
      });
  }

  // SCT_* variables that appear on only one side.
  const docVars = envVars(docs, text);
  const codeVars = envVars(code, text);
  for (const [token, loc] of docVars) {
    if (!codeVars.has(token)) {
      add(loc.file, loc.line, "env-var", `${token} appears in docs but not in code set`);
    }
  }
  for (const [token, loc] of codeVars) {
    if (!docVars.has(token)) {
      add(loc.file, loc.line, "env-var", `${token} appears in code set but not in docs`);
    }
  }

  // Prose rules: whole Markdown files, then the comment lines of code files.
  for (const file of prose) {
    for (const issue of proseIssues(text(file))) {
      add(file, issue.line, issue.rule, `"${issue.excerpt}"`);
    }
  }
  for (const file of code) {
    for (const issue of proseIssues(commentLines(text(file)).join("\n"))) {
      add(file, issue.line, issue.rule, `"${issue.excerpt}"`);
    }
  }

  return problems.sort((a, b) => {
    if (a.file !== b.file) return a.file.localeCompare(b.file);
    return a.line - b.line;
  });
}

// Runs impact check against base ref.
function runImpact(baseRef, repoRoot) {
  const reason = parseOptOut();
  if (reason) {
    console.log(`docs impact: skipped by the PR body: ${reason}`);
    process.exit(0);
  }

  let changedFiles;
  try {
    changedFiles = git(["diff", "--name-only", `${baseRef}...HEAD`], repoRoot)
      .split(/\r?\n/)
      .map((s) => s.trim())
      .filter(Boolean);
  } catch (error) {
    console.error(`docs impact: git diff failed: ${error.message}`);
    process.exit(1);
  }

  let failures;
  try {
    failures = impactFailures(changedFiles, { baseRef, repoRoot });
  } catch (error) {
    console.error(`docs impact: ${error.message}`);
    process.exit(1);
  }

  if (failures.length === 0) {
    console.log("docs impact: OK");
    process.exit(0);
  }

  for (const f of failures) {
    const docsLabel = Array.isArray(f.docs) ? f.docs.join(" or ") : f.docs;
    console.log(`docs impact: ${f.name}: changed ${f.file} but none of ${docsLabel}`);
  }
  console.log('Update the docs in this PR, or add "Docs: none, <your reason>" to the PR body.');
  process.exit(1);
}

// The source files the static checks scan.
const CODE_FILE = /\.(rs|ts|svelte|js|mjs)$/;
const CODE_DIR = /^(src\/|src-tauri\/src\/|crates\/|scripts\/|tools\/)/;

// Gathers the tracked and new files, runs the static checks and prints the result.
// quality-ignore: reads git and the disk; CI runs it on every pull request through npm run docs:check.
function runStatic(repoRoot) {
  const read = (path) => readFileSync(resolve(repoRoot, path), "utf8");
  const exists = (path) => existsSync(resolve(repoRoot, path));

  const tracked = git(["ls-files", "--cached", "--others", "--exclude-standard"], repoRoot)
    .split(/\r?\n/)
    .map((s) => s.trim())
    .filter((f) => f && exists(f));

  const docs = tracked.filter((f) => f.endsWith(".md"));
  const prose = docs.filter(
    (f) => !f.startsWith("docs/investigations/") && f !== "CODE_OF_CONDUCT.md",
  );
  const code = tracked.filter((f) => CODE_FILE.test(f) && CODE_DIR.test(f));
  const scripts = new Set(Object.keys(JSON.parse(read("package.json")).scripts ?? {}));

  const problems = checkFiles({ docs, prose, code, read, exists, scripts });

  if (problems.length === 0) {
    console.log("docs:check: OK");
    process.exit(0);
  }

  for (const p of problems) {
    console.log(`${p.file}:${p.line}: ${p.rule}: ${p.message}`);
  }
  console.log(`docs:check: ${problems.length} problems`);
  process.exit(1);
}

// Runs checker in static or impact mode.
function main() {
  const repoRoot = resolve(git(["rev-parse", "--show-toplevel"]).trim());

  const impactIdx = process.argv.indexOf("--impact");
  if (impactIdx >= 0) {
    const baseRef = process.argv[impactIdx + 1];
    if (!baseRef) {
      console.error("usage: check-docs.mjs --impact <base-ref>");
      process.exit(1);
    }
    runImpact(baseRef, repoRoot);
  } else {
    runStatic(repoRoot);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
