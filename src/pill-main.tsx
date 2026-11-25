import React from "react";
import ReactDOM from "react-dom/client";
import "./styles/pill.css";
import { Pill } from "./components/Pill";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Pill />
  </React.StrictMode>,
);
