import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import PopoutApp from "./PopoutApp";
import ProjectorApp from "./ProjectorApp";
import SecretsApp from "./SecretsApp";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

// Same bundle for every window; the window label picks the UI.
const label = getCurrentWebviewWindow().label;
const Root = label === "chat-popout" ? PopoutApp : label === "projector" ? ProjectorApp : label === "secrets" ? SecretsApp : App;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>,
);
