import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "@fontsource/albert-sans/400.css";
import "@fontsource/albert-sans/500.css";
import "@fontsource/albert-sans/600.css";
import "@fontsource/albert-sans/700.css";
import "@fontsource/noto-sans-sc/400.css";
import "@fontsource/noto-sans-sc/500.css";
import "@fontsource/noto-sans-sc/600.css";
import "@fontsource/noto-sans-sc/700.css";
import "./styles.css";

// Warm up both UI fonts so switching language never waits for a font file.
for (const w of [400, 500, 600, 700]) {
  document.fonts.load(`${w} 13px "Albert Sans"`, "Aa").catch(() => {});
  document.fonts.load(`${w} 13px "Noto Sans SC"`, "选择配置渲染相机已连接").catch(() => {});
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
