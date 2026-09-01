import type { EditorView } from "@codemirror/view";

export interface ExCommands {
  write?: () => void;
  quit?: () => void;
  writeQuit?: () => void;
}

/** Ex command name -> registered prefix, for Vim.defineEx. */
export const EX_COMMANDS: ReadonlyArray<readonly [name: string, prefix: string]> = [
  ["write", "w"],
  ["quit", "q"],
  ["wq", "wq"],
  ["x", "x"],
];

const registry = new WeakMap<EditorView, ExCommands>();

export function setExCommands(view: EditorView, commands: ExCommands | null): void {
  if (commands) registry.set(view, commands);
  else registry.delete(view);
}

export function exTarget(name: string): keyof ExCommands | null {
  switch (name) {
    case "write":
      return "write";
    case "quit":
      return "quit";
    case "wq":
    case "x":
      return "writeQuit";
    default:
      return null;
  }
}

export function runEx(view: EditorView, name: string): boolean {
  const target = exTarget(name);
  const fn = target ? registry.get(view)?.[target] : undefined;
  if (!fn) return false;
  fn();
  return true;
}
