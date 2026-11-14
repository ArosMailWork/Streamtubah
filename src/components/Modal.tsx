import { ReactNode, useEffect, useState } from "react";
import { useModalGate } from "../store";

export function Modal(props: { title: string; onClose: () => void; children: ReactNode; footer?: ReactNode; wide?: boolean }) {
  useModalGate();
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && props.onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [props.onClose]);
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && props.onClose()}>
      <div className={`modal ${props.wide ? "wide" : ""}`}>
        <header>
          <h2>{props.title}</h2>
          <button className="icon" onClick={props.onClose} aria-label="Close">✕</button>
        </header>
        <div className="modal-body">{props.children}</div>
        {props.footer && <footer>{props.footer}</footer>}
      </div>
    </div>
  );
}

export function PromptDialog(props: {
  title: string;
  label: string;
  initial?: string;
  confirm?: string;
  onSubmit: (value: string) => void;
  onClose: () => void;
}) {
  const [value, setValue] = useState(props.initial ?? "");
  const submit = () => {
    if (value.trim()) {
      props.onSubmit(value.trim());
      props.onClose();
    }
  };
  return (
    <Modal
      title={props.title}
      onClose={props.onClose}
      footer={
        <>
          <button onClick={props.onClose}>Cancel</button>
          <button className="primary" onClick={submit}>{props.confirm ?? "OK"}</button>
        </>
      }
    >
      <label className="field">
        <span>{props.label}</span>
        <input autoFocus value={value} onChange={(e) => setValue(e.target.value)} onKeyDown={(e) => e.key === "Enter" && submit()} />
      </label>
    </Modal>
  );
}
