import type { ResourceKind } from "../api";

export type View =
  | { kind: "pods" }
  | { kind: "resource"; resourceKind: ResourceKind }
  | { kind: "yaml"; title: string; body: string }
  | { kind: "yamlEdit"; title: string; body: string; resourceKind: ResourceKind; namespace: string | null; name: string }
  | { kind: "describe"; title: string; resourceKind: ResourceKind; namespace: string | null; name: string; body: string }
  | { kind: "logs"; namespace: string; pod: string }
  | { kind: "exec"; namespace: string; pod: string; container: string | null }
  | { kind: "forwards" }
  | { kind: "metrics" };

export class ViewStack {
  stack = $state<View[]>([{ kind: "pods" }]);

  get top(): View {
    return this.stack[this.stack.length - 1];
  }
  get depth(): number {
    return this.stack.length;
  }
  get canPop(): boolean {
    return this.stack.length > 1;
  }
  push(v: View): void {
    this.stack.push(v);
  }
  replaceTop(v: View): void {
    this.stack[this.stack.length - 1] = v;
  }
  pop(): void {
    if (this.stack.length > 1) this.stack.pop();
  }
  reset(): void {
    this.stack = [{ kind: "pods" }];
  }
}
