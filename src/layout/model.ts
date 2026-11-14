// Layout model (Unity-style): a preset is a free split tree whose leaves are
// panels. Users rearrange it by docking panels in the workspace; the layout
// drawer only picks/saves presets and shows generated thumbnails.
import { useSyncExternalStore } from "react";

export type WidgetId = "preview" | "program" | "scenes" | "sources" | "mixer" | "controls";
export type Mode = "editor" | "live" | "studio";
export type Zone = "center" | "left" | "right" | "top" | "bottom";
export type Path = number[];

export type LayoutNode =
  | { type: "split"; dir: "row" | "col"; sizes: number[]; children: LayoutNode[] }
  | { type: "leaf"; widget: WidgetId };

export interface Preset {
  id: string;
  name: string;
  builtin: boolean;
  mode: Mode;
  root: LayoutNode;
  chatOpen: boolean;
}

export const WIDGETS: { id: WidgetId; name: string; icon: string }[] = [
  { id: "preview", name: "Preview", icon: "▣" },
  { id: "program", name: "Program", icon: "◉" },
  { id: "scenes", name: "Scenes", icon: "☰" },
  { id: "sources", name: "Sources", icon: "◧" },
  { id: "mixer", name: "Mixer", icon: "♫" },
  { id: "controls", name: "Controls", icon: "●" },
];
export const widgetInfo = (id: WidgetId) => WIDGETS.find((w) => w.id === id)!;

export const MODES: { id: Mode; name: string; hint: string }[] = [
  { id: "editor", name: "Editor", hint: "Every panel, for building scenes" },
  { id: "live", name: "Live", hint: "Fewer panels and chat open, for streaming" },
  { id: "studio", name: "Studio", hint: "Edit a preview scene, then transition it on air" },
];

const leaf = (widget: WidgetId): LayoutNode => ({ type: "leaf", widget });
const row = (sizes: number[], ...children: LayoutNode[]): LayoutNode => ({ type: "split", dir: "row", sizes, children });
const col = (sizes: number[], ...children: LayoutNode[]): LayoutNode => ({ type: "split", dir: "col", sizes, children });
const dock4 = () => row([20, 27, 35, 18], leaf("scenes"), leaf("sources"), leaf("mixer"), leaf("controls"));

const BUILTIN: Preset[] = [
  { id: "editor", name: "Editor", builtin: true, mode: "editor", chatOpen: false, root: col([64, 36], leaf("preview"), dock4()) },
  { id: "sidebar-left", name: "Sidebar left", builtin: true, mode: "editor", chatOpen: false,
    root: row([22, 78], col([50, 50], leaf("scenes"), leaf("sources")), col([68, 32], leaf("preview"), row([65, 35], leaf("mixer"), leaf("controls")))) },
  { id: "sidebar-right", name: "Sidebar right", builtin: true, mode: "editor", chatOpen: false,
    root: row([78, 22], col([68, 32], leaf("preview"), row([65, 35], leaf("mixer"), leaf("controls"))), col([50, 50], leaf("scenes"), leaf("sources"))) },
  { id: "column", name: "Column", builtin: true, mode: "editor", chatOpen: false,
    root: row([24, 76], col([25, 25, 32, 18], leaf("scenes"), leaf("sources"), leaf("mixer"), leaf("controls")), leaf("preview")) },
  { id: "live", name: "Live", builtin: true, mode: "live", chatOpen: true,
    root: col([78, 22], leaf("preview"), row([30, 45, 25], leaf("scenes"), leaf("mixer"), leaf("controls"))) },
  { id: "live-compact", name: "Live compact", builtin: true, mode: "live", chatOpen: true,
    root: row([80, 20], leaf("preview"), col([40, 35, 25], leaf("scenes"), leaf("mixer"), leaf("controls"))) },
  { id: "studio", name: "Studio", builtin: true, mode: "studio", chatOpen: false,
    root: col([60, 40], row([50, 50], leaf("preview"), leaf("program")), dock4()) },
  { id: "studio-stacked", name: "Studio stacked", builtin: true, mode: "studio", chatOpen: false,
    root: row([70, 30], col([50, 50], leaf("preview"), leaf("program")), col([30, 30, 25, 15], leaf("scenes"), leaf("sources"), leaf("mixer"), leaf("controls"))) },
];

// --- Tree helpers ---------------------------------------------------------------

const clone = <T>(v: T): T => JSON.parse(JSON.stringify(v));

export function nodeAt(root: LayoutNode, path: Path): LayoutNode | undefined {
  let n: LayoutNode | undefined = root;
  for (const i of path) n = n?.type === "split" ? n.children[i] : undefined;
  return n;
}

export function widgetsIn(n: LayoutNode): WidgetId[] {
  return n.type === "leaf" ? [n.widget] : n.children.flatMap(widgetsIn);
}

/** Structure without sizes: changes when panels move, not when they are resized. */
export function shape(n: LayoutNode): string {
  return n.type === "leaf" ? n.widget : `${n.dir}(${n.children.map(shape).join(",")})`;
}

function findParent(root: LayoutNode, target: LayoutNode): { parent: Extract<LayoutNode, { type: "split" }>; index: number } | null {
  if (root.type !== "split") return null;
  const index = root.children.indexOf(target);
  if (index >= 0) return { parent: root, index };
  for (const c of root.children) {
    const r = findParent(c, target);
    if (r) return r;
  }
  return null;
}

/** Removes a node by identity, collapsing single-child splits. Returns the new root. */
function remove(root: LayoutNode, target: LayoutNode): LayoutNode {
  const hit = findParent(root, target);
  if (!hit) return root;
  const { parent, index } = hit;
  const freed = parent.sizes[index];
  parent.children.splice(index, 1);
  parent.sizes.splice(index, 1);
  const neighbor = Math.min(index, parent.sizes.length - 1);
  if (neighbor >= 0) parent.sizes[neighbor] += freed;
  if (parent.children.length === 1) {
    const only = parent.children[0];
    if (parent === root) return only;
    const up = findParent(root, parent)!;
    up.parent.children[up.index] = only;
  }
  return root;
}

/** Places `node` next to `target` (by identity) on the given side. */
function insert(root: LayoutNode, target: LayoutNode, node: LayoutNode, zone: Exclude<Zone, "center">): LayoutNode {
  const dir = zone === "left" || zone === "right" ? "row" : "col";
  const before = zone === "left" || zone === "top";
  const hit = findParent(root, target);
  if (hit && hit.parent.dir === dir) {
    const { parent, index } = hit;
    const half = parent.sizes[index] / 2;
    parent.sizes[index] = half;
    const at = before ? index : index + 1;
    parent.children.splice(at, 0, node);
    parent.sizes.splice(at, 0, half);
    return root;
  }
  const split: LayoutNode = { type: "split", dir, sizes: [50, 50], children: before ? [node, target] : [target, node] };
  if (!hit) return split;
  hit.parent.children[hit.index] = split;
  return root;
}

// --- Persistence -----------------------------------------------------------------

const KEY = "streamtubah.layouts.v2";

interface Saved {
  presets: Preset[];
  active: string;
}

function load(): Saved {
  try {
    const s = JSON.parse(localStorage.getItem(KEY) ?? "") as Saved;
    const presets = [...BUILTIN.filter((b) => !s.presets.some((p) => p.id === b.id)).map(clone), ...s.presets];
    return { presets, active: presets.some((p) => p.id === s.active) ? s.active : "editor" };
  } catch {
    return { presets: BUILTIN.map(clone), active: "editor" };
  }
}

let state: Saved = load();
const subs = new Set<() => void>();
function commit(next: Saved) {
  state = next;
  localStorage.setItem(KEY, JSON.stringify(state));
  subs.forEach((f) => f());
}

export function useLayouts() {
  return useSyncExternalStore(
    (cb) => {
      subs.add(cb);
      return () => {
        subs.delete(cb);
      };
    },
    () => state,
  );
}

export function activePreset(): Preset {
  return state.presets.find((p) => p.id === state.active) ?? state.presets[0];
}

export function setActive(id: string) {
  commit({ ...state, active: id });
}

export function updatePreset(id: string, patch: Partial<Preset>) {
  commit({ ...state, presets: state.presets.map((p) => (p.id === id ? { ...p, ...patch } : p)) });
}

function editRoot(id: string, f: (root: LayoutNode) => LayoutNode) {
  const p = state.presets.find((x) => x.id === id);
  if (p) updatePreset(id, { root: f(clone(p.root)) });
}

export function setSizes(id: string, path: Path, sizes: number[]) {
  editRoot(id, (root) => {
    const n = nodeAt(root, path);
    if (n?.type === "split" && n.children.length === sizes.length) n.sizes = sizes;
    return root;
  });
}

/** Docks the panel at `from` onto the panel at `to`: swap (center) or split beside it. */
export function dock(id: string, from: Path, to: Path, zone: Zone) {
  if (from.join() === to.join()) return;
  editRoot(id, (root) => {
    const a = nodeAt(root, from);
    const b = nodeAt(root, to);
    if (a?.type !== "leaf" || b?.type !== "leaf") return root;
    if (zone === "center") {
      [a.widget, b.widget] = [b.widget, a.widget];
      return root;
    }
    return insert(remove(root, a), b, a, zone);
  });
}

export function closePanel(id: string, path: Path) {
  editRoot(id, (root) => {
    const n = nodeAt(root, path);
    return n && n !== root ? remove(root, n) : root;
  });
}

/** Adds a missing panel along the right edge of the layout. */
export function addPanel(id: string, widget: WidgetId) {
  editRoot(id, (root) => {
    if (widgetsIn(root).includes(widget)) return root;
    if (root.type === "split" && root.dir === "row") {
      root.sizes = root.sizes.map((s) => s * 0.8);
      root.children.push(leaf(widget));
      root.sizes.push(20);
      return root;
    }
    return row([80, 20], root, leaf(widget));
  });
}

export function savePresetAs(name: string, mode: Mode, chatOpen: boolean): string {
  const id = `user-${Date.now().toString(36)}`;
  const src = activePreset();
  commit({ presets: [...state.presets, { id, name, builtin: false, mode, chatOpen, root: clone(src.root) }], active: id });
  return id;
}

export function duplicatePreset(id: string): string {
  const src = state.presets.find((p) => p.id === id) ?? activePreset();
  const copy = { ...clone(src), id: `user-${Date.now().toString(36)}`, name: `${src.name} copy`, builtin: false };
  commit({ ...state, presets: [...state.presets, copy] });
  return copy.id;
}

export function deletePreset(id: string) {
  const presets = state.presets.filter((p) => p.id !== id || p.builtin);
  commit({ presets, active: state.active === id ? "editor" : state.active });
}

export function resetPreset(id: string) {
  const b = BUILTIN.find((x) => x.id === id);
  if (b) updatePreset(id, clone(b));
}

export function isModified(p: Preset): boolean {
  const b = BUILTIN.find((x) => x.id === p.id);
  return !!b && JSON.stringify(b.root) !== JSON.stringify(p.root);
}
