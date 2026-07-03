<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import "@xterm/xterm/css/xterm.css";
  import { api } from "../api";

  let { tabId, namespace, pod, container }: {
    tabId: number;
    namespace: string;
    pod: string;
    container: string | null;
  } = $props();

  let el: HTMLDivElement | undefined = $state();
  let term: Terminal | undefined;
  let execId: number | undefined;
  let closed = $state(false);

  // Kept across output batches so a multi-byte UTF-8 codepoint split across
  // two channel messages decodes correctly instead of emitting U+FFFD.
  const decoder = new TextDecoder(undefined, {});

  function b64encode(bytes: Uint8Array): string {
    let s = "";
    for (const b of bytes) s += String.fromCharCode(b);
    return btoa(s);
  }
  function b64decodeToBytes(b64: string): Uint8Array {
    const bin = atob(b64);
    const bytes = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    return bytes;
  }

  onMount(async () => {
    term = new Terminal({
      fontFamily: "ui-monospace, monospace",
      fontSize: 12,
      cursorBlink: true,
      theme: { background: "#16161e", foreground: "#c0caf5" },
    });
    term.open(el!);
    const cols = term.cols,
      rows = term.rows;
    term.onData((d) => {
      if (execId !== undefined) {
        const bytes = new TextEncoder().encode(d);
        api.execStdin(tabId, execId, b64encode(bytes)).catch(() => {});
      }
    });
    term.onResize(({ cols, rows }) => {
      if (execId !== undefined) {
        api.execResize(tabId, execId, cols, rows).catch(() => {});
      }
    });
    try {
      execId = await api.startExec(
        tabId,
        namespace,
        pod,
        container,
        ["/bin/sh", "-c", "TERM=xterm exec /bin/sh"],
        cols,
        rows,
        (ev) => {
          if (ev.type === "output") {
            term!.write(decoder.decode(b64decodeToBytes(ev.data), { stream: true }));
          } else {
            closed = true;
            term!.write(`\r\n[exec closed]\r\n`);
          }
        },
      );
    } catch (e) {
      term.write(`\r\nfailed to exec: ${String(e)}\r\n`);
    }
  });
  onDestroy(() => {
    if (execId !== undefined) api.stopExec(tabId, execId).catch(() => {});
    term?.dispose();
  });
</script>

<div class="terminal-view">
  <div class="detail-bar">
    <span class="mono">exec {namespace}/{pod}{container ? `:${container}` : ""}</span>
    {#if closed}<span class="dim">closed</span>{/if}
  </div>
  <div class="term-host" bind:this={el}></div>
</div>
