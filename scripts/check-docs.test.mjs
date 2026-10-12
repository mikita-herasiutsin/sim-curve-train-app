import { describe, expect, it } from "vitest";
import {
  checkFiles,
  extractLinks,
  githubSlug,
  impactFailures,
  parseOptOut,
  proseIssues,
} from "./check-docs.mjs";

// Builds an SCT_ variable name without writing the prefix literally, so this file adds no
// variables to the code scan.
const envName = (name) => "SCT" + "_" + name;

// Runs checkFiles on in-memory files. Keys are repo-relative paths; the .md files are docs.
function check(files, scripts = ["verify"]) {
  const docs = Object.keys(files).filter((f) => f.endsWith(".md"));
  const code = Object.keys(files).filter((f) => /\.(rs|ts|svelte|js|mjs)$/.test(f));
  return checkFiles({
    docs,
    prose: docs.filter((f) => !f.startsWith("docs/investigations/")),
    code,
    read: (path) => {
      if (!(path in files)) throw new Error(`test read of unknown file ${path}`);
      return files[path];
    },
    exists: (path) => path in files,
    scripts: new Set(scripts),
  });
}

describe("githubSlug", () => {
  it("converts heading text to a slug", () => {
    expect(githubSlug("Smoke test (dev)")).toBe("smoke-test-dev");
  });

  it("keeps the double hyphen that GitHub gives a dot separator", () => {
    expect(githubSlug("SCT-014 · Calibration")).toBe("sct-014--calibration");
  });

  it("appends numeric suffixes for repeated headings", () => {
    const counts = new Map();
    expect(githubSlug("Heading", counts)).toBe("heading");
    expect(githubSlug("Heading", counts)).toBe("heading-1");
    expect(githubSlug("Heading", counts)).toBe("heading-2");
  });
});

describe("extractLinks", () => {
  it("extracts links while skipping code fences and inline code", () => {
    const doc = `
# Overview

[first](first.md)
[ref]: ref.md

\`[inline link](inline.md)\`

\`\`\`
[fenced link](fenced.md)
\`\`\`

~~~
[tilde fenced](tilde.md)
~~~

[second](second.md)
`;

    const targets = extractLinks(doc).map((l) => l.target);
    expect(targets).toContain("first.md");
    expect(targets).toContain("ref.md");
    expect(targets).toContain("second.md");
    expect(targets).not.toContain("inline.md");
    expect(targets).not.toContain("fenced.md");
    expect(targets).not.toContain("tilde.md");
  });

  it("reads a <...> destination up to the closing bracket", () => {
    expect(extractLinks("[a](<my file.md>)")[0].target).toBe("my file.md");
  });

  it("finds an image nested in link text", () => {
    const targets = extractLinks("[![alt](img.png)](target.md)").map((l) => l.target);
    expect(targets.sort()).toEqual(["img.png", "target.md"]);
  });
});

describe("proseIssues", () => {
  it("hits each prose rule once", () => {
    const text = [
      "Here is an em dash — right here.",
      "Here is an en dash – between numbers.",
      "Here is a “curly quote” pair.",
      "This step is crucial for success.",
      "We add this in order to satisfy the test.",
    ].join("\n");

    const issues = proseIssues(text);
    const rules = issues.map((i) => i.rule);

    expect(rules).toContain("em-dash");
    expect(rules).toContain("en-dash");
    expect(rules).toContain("curly-quote");
    expect(rules).toContain("ai-vocabulary");
    expect(rules).toContain("filler");
  });

  it("flags the added AI vocabulary", () => {
    const issues = proseIssues("We utilized it, then boasted about it.");
    expect(issues.map((i) => i.rule)).toEqual(["ai-vocabulary", "ai-vocabulary"]);
  });

  it("skips prose-ok lines and fenced code", () => {
    const text = `
This is crucial <!-- prose-ok -->
Word — word <!-- prose-ok -->

\`\`\`
This is crucial — not flagged inside fence.
In order to test “curly quotes”.
\`\`\`

Here is \`crucial\` inside inline code.
`;

    const issues = proseIssues(text);
    expect(issues).toEqual([]);
  });

  it("does not let a stray backtick hide a banned word in the next paragraph", () => {
    const issues = proseIssues("A stray ` mark here.\n\nThis step is crucial.\n");
    expect(issues.map((i) => i.rule)).toEqual(["ai-vocabulary"]);
    expect(issues[0].line).toBe(3);
  });
});

describe("parseOptOut", () => {
  it("accepts valid opt-out with reason", () => {
    const body = "Docs: none, only renames a local variable";
    expect(parseOptOut(body)).toBe("only renames a local variable");
  });

  it("rejects opt-out without reason or with short reasons", () => {
    expect(parseOptOut("Docs: none")).toBeNull();
    expect(parseOptOut("Docs: none, fix")).toBeNull();
    expect(parseOptOut("No docs needed")).toBeNull();
    expect(parseOptOut("Docs: none, <reason> left from the template")).toBeNull();
  });

  it("rejects the REASON placeholder from the PR template", () => {
    expect(parseOptOut("Docs: none, REASON")).toBeNull();
  });

  it("does not take a reason from the next line", () => {
    expect(parseOptOut("Docs: none\n\n## Testing")).toBeNull();
    expect(parseOptOut("Docs: none\r\n\r\n## Testing")).toBeNull();
  });
});

describe("impactFailures", () => {
  it("fails when preset file changes without preset docs", () => {
    const failures = impactFailures(["presets/gt3.json"]);
    expect(failures).toHaveLength(1);
    expect(failures[0].name).toBe("preset format");
    expect(failures[0].file).toBe("presets/gt3.json");
  });

  it("passes when any doc changes for app code", () => {
    const failures = impactFailures(["src/lib/drill.ts", "README.md"]);
    expect(failures).toEqual([]);
  });

  it("passes when changed files require no docs updates", () => {
    const failures = impactFailures(["LICENSE"]);
    expect(failures).toEqual([]);
  });

  it("names the base ref when package.json scripts cannot be compared", () => {
    const ctx = { baseRef: "no-such-ref-for-docs-check", repoRoot: process.cwd() };
    expect(() => impactFailures(["package.json"], ctx)).toThrow(/no-such-ref-for-docs-check/);
  });
});

describe("checkFiles", () => {
  it("passes a clean doc", () => {
    expect(check({ "README.md": "# Title\n\nSee [the title](#title).\n" })).toEqual([]);
  });

  it("reports a missing link target", () => {
    const problems = check({ "README.md": "[x](missing.md)\n" });
    expect(problems).toHaveLength(1);
    expect(problems[0]).toMatchObject({ file: "README.md", line: 1, rule: "link" });
    expect(problems[0].message).toContain("missing.md");
  });

  it("reports a missing anchor", () => {
    const problems = check({
      "README.md": "[x](docs/a.md#nope)\n",
      "docs/a.md": "# Real\n",
    });
    expect(problems).toHaveLength(1);
    expect(problems[0].message).toContain("#nope");
  });

  it("accepts headings indented by up to three spaces", () => {
    expect(check({ "README.md": "   ## Indented\n\n[x](#indented)\n" })).toEqual([]);
  });

  it("slugs headings after removing links and emphasis", () => {
    const doc =
      "## *Bold* `code` title\n\n## _Emphasis_ word\n\n[a](#bold-code-title)\n[b](#emphasis-word)\n";
    expect(check({ "README.md": doc })).toEqual([]);
  });

  it("skips external URIs and decodes percent-encoded paths", () => {
    const files = {
      "README.md": "[m](mailto:a@b.c) [h](https://example.com/x.md) [f](my%20file.md)\n",
      "my file.md": "# A\n",
    };
    expect(check(files)).toEqual([]);
  });

  it("checks the image inside a nested link", () => {
    const problems = check({
      "README.md": "[![alt](img.png)](target.md)\n",
      "target.md": "",
    });
    expect(problems).toHaveLength(1);
    expect(problems[0].message).toContain("img.png");
  });

  it("resolves a <...> destination with spaces", () => {
    const files = { "docs/x.md": "[a](<my file.md>)\n", "docs/my file.md": "# Hi\n" };
    expect(check(files)).toEqual([]);
  });

  it("reports an unknown npm script", () => {
    const problems = check({ "README.md": "Run `npm run nope` first.\n" }, ["verify"]);
    expect(problems).toHaveLength(1);
    expect(problems[0]).toMatchObject({ rule: "npm-script", line: 1 });
  });

  it("reports an SCT_ variable that only docs mention", () => {
    const problems = check({
      "README.md": `Set ${envName("DOCS_ONLY")} to a path.\n`,
      "src/a.ts": "const x = 1;\n",
    });
    expect(problems).toHaveLength(1);
    expect(problems[0]).toMatchObject({ file: "README.md", rule: "env-var" });
  });

  it("reports an SCT_ variable that only code reads", () => {
    const problems = check({
      "README.md": "No variables here.\n",
      "src/a.ts": `const x = process.env.${envName("CODE_ONLY")};\n`,
    });
    expect(problems).toHaveLength(1);
    expect(problems[0]).toMatchObject({ file: "src/a.ts", rule: "env-var" });
  });

  it("checks a block comment continuation line without a leading star", () => {
    const problems = check({
      "src/a.ts": "/*\n * fine\n range 3 – 5\n */\nconst x = 1;\n",
    });
    expect(problems).toHaveLength(1);
    expect(problems[0]).toMatchObject({ file: "src/a.ts", line: 3, rule: "en-dash" });
  });

  it("does not check code lines that start with a star", () => {
    const problems = check({
      "crates/core/src/x.rs": "fn f(s: &mut S) {\n    *s = x; // fine — kept\n}\n",
    });
    expect(problems).toEqual([]);
  });

  it("works with CRLF input", () => {
    expect(check({ "README.md": "# Title\r\n\r\n[t](#title)\r\n" })).toEqual([]);
    const problems = check({ "README.md": "Line one.\r\n\r\nThis is crucial.\r\n" });
    expect(problems).toHaveLength(1);
    expect(problems[0]).toMatchObject({ line: 3, rule: "ai-vocabulary" });
  });
});
