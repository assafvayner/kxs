import { isMap, isScalar, isSeq, type Document, type Pair, type ParsedNode } from "yaml";

export interface Range {
  from: number;
  to: number;
}

type P = Pair<ParsedNode, ParsedNode | null>;

export function pointerSegments(pointer: string): string[] {
  const body = pointer.startsWith("#") ? pointer.slice(1) : pointer;
  if (!body) return [];
  return body
    .split("/")
    .slice(1)
    .map((s) => s.replace(/~1/g, "/").replace(/~0/g, "~"));
}

export interface Resolved {
  /** Value node at the pointer, null when the path stops early or the value is empty. */
  node: ParsedNode | null;
  /** Map entry that owns `node`, or the deepest entry reached when the path stops early. */
  pair: P | null;
}

export function resolvePointer(doc: Document, pointer: string): Resolved {
  let node: ParsedNode | null = doc.contents as ParsedNode | null;
  let pair: P | null = null;
  for (const seg of pointerSegments(pointer)) {
    if (!node) break;
    if (isMap(node)) {
      const p = (node.items as P[]).find((it) => isScalar(it.key) && String(it.key.value) === seg);
      if (!p) return { node: null, pair };
      pair = p;
      node = p.value;
    } else if (isSeq(node)) {
      node = (node.items[Number(seg)] as ParsedNode | undefined) ?? null;
    } else {
      return { node: null, pair };
    }
  }
  return { node, pair };
}

export function nodeRange(node: ParsedNode | null | undefined): Range | null {
  return node?.range ? { from: node.range[0], to: node.range[1] } : null;
}

export function keyRange(pair: P | null): Range | null {
  return pair ? nodeRange(pair.key) : null;
}

/** Range of the value at `pointer`, or a zero-width range after the key when the value is empty. */
export function valueRange(doc: Document, pointer: string): Range | null {
  const { node, pair } = resolvePointer(doc, pointer);
  const r = nodeRange(node);
  if (r) return r;
  const k = keyRange(pair);
  return k ? { from: k.to, to: k.to } : null;
}

export function findRootPair(doc: Document, key: string): P | null {
  const root = doc.contents;
  if (!isMap(root)) return null;
  return (root.items as P[]).find((p) => isScalar(p.key) && p.key.value === key) ?? null;
}

/** Range of the key owning the object at `pointer`; the `kind:` key (or document start) at the root. */
export function ownerKeyRange(doc: Document, pointer: string): Range {
  const { pair } = resolvePointer(doc, pointer);
  return keyRange(pair) ?? keyRange(findRootPair(doc, "kind")) ?? { from: 0, to: 0 };
}
