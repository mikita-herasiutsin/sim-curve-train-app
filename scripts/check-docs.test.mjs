import { describe, expect, it } from "vitest";
import {
  extractLinks,
  githubSlug,
  impactFailures,
  parseOptOut,
  proseIssues,
} from "./check-docs.mjs";

describe("githubSlug", () => {
  it("converts heading text to a slug", () => {
    expect(githubSlug("Smoke test (dev)")).toBe("smoke-test-dev");
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
});

describe("proseIssues", () => {
  it("hits each prose rule once", () => {
    const text = [
      "Here is an em dash \u2014 right here.",
      "Here is an en dash \u2013 between numbers.",
      "Here is a \u201Ccurly quote\u201D pair.",
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

  it("skips prose-ok lines and fenced code", () => {
    const text = `
This is crucial <!-- prose-ok -->
Word \u2014 word <!-- prose-ok -->

\`\`\`
This is crucial \u2014 not flagged inside fence.
In order to test \u201Ccurly quotes\u201D.
\`\`\`

Here is \`crucial\` inside inline code.
`;

    const issues = proseIssues(text);
    expect(issues).toEqual([]);
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
});
