import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import { invoke } from "@tauri-apps/api/core";
import { ContextsStore } from "./contexts.svelte";

const emptyView = {
  contexts: [],
  currentContext: null,
  files: ["/tmp/config"],
  defaultTarget: "/tmp/config",
  warnings: [],
};

describe("ContextsStore", () => {
  it("refresh loads the view", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(emptyView);
    const s = new ContextsStore();
    await s.refresh();
    expect(s.view.files).toEqual(["/tmp/config"]);
    expect(s.error).toBeNull();
  });

  it("refresh captures errors and keeps previous view", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(emptyView);
    const s = new ContextsStore();
    await s.refresh();
    vi.mocked(invoke).mockRejectedValueOnce("boom");
    await s.refresh();
    expect(s.error).toBe("boom");
    expect(s.view.files).toEqual(["/tmp/config"]);
  });
});
