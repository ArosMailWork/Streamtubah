// Renders a preset's split tree with draggable separators and dockable panels.
import { Fragment, useState } from "react";
import { Group, Panel, Separator } from "react-resizable-panels";
import { api, StudioState } from "../api";
import { act } from "../store";
import { ControlsPanel } from "../components/Controls";
import { MixerPanel, ScenesPanel, SourcesPanel } from "../components/Panels";
import { Preview } from "../components/Preview";
import { DockLeaf, useDockHandle } from "./dock";
import { LayoutNode, Path, Preset, setSizes, shape, WidgetId } from "./model";

/** Opens the program output in a separate window or fullscreen on a monitor. */
function ProjectorMenu() {
  const [monitors, setMonitors] = useState<string[]>([]);
  return (
    <select className="small projector-select" value="" title="Projector: show the program output in its own window"
      onFocus={() => api.projectorMonitors().then(setMonitors)} onMouseDown={(e) => e.stopPropagation()}
      onChange={(e) => {
        const v = e.target.value;
        if (v) act(() => api.projectorOpen(v === "window" ? null : +v));
      }}>
      <option value="">⧉ Projector</option>
      <option value="window">Windowed</option>
      {monitors.map((m, i) => <option key={m} value={i}>Fullscreen on {m}</option>)}
    </select>
  );
}

function DisplayWidget({ title, live, children, projector }: { title: string; live?: boolean; children: React.ReactNode; projector?: boolean }) {
  const { handle, close } = useDockHandle();
  return (
    <div className="display-widget">
      <div className={`display-label ${live ? "live" : ""}`} {...handle}>
        <span className="grow">{title}</span>
        {projector && <ProjectorMenu />}
        {close && <button className="icon small dock-close" title="Close panel" onClick={close}>✕</button>}
      </div>
      {children}
    </div>
  );
}

function Widget({ id, state }: { id: WidgetId; state: StudioState }) {
  switch (id) {
    case "preview":
      return (
        <DisplayWidget projector title={state.studio_mode ? `Preview — ${state.preview_scene ?? ""}` : `Preview — ${state.current ?? ""}`}>
          <Preview display="main" />
        </DisplayWidget>
      );
    case "program":
      return state.studio_mode ? (
        <DisplayWidget title={`Program — ${state.current ?? ""}`} live><Preview display="program" /></DisplayWidget>
      ) : (
        <DisplayWidget title="Program"><div className="widget-placeholder muted">Program view appears in Studio mode</div></DisplayWidget>
      );
    case "scenes":
      return <ScenesPanel state={state} />;
    case "sources":
      return <SourcesPanel state={state} />;
    case "mixer":
      return <MixerPanel state={state} />;
    case "controls":
      return <ControlsPanel state={state} />;
  }
}

function Node({ node, path, preset, state }: { node: LayoutNode; path: Path; preset: Preset; state: StudioState }) {
  if (node.type === "leaf") {
    return (
      <DockLeaf ctx={{ presetId: preset.id, path, widget: node.widget }}>
        <Widget id={node.widget} state={state} />
      </DockLeaf>
    );
  }
  const ids = node.children.map((_, i) => `p${path.join(".")}.${i}`);
  return (
    <Group
      orientation={node.dir === "row" ? "horizontal" : "vertical"}
      className="layout-group"
      defaultLayout={Object.fromEntries(ids.map((id, i) => [id, node.sizes[i]]))}
      onLayoutChanged={(layout) => setSizes(preset.id, path, ids.map((id) => Math.round((layout[id] ?? 0) * 100) / 100))}
    >
      {node.children.map((child, i) => (
        <Fragment key={ids[i]}>
          {i > 0 && <Separator className={`layout-sep ${node.dir}`} />}
          <Panel id={ids[i]} minSize="5" className="layout-panel">
            <Node node={child} path={[...path, i]} preset={preset} state={state} />
          </Panel>
        </Fragment>
      ))}
    </Group>
  );
}

export function LayoutView({ preset, state }: { preset: Preset; state: StudioState }) {
  // Remount when panels move (not when resized) so default sizes apply to the new structure.
  return <Node key={`${preset.id}:${shape(preset.root)}`} node={preset.root} path={[]} preset={preset} state={state} />;
}
