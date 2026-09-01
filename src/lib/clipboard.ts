/** Copy text to the system clipboard, falling back to the legacy execCommand
 * path when the async clipboard API is unavailable in the webview. */
export async function copyText(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
    return;
  } catch {
    // fall through to the legacy path
  }
  const ta = document.createElement("textarea");
  ta.value = text;
  ta.style.position = "fixed";
  ta.style.opacity = "0";
  document.body.appendChild(ta);
  ta.select();
  try {
    if (!document.execCommand("copy")) throw new Error("copy rejected");
  } finally {
    ta.remove();
  }
}
