import { describe, expect, it } from "vitest";
import { AppError, normalizeError } from "./errors";
import { decodeIds, protocolUrl } from "./tauri";

describe("decodeIds", () => {
  it("decodes little-endian u32 ArrayBuffer", () => {
    const buf = new ArrayBuffer(12);
    const dv = new DataView(buf);
    dv.setUint32(0, 1, true);
    dv.setUint32(4, 70000, true);
    dv.setUint32(8, 0xfffffffe, true);
    expect(Array.from(decodeIds(buf))).toEqual([1, 70000, 0xfffffffe]);
  });
  it("decodes an unaligned Uint8Array view", () => {
    const bytes = new Uint8Array(9);
    new DataView(bytes.buffer).setUint32(1, 42, true);
    expect(Array.from(decodeIds(bytes.subarray(1, 5)))).toEqual([42]);
  });
  it("accepts number[] and null", () => {
    expect(Array.from(decodeIds([3, 2, 1]))).toEqual([3, 2, 1]);
    expect(decodeIds(null).length).toBe(0);
  });
});

describe("protocolUrl", () => {
  it("uses http://scheme.localhost on Windows", () => {
    expect(protocolUrl("thumb", "12/abc", true)).toBe("http://thumb.localhost/12/abc");
  });
  it("uses scheme://localhost elsewhere", () => {
    expect(protocolUrl("svgfile", "12/abc", false)).toBe("svgfile://localhost/12/abc");
  });
});

describe("normalizeError", () => {
  it("keeps BackendError objects", () => {
    const e = normalizeError({ kind: "invalid_query", message: "bad regex" });
    expect(e).toBeInstanceOf(AppError);
    expect(e.kind).toBe("invalid_query");
    expect(e.message).toBe("bad regex");
  });
  it("wraps strings, JSON strings and Errors", () => {
    expect(normalizeError("boom").message).toBe("boom");
    expect(normalizeError('{"kind":"io","message":"denied"}').kind).toBe("io");
    expect(normalizeError(new TypeError("x")).kind).toBe("TypeError");
    expect(normalizeError(undefined).message).toBe("Unknown error");
  });
});
