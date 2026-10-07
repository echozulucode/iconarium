import React from "react";
import ReactDOM from "react-dom/client";
import "./index.css";
import { getBackend } from "./api/backend";
import { App } from "./app/App";
import { setBackend } from "./app/backendRef";

const root = ReactDOM.createRoot(document.getElementById("root")!);

getBackend()
  .then((b) => {
    setBackend(b);
    root.render(
      <React.StrictMode>
        <App />
      </React.StrictMode>,
    );
  })
  .catch((e) => {
    console.error(e);
    root.render(
      <div style={{ padding: 24, fontSize: 13 }}>
        <strong>SVG Library Browser couldn't start.</strong>
        <pre style={{ whiteSpace: "pre-wrap" }}>{String(e)}</pre>
      </div>,
    );
  });
