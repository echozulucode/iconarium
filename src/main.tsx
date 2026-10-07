import React from "react";
import ReactDOM from "react-dom/client";
import "./index.css";

function Placeholder() {
  return <div className="p-6 text-sm">SVG Library Browser — scaffold</div>;
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Placeholder />
  </React.StrictMode>,
);
