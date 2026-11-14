// First run, or the app folder was moved to another PC: measure it and offer optimized settings.
// Nothing is applied without the user's click; "Not now" remembers this PC so it is not asked again.
import { useEffect, useState } from "react";
import { api } from "../api";
import { AutoOptimize } from "./AutoOptimize";
import { Modal } from "./Modal";

export function NewMachineDialog({ live }: { live: boolean }) {
  const [check, setCheck] = useState<{ machine: string; level: number } | null>(null);
  const [level, setLevel] = useState(3);

  useEffect(() => {
    api.optimizeMachineCheck()
      .then((c) => { if (c.new_machine) { setCheck(c); setLevel(c.level || 3); } })
      .catch(() => {});
  }, []);
  if (!check || live) return null;

  const dismiss = () => {
    api.optimizeDismiss(level).catch(() => {});
    setCheck(null);
  };
  return (
    <Modal title="New PC detected — optimize settings?" onClose={dismiss} wide
      footer={<button onClick={dismiss}>Not now</button>}>
      <div className="muted small">{check.machine}</div>
      <AutoOptimize live={live} autoStart initialLevel={check.level || 3} onLevel={setLevel} onApplied={() => setCheck(null)} />
    </Modal>
  );
}
