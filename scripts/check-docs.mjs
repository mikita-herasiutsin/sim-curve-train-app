import { execSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, relative, resolve } from "node:path";
import process from "node:process";
import { pathToFileURL } from "node:url";

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

// Masks fenced code blocks and inline code spans with spaces.
function maskCode(text) {
  const lines = text.split("\n");
  const out = [];
  let inFence = false;
  let fenceChar = "";
  let fenceLen = 0;

  for (const line of lines) {
    if (!inFence) {
      const match = line.match(/^[ ]{0,3}(`{3,}|~{3,})/);
      if (match) {
        inFence = true;
        fenceChar = match[1][0];
        fenceLen = match[1].length;
        out.push(" ".repeat(line.length));
        continue;
      }
      out.push(line);
    } else {
      const match = line.match(/^[ ]{0,3}(`{3,}|~{3,})\s*$/);
      if (match && match[1][0] === fenceChar && match[1].length >= fenceLen) {
        inFence = false;
      }
      out.push(" ".repeat(line.length));
    }
  }

  return out.join("\n").replace(/(`+)([\s\S]*?)\1/g, (m) => m.replace(/[^\n]/g, " "));
}

// Counts lines from start of string to index.
function lineAt(text, index) {
  let line = 1;
  for (let i = 0; i < index; i++) {
    if (text[i] === "\n") line++;
  }
  return line;
}

// Extracts inline and reference links outside code blocks and spans.
export function extractLinks(text) {
  const clean = maskCode(text.replace(/\r\n/g, "\n"));
  const links = [];
  const lines = clean.split("\n");

  for (let i = 0; i < lines.length; i++) {
    const refMatch = lines[i].match(/^[ ]{0,3}\[([^\]]+)\]:\s*<?([^\s>]+)>?/);
    if (refMatch) {
      links.push({
        text: refMatch[1],
        target: refMatch[2],
        line: i + 1,
      });
    }
  }

  let i = 0;
  while (i < clean.length) {
    if (clean[i] === "[") {
      let depth = 1;
      let j = i + 1;
      while (j < clean.length && depth > 0) {
        if (clean[j] === "[") depth++;
        else if (clean[j] === "]") depth--;
        j++;
      }
      if (depth === 0 && j < clean.length && clean[j] === "(") {
        let parenDepth = 1;
        let k = j + 1;
        while (k < clean.length && parenDepth > 0) {
          if (clean[k] === "(") parenDepth++;
          else if (clean[k] === ")") parenDepth--;
          k++;
        }
        if (parenDepth === 0) {
          const rawDest = clean.slice(j + 1, k - 1).trim();
          const target = rawDest.replace(/^<|>$/g, "").split(/\s+/)[0];
          if (target) {
            links.push({
              text: clean.slice(i + 1, j - 1),
              target,
              line: lineAt(clean, i),
            });
          }
          i = k;
          continue;
        }
      }
    }
    i++;
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
  "leverage",
  "leverages",
  "leveraging",
  "facilitate",
  "facilitates",
  "seamless",
  "seamlessly",
  "boasts",
];

const FILLER_PHRASES = [
  "in order to",
  "due to the fact",
  "it is important to note",
  "serves as",
  "stands as",
];

const PROSE_RULES = [
  { rule: "em-dash", regex: /\u2014/g },
  { rule: "en-dash", regex: /\u2013/g },
  { rule: "curly-quote", regex: /[\u2018\u2019\u201C\u201D]/g },
  {
    rule: "ai-vocabulary",
    regex: new RegExp(
      `\\b(?:${[...AI_WORDS].sort((a, b) => b.length - a.length).join("|")})\\b`,
      "gi",
    ),
  },
  {
    rule: "filler",
    regex: new RegExp(
      `\\b(?:${FILLER_PHRASES.map((p) => p.replace(/\s+/g, "\\s+"))
        .sort((a, b) => b.length - a.length)
        .join("|")})\\b`,
      "gi",
    ),
  },
];

// Finds prose rule violations in text, skipping fences, inline code and prose-ok.
export function proseIssues(text) {
  const raw = text.replace(/\r\n/g, "\n");
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
        const excerpt = originalLine.slice(start, end);
        lineIssues.push({
          line: idx + 1,
          rule,
          excerpt,
          index: matchStart,
        });
      }
    }
    lineIssues.sort((a, b) => a.index - b.index);
    for (const issue of lineIssues) {
      issues.push({
        line: issue.line,
        rule: issue.rule,
        excerpt: issue.excerpt,
      });
    }
  }

  return issues;
}

// Returns the reason from a "Docs: none, <reason>" line in the PR body, or null.
export function parseOptOut(body = process.env.PR_BODY) {
  const match = body?.match(/^\s*Docs:\s*none\b[\s,:;.-]*(.+)$/im);
  const reason = match?.[1].trim() ?? "";
  return reason.length >= 10 && !reason.includes("<reason>") ? reason : null;
}

// True when the "scripts" block of package.json differs between baseRef and HEAD.
function packageScriptsDiffer(ctx) {
  if (!ctx?.baseRef) return false;
  const run = (cmd) => execSync(cmd, { encoding: "utf8", cwd: ctx.repoRoot });
  const scripts = (json) => JSON.stringify(JSON.parse(json).scripts ?? {});
  return (
    scripts(run(`git show ${ctx.baseRef}:package.json`)) !==
    scripts(run("git show HEAD:package.json"))
  );
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

// Extracts headings outside fenced code blocks and computes their slugs.
function getDocHeadings(filePath, cache) {
  if (cache.has(filePath)) return cache.get(filePath);
  const content = readFileSync(filePath, "utf8").replace(/\r\n/g, "\n");
  const lines = content.split("\n");
  const slugs = new Set();
  const counts = new Map();
  let inFence = false;
  let fenceChar = "";
  let fenceLen = 0;

  for (const line of lines) {
    if (!inFence) {
      const match = line.match(/^[ ]{0,3}(`{3,}|~{3,})/);
      if (match) {
        inFence = true;
        fenceChar = match[1][0];
        fenceLen = match[1].length;
        continue;
      }
      const headingMatch = line.match(/^#{1,6}\s+(.*?)(?:\s+#+)?$/);
      if (headingMatch) {
        slugs.add(githubSlug(headingMatch[1].trim(), counts));
      }
    } else {
      const match = line.match(/^[ ]{0,3}(`{3,}|~{3,})\s*$/);
      if (match && match[1][0] === fenceChar && match[1].length >= fenceLen) {
        inFence = false;
      }
    }
  }

  cache.set(filePath, slugs);
  return slugs;
}

// Runs impact check against base ref.
function runImpact(baseRef, repoRoot) {
  const reason = parseOptOut();
  if (reason) {
    console.log(`docs impact: skipped by the PR body: ${reason}`);
    process.exit(0);
  }

  let diffText = "";
  try {
    diffText = execSync(`git diff --name-only ${baseRef}...HEAD`, {
      encoding: "utf8",
      cwd: repoRoot,
    });
  } catch (error) {
    console.error(`docs impact: git diff failed: ${error.message}`);
    process.exit(1);
  }

  const changedFiles = diffText
    .split(/\r?\n/)
    .map((s) => s.trim())
    .filter(Boolean);
  const failures = impactFailures(changedFiles, { baseRef, repoRoot });

  if (failures.length === 0) {
    console.log("docs impact: OK");
    process.exit(0);
  }

  for (const f of failures) {
    const docsLabel = Array.isArray(f.docs) ? f.docs.join(" or ") : f.docs;
    console.log(`docs impact: ${f.name}: changed ${f.file} but none of ${docsLabel}`);
  }
  console.log('Update the docs in this PR, or add "Docs: none, <reason>" to the PR body.');
  process.exit(1);
}

// Runs static checks over tracked docs and code comments.
function runStatic(repoRoot) {
  const tracked = execSync("git ls-files", { encoding: "utf8", cwd: repoRoot })
    .split(/\r?\n/)
    .map((s) => s.trim())
    .filter(Boolean);

  const docSet = tracked.filter((f) => f.endsWith(".md"));
  const proseSet = docSet.filter(
    (f) => !f.startsWith("docs/investigations/") && f !== "CODE_OF_CONDUCT.md",
  );
  const codeSet = tracked.filter((f) => {
    const ext = /\.(rs|ts|svelte|js|mjs)$/.test(f);
    const dir = /^(src\/|src-tauri\/src\/|crates\/|scripts\/)/.test(f);
    return ext && dir;
  });

  const problems = [];
  const headingCache = new Map();

  // Check 1: Links
  for (const docFile of docSet) {
    const docPath = resolve(repoRoot, docFile);
    const content = readFileSync(docPath, "utf8");
    const links = extractLinks(content);

    for (const link of links) {
      let target = link.target;
      if (target.startsWith("mailto:")) continue;

      let isRepoUrl = false;
      if (/^https?:\/\//i.test(target)) {
        const match = target.match(
          /^https:\/\/github\.com\/mikita-herasiutsin\/sim-curve-train-app\/(?:blob|tree)\/main\/(.+)$/,
        );
        if (!match) continue;
        target = match[1];
        isRepoUrl = true;
      }

      const hashIdx = target.indexOf("#");
      const rawPath = hashIdx >= 0 ? target.slice(0, hashIdx) : target;
      const anchor = hashIdx >= 0 ? target.slice(hashIdx + 1) : null;
      const qIdx = rawPath.indexOf("?");
      const pathPart = qIdx >= 0 ? rawPath.slice(0, qIdx) : rawPath;

      let resolvedPath;
      if (isRepoUrl) {
        resolvedPath = resolve(repoRoot, pathPart);
      } else if (pathPart === "") {
        resolvedPath = docPath;
      } else {
        resolvedPath = resolve(dirname(docPath), pathPart);
      }

      if (!existsSync(resolvedPath)) {
        problems.push({
          file: docFile,
          line: link.line,
          rule: "link",
          message: `target does not exist: ${link.target}`,
        });
        continue;
      }

      if (anchor !== null && (pathPart === "" || resolvedPath.endsWith(".md"))) {
        const slugs = getDocHeadings(resolvedPath, headingCache);
        if (!slugs.has(anchor) && !slugs.has(decodeURIComponent(anchor))) {
          const displayPath = relative(repoRoot, resolvedPath).replace(/\\/g, "/");
          problems.push({
            file: docFile,
            line: link.line,
            rule: "link",
            message: `anchor #${anchor} not found in ${displayPath}`,
          });
        }
      }
    }
  }

  // Check 2: npm scripts
  const pkgJsonPath = resolve(repoRoot, "package.json");
  const pkgJson = JSON.parse(readFileSync(pkgJsonPath, "utf8"));
  const validScripts = new Set(Object.keys(pkgJson.scripts ?? {}));

  for (const docFile of docSet) {
    const docPath = resolve(repoRoot, docFile);
    const content = readFileSync(docPath, "utf8").replace(/\r\n/g, "\n");
    const lines = content.split("\n");

    for (let i = 0; i < lines.length; i++) {
      const line = lines[i];
      const regex = /\bnpm\s+run\s+([A-Za-z0-9_:-]+)/g;
      let match;
      while ((match = regex.exec(line)) !== null) {
        const scriptName = match[1];
        if (!validScripts.has(scriptName)) {
          problems.push({
            file: docFile,
            line: i + 1,
            rule: "npm-script",
            message: `unknown script "${scriptName}"`,
          });
        }
      }
    }
  }

  // Check 3: Env vars
  const docVars = new Map();
  const codeVars = new Map();
  const envRegex = /\bSCT_[A-Z][A-Z0-9_]*\b/g;

  for (const docFile of docSet) {
    const docPath = resolve(repoRoot, docFile);
    const content = readFileSync(docPath, "utf8").replace(/\r\n/g, "\n");
    const lines = content.split("\n");
    for (let i = 0; i < lines.length; i++) {
      let match;
      envRegex.lastIndex = 0;
      while ((match = envRegex.exec(lines[i])) !== null) {
        const token = match[0];
        if (!docVars.has(token)) {
          docVars.set(token, { file: docFile, line: i + 1 });
        }
      }
    }
  }

  for (const codeFile of codeSet) {
    const codePath = resolve(repoRoot, codeFile);
    const content = readFileSync(codePath, "utf8").replace(/\r\n/g, "\n");
    const lines = content.split("\n");
    for (let i = 0; i < lines.length; i++) {
      let match;
      envRegex.lastIndex = 0;
      while ((match = envRegex.exec(lines[i])) !== null) {
        const token = match[0];
        if (!codeVars.has(token)) {
          codeVars.set(token, { file: codeFile, line: i + 1 });
        }
      }
    }
  }

  for (const [token, loc] of docVars) {
    if (!codeVars.has(token)) {
      problems.push({
        file: loc.file,
        line: loc.line,
        rule: "env-var",
        message: `${token} appears in docs but not in code set`,
      });
    }
  }

  for (const [token, loc] of codeVars) {
    if (!docVars.has(token)) {
      problems.push({
        file: loc.file,
        line: loc.line,
        rule: "env-var",
        message: `${token} appears in code set but not in docs`,
      });
    }
  }

  // Check 4: Prose lint
  for (const docFile of proseSet) {
    const docPath = resolve(repoRoot, docFile);
    const content = readFileSync(docPath, "utf8");
    const issues = proseIssues(content);
    for (const issue of issues) {
      problems.push({
        file: docFile,
        line: issue.line,
        rule: issue.rule,
        message: `"${issue.excerpt}"`,
      });
    }
  }

  for (const codeFile of codeSet) {
    const codePath = resolve(repoRoot, codeFile);
    const content = readFileSync(codePath, "utf8").replace(/\r\n/g, "\n");
    const lines = content.split("\n");
    const commentOnly = lines.map((line) => {
      const trimmed = line.trimStart();
      const isComment =
        trimmed.startsWith("//") ||
        trimmed.startsWith("/*") ||
        trimmed.startsWith("*") ||
        trimmed.startsWith("<!--");
      return isComment ? line : "";
    });
    const issues = proseIssues(commentOnly.join("\n"));
    for (const issue of issues) {
      problems.push({
        file: codeFile,
        line: issue.line,
        rule: issue.rule,
        message: `"${issue.excerpt}"`,
      });
    }
  }

  problems.sort((a, b) => {
    if (a.file !== b.file) return a.file.localeCompare(b.file);
    return a.line - b.line;
  });

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
  const repoRoot = resolve(execSync("git rev-parse --show-toplevel", { encoding: "utf8" }).trim());

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
