// Projector window: the native program display covers this page; it only handles keys.
import { useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

export default function ProjectorApp() {
  useEffect(() => {
    document.body.style.background = "#000";
    const win = getCurrentWindow();
    const onKey = async (e: KeyboardEvent) => {
      if (e.key === "F11") {
        e.preventDefault();
        await win.setFullscreen(!(await win.isFullscreen()));
      } else if (e.key === "Escape") {
        if (await win.isFullscreen()) await win.setFullscreen(false);
        else await win.close();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  return null;
}
