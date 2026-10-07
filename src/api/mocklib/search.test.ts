import { describe, expect, it } from "vitest";
import { matchDoc, parseQuery, searchDocs, snippet, type SearchDoc } from "./search";

const doc = (id: number, relPath: string, extra: Partial<SearchDoc> = {}): SearchDoc => {
  const parts = relPath.split("/");
  return {
    id,
    relPath,
    filename: parts[parts.length - 1],
    relDir: parts.slice(0, -1).join("/"),
    title: "",
    texts: [],
    desc: "",
    idClass: "",
    ...extra,
  };
};

const docs: SearchDoc[] = [
  doc(1, "network/switches/ethernet-switch.svg"),
  doc(2, "network/ethernet.svg"),
  doc(3, "diagrams/system-architecture.svg", { texts: ["Ethernet Control Interface", "Motor Controller"] }),
  doc(4, "electrical/motor-controller.svg"),
  doc(5, "diagrams/power-system.svg", { texts: ["Motor Controller"], desc: "Plant power overview" }),
  doc(6, "ui/icons/arrow-left.svg", { title: "Arrow Left", idClass: "icon-arrow st0" }),
];

describe("parseQuery", () => {
  it("handles words, phrases, negation, globs and regex", () => {
    const pq = parseQuery('motor "control interface" -switch net*/*.svg re:^eth');
    expect(pq.terms.map((t) => t.kind)).toEqual(["word", "word", "word", "glob", "regex"]);
    expect(pq.terms[2].negate).toBe(true);
  });
  it("reports invalid regex", () => {
    expect(() => parseQuery("re:([")).toThrowError(/Invalid regular expression/);
  });
  it("empty query", () => {
    expect(parseQuery("   ").empty).toBe(true);
  });
});

describe("searchDocs", () => {
  it("ANDs tokens and ranks filename above content", () => {
    expect(searchDocs(docs, "ethernet")).toEqual([2, 1, 3]);
    expect(searchDocs(docs, "motor controller")).toEqual([4, 5, 3]);
  });
  it("supports exclusion", () => {
    expect(searchDocs(docs, "ethernet -switch")).toEqual([2, 3]);
  });
  it("supports phrases", () => {
    expect(searchDocs(docs, '"control interface"')).toEqual([3]);
  });
  it("supports globs on filename and path", () => {
    expect(searchDocs(docs, "*switch*.svg")).toEqual([1]);
    expect(searchDocs(docs, "network/*")).toEqual([2]);
    expect(searchDocs(docs, "network/**/*.svg").sort()).toEqual([1, 2]);
  });
  it("supports regex", () => {
    expect(searchDocs(docs, "re:^arrow-(left|right)")).toEqual([6]);
  });
  it("empty query returns path order", () => {
    expect(searchDocs(docs, "")).toEqual([5, 3, 4, 2, 1, 6]);
  });
});

describe("match explanations", () => {
  it("explains content matches with the matching label", () => {
    const m = matchDoc(docs[2], parseQuery("ethernet"));
    expect(m?.info).toEqual({ field: "text", snippet: "Ethernet Control Interface" });
  });
  it("uses the weakest field across terms", () => {
    const m = matchDoc(docs[4], parseQuery("power overview"));
    expect(m?.info.field).toBe("desc");
  });
  it("filename match explanation", () => {
    expect(matchDoc(docs[3], parseQuery("motor"))?.info.field).toBe("filename");
  });
  it("snippets long text around the match", () => {
    const long = "a".repeat(100) + " Ethernet " + "b".repeat(100);
    const s = snippet(long, 101, 8, 40);
    expect(s.startsWith("…")).toBe(true);
    expect(s.endsWith("…")).toBe(true);
    expect(s).toContain("Ethernet");
  });
});
