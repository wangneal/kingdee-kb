import { invoke } from "@tauri-apps/api/core"
import React from "react"
import ReactDOM from "react-dom/client"
import { BrowserRouter } from "react-router-dom"
import App from "./App"
import "./index.css"

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <BrowserRouter>
      <App />
    </BrowserRouter>
  </React.StrictMode>,
)

// Notify Tauri backend that the React app is mounted and ready
// Backend will close the splashscreen window and show the main window
invoke("set_complete", { task: "frontend" }).catch(console.error)
