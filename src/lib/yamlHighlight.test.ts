import { describe, expect, it } from "vitest";
import {
  tokenizeYamlDocument,
  tokenizeYamlLine,
  yamlLineSegments,
  type YamlToken,
} from "./yamlHighlight";

function toks(line: string): Array<{ cls: YamlToken["cls"]; text: string }> {
  return tokenizeYamlLine(line).map((t) => ({ cls: t.cls, text: line.slice(t.start, t.end) }));
}

function docToks(text: string): Array<Array<{ cls: YamlToken["cls"]; text: string }>> {
  const lines = text.split("\n");
  return tokenizeYamlDocument(text).map((lineToks, i) =>
    lineToks.map((t) => ({ cls: t.cls, text: lines[i].slice(t.start, t.end) })),
  );
}

describe("tokenizeYamlLine: keys and scalar values", () => {
  it("splits a plain string key/value", () => {
    expect(toks("key: value")).toEqual([
      { cls: "key", text: "key:" },
      { cls: "str", text: "value" },
    ]);
  });
  it("classifies integers and floats as num", () => {
    expect(toks("count: 3")).toEqual([
      { cls: "key", text: "count:" },
      { cls: "num", text: "3" },
    ]);
    expect(toks("ratio: 3.14")).toEqual([
      { cls: "key", text: "ratio:" },
      { cls: "num", text: "3.14" },
    ]);
    expect(toks("delta: -5")).toEqual([
      { cls: "key", text: "delta:" },
      { cls: "num", text: "-5" },
    ]);
  });
  it("classifies booleans and null as num", () => {
    expect(toks("enabled: true")[1]).toEqual({ cls: "num", text: "true" });
    expect(toks("disabled: false")[1]).toEqual({ cls: "num", text: "false" });
    expect(toks("value: null")[1]).toEqual({ cls: "num", text: "null" });
    expect(toks("value: ~")[1]).toEqual({ cls: "num", text: "~" });
  });
  it("classifies ISO timestamps as num", () => {
    expect(toks("creationTimestamp: 2024-01-01T00:00:00Z")[1]).toEqual({
      cls: "num",
      text: "2024-01-01T00:00:00Z",
    });
    expect(toks("date: 2024-01-01")[1]).toEqual({ cls: "num", text: "2024-01-01" });
  });
  it("treats a multi-word unquoted value as a single string span", () => {
    expect(toks("message: hello world foo")[1]).toEqual({
      cls: "str",
      text: "hello world foo",
    });
  });
  it("leaves a key with no value as just the key token", () => {
    expect(toks("metadata:")).toEqual([{ cls: "key", text: "metadata:" }]);
  });
  it("returns no tokens for blank/whitespace-only lines", () => {
    expect(toks("")).toEqual([]);
    expect(toks("   ")).toEqual([]);
  });
});

describe("tokenizeYamlLine: quoted keys and values", () => {
  it("highlights a quoted value containing a colon (e.g. a URL)", () => {
    expect(toks('url: "http://example.com:8080"')).toEqual([
      { cls: "key", text: "url:" },
      { cls: "str", text: '"http://example.com:8080"' },
    ]);
  });
  it("highlights a quoted key", () => {
    expect(toks('"foo-bar": baz')).toEqual([
      { cls: "key", text: '"foo-bar":' },
      { cls: "str", text: "baz" },
    ]);
  });
  it("handles single-quoted values with escaped quotes", () => {
    expect(toks("name: 'o''brien'")).toEqual([
      { cls: "key", text: "name:" },
      { cls: "str", text: "'o''brien'" },
    ]);
  });
  it("does not treat a '#' inside a quoted value as a comment", () => {
    expect(toks('note: "a # b"')).toEqual([
      { cls: "key", text: "note:" },
      { cls: "str", text: '"a # b"' },
    ]);
  });
});

describe("tokenizeYamlLine: comments", () => {
  it("highlights a whole-line comment", () => {
    expect(toks("# just a comment")).toEqual([{ cls: "cmt", text: "# just a comment" }]);
  });
  it("highlights an indented comment", () => {
    expect(toks("  # indented")).toEqual([{ cls: "cmt", text: "# indented" }]);
  });
  it("highlights an inline comment after a string value", () => {
    expect(toks("name: foo # comment")).toEqual([
      { cls: "key", text: "name:" },
      { cls: "str", text: "foo" },
      { cls: "cmt", text: "# comment" },
    ]);
  });
  it("highlights an inline comment after a number value", () => {
    expect(toks("port: 8080 # http")).toEqual([
      { cls: "key", text: "port:" },
      { cls: "num", text: "8080" },
      { cls: "cmt", text: "# http" },
    ]);
  });
  it("does not treat a '#' glued to a value as a comment", () => {
    expect(toks("token: abc#123")[1]).toEqual({ cls: "str", text: "abc#123" });
  });
});

describe("tokenizeYamlLine: document separators", () => {
  it("highlights ---", () => {
    expect(toks("---")).toEqual([{ cls: "docsep", text: "---" }]);
  });
  it("highlights --- with a trailing comment", () => {
    expect(toks("--- # sep")).toEqual([
      { cls: "docsep", text: "---" },
      { cls: "cmt", text: "# sep" },
    ]);
  });
  it("highlights the end-of-document marker", () => {
    expect(toks("...")).toEqual([{ cls: "docsep", text: "..." }]);
  });
});

describe("tokenizeYamlLine: list items", () => {
  it("highlights a dash before a scalar value", () => {
    expect(toks("- foo")).toEqual([
      { cls: "dash", text: "-" },
      { cls: "str", text: "foo" },
    ]);
  });
  it("highlights a dash before a mapping entry", () => {
    expect(toks("- name: foo")).toEqual([
      { cls: "dash", text: "-" },
      { cls: "key", text: "name:" },
      { cls: "str", text: "foo" },
    ]);
  });
  it("highlights nested dashes", () => {
    expect(toks("- - foo")).toEqual([
      { cls: "dash", text: "-" },
      { cls: "dash", text: "-" },
      { cls: "str", text: "foo" },
    ]);
  });
  it("highlights a comment after a bare dash", () => {
    expect(toks("- # comment")).toEqual([
      { cls: "dash", text: "-" },
      { cls: "cmt", text: "# comment" },
    ]);
  });
  it("does not treat a negative number as a dash", () => {
    expect(toks("- -5")).toEqual([
      { cls: "dash", text: "-" },
      { cls: "num", text: "-5" },
    ]);
  });
});

describe("tokenizeYamlLine: anchors, aliases, tags", () => {
  it("highlights an anchor before its value", () => {
    expect(toks("base: &default value")).toEqual([
      { cls: "key", text: "base:" },
      { cls: "anchor", text: "&default" },
      { cls: "str", text: "value" },
    ]);
  });
  it("highlights an alias value (merge key)", () => {
    expect(toks("<<: *default")).toEqual([
      { cls: "key", text: "<<:" },
      { cls: "anchor", text: "*default" },
    ]);
  });
  it("highlights an explicit tag before its value", () => {
    expect(toks("kind: !!str Pod")).toEqual([
      { cls: "key", text: "kind:" },
      { cls: "tag", text: "!!str" },
      { cls: "str", text: "Pod" },
    ]);
  });
});

describe("tokenizeYamlLine: flow collections are left unstyled", () => {
  it("leaves an empty flow mapping unstyled", () => {
    expect(toks("spec: {}")).toEqual([{ cls: "key", text: "spec:" }]);
  });
  it("leaves an empty flow sequence unstyled", () => {
    expect(toks("items: []")).toEqual([{ cls: "key", text: "items:" }]);
  });
  it("leaves a populated flow sequence unstyled", () => {
    expect(toks("items: [1, 2, 3]")).toEqual([{ cls: "key", text: "items:" }]);
  });
});

describe("tokenizeYamlDocument: block scalars", () => {
  it("treats literal block scalar continuation lines as plain string", () => {
    const text = ["message: |", "  line one", "  line two", "next: value"].join("\n");
    const result = docToks(text);
    expect(result[0]).toEqual([{ cls: "key", text: "message:" }]);
    expect(result[1]).toEqual([{ cls: "str", text: "line one" }]);
    expect(result[2]).toEqual([{ cls: "str", text: "line two" }]);
    expect(result[3]).toEqual([
      { cls: "key", text: "next:" },
      { cls: "str", text: "value" },
    ]);
  });
  it("treats folded block scalar (>) continuation lines as plain string", () => {
    const text = ["desc: >-", "  folded", "  text"].join("\n");
    const result = docToks(text);
    expect(result[0]).toEqual([{ cls: "key", text: "desc:" }]);
    expect(result[1]).toEqual([{ cls: "str", text: "folded" }]);
    expect(result[2]).toEqual([{ cls: "str", text: "text" }]);
  });
  it("ends a block scalar at a blank line followed by a dedented key", () => {
    const text = ["message: |", "  body", "", "next: value"].join("\n");
    const result = docToks(text);
    expect(result[2]).toEqual([]);
    expect(result[3]).toEqual([
      { cls: "key", text: "next:" },
      { cls: "str", text: "value" },
    ]);
  });
  it("handles a block scalar under a list item", () => {
    const text = ["- message: |", "    body line", "  other: value"].join("\n");
    const result = docToks(text);
    expect(result[1]).toEqual([{ cls: "str", text: "body line" }]);
    expect(result[2]).toEqual([
      { cls: "key", text: "other:" },
      { cls: "str", text: "value" },
    ]);
  });
  it("normal keys and values across multiple lines", () => {
    const text = ["apiVersion: v1", "kind: Pod", "metadata:", "  name: web"].join("\n");
    const result = docToks(text);
    expect(result[0]).toEqual([
      { cls: "key", text: "apiVersion:" },
      { cls: "str", text: "v1" },
    ]);
    expect(result[2]).toEqual([{ cls: "key", text: "metadata:" }]);
    expect(result[3]).toEqual([
      { cls: "key", text: "name:" },
      { cls: "str", text: "web" },
    ]);
  });
});

describe("yamlLineSegments", () => {
  it("reconstructs the original line exactly, gap-filling unstyled regions", () => {
    const line = "name: foo # comment";
    const segments = yamlLineSegments(line, tokenizeYamlLine(line));
    expect(segments.map((s) => s.text).join("")).toBe(line);
    expect(segments).toEqual([
      { text: "name:", cls: "key" },
      { text: " ", cls: null },
      { text: "foo", cls: "str" },
      { text: " ", cls: null },
      { text: "# comment", cls: "cmt" },
    ]);
  });
  it("returns a single unstyled segment for a line with no tokens", () => {
    expect(yamlLineSegments("", [])).toEqual([{ text: "", cls: null }]);
  });
  it("returns a single unstyled segment when the whole line is unstyled (flow value)", () => {
    const line = "items: [1, 2, 3]";
    const segments = yamlLineSegments(line, tokenizeYamlLine(line));
    expect(segments.map((s) => s.text).join("")).toBe(line);
    expect(segments.at(-1)).toEqual({ text: " [1, 2, 3]", cls: null });
  });
});
