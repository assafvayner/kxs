import { describe, expect, it } from "vitest";
import { cpuUtil, memUtil, ofTotal, percent, utilClass } from "./utilization";

describe("percent", () => {
  it("rounds against the total", () => {
    expect(percent(123, 250)).toBe(49);
    expect(percent(0, 250)).toBe(0);
    expect(percent(300, 250)).toBe(120);
  });
  it("is null without a usable total", () => {
    expect(percent(123, null)).toBeNull();
    expect(percent(123, undefined)).toBeNull();
    expect(percent(123, 0)).toBeNull();
  });
});

describe("utilClass", () => {
  it("applies the 80/100 thresholds", () => {
    expect(utilClass(0)).toBe("");
    expect(utilClass(80)).toBe("");
    expect(utilClass(81)).toBe("st-warn");
    expect(utilClass(100)).toBe("st-warn");
    expect(utilClass(101)).toBe("st-bad");
    expect(utilClass(null)).toBe("");
  });
});

describe("cell formatting", () => {
  it("shows usage with percent when the request is known", () => {
    expect(cpuUtil(123, 250)).toEqual({ text: "123m 49%", cls: "" });
    expect(cpuUtil(230, 250)).toEqual({ text: "230m 92%", cls: "st-warn" });
    expect(cpuUtil(300, 250)).toEqual({ text: "300m 120%", cls: "st-bad" });
    expect(memUtil(45, 128)).toEqual({ text: "45Mi 35%", cls: "" });
  });
  it("shows bare usage without a request", () => {
    expect(cpuUtil(123, null)).toEqual({ text: "123m", cls: "" });
    expect(memUtil(45, null)).toEqual({ text: "45Mi", cls: "" });
  });
  it("shows a dash when metrics are unavailable", () => {
    expect(cpuUtil(null, 250)).toEqual({ text: "—", cls: "" });
    expect(memUtil(undefined, 128)).toEqual({ text: "—", cls: "" });
  });
});

describe("ofTotal", () => {
  it("renders used/allocatable with percent", () => {
    expect(ofTotal(412, 4000, "m")).toEqual({ text: "412m/4000m 10%", cls: "" });
    expect(ofTotal(3800, 4000, "m")).toEqual({ text: "3800m/4000m 95%", cls: "st-warn" });
  });
  it("renders a dash for an unknown allocatable", () => {
    expect(ofTotal(412, null, "m")).toEqual({ text: "412m/—", cls: "" });
  });
});
