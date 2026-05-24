import { createRoot } from "react-dom/client";
import SidebarApp from "./App";

const rootEl = document.getElementById("sidebar-root");
if (rootEl) {
  createRoot(rootEl).render(<SidebarApp />);
}
