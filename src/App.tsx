import { Route, Routes } from "react-router-dom"
import ErrorBoundary from "./components/ErrorBoundary"
import Layout from "./components/Layout"
import { ToastProvider } from "./components/Toast"
import { AgentProvider } from "./contexts/AgentContext"
import { AudioProvider } from "./contexts/AudioContext"
import { OutlineProvider } from "./contexts/OutlineContext"
import { ProjectProvider } from "./contexts/ProjectContext"
import Browse from "./pages/Browse"
import Chat from "./pages/Chat"
import Home from "./pages/Home"
import Import from "./pages/Import"
import KnowledgeGraph from "./pages/KnowledgeGraph"
import Products from "./pages/Products"
import ProjectManagement from "./pages/ProjectManagement"
import ResearchAssistant from "./pages/ResearchAssistant"
import RiskControl from "./pages/RiskControl"
import Search from "./pages/Search"
import Settings from "./pages/Settings"
import Skills from "./pages/Skills"

function App() {
  return (
    <ErrorBoundary>
      <ToastProvider>
        <ProjectProvider>
          <AgentProvider>
            <Routes>
              <Route path="/" element={<Layout />}>
                <Route index element={<Home />} />
                <Route path="browse" element={<Browse />} />
                <Route path="search" element={<Search />} />
                <Route path="chat" element={<Chat />} />
                <Route
                  path="research"
                  element={
                    <OutlineProvider>
                      <AudioProvider>
                        <ResearchAssistant />
                      </AudioProvider>
                    </OutlineProvider>
                  }
                />
                <Route
                  path="research/:sessionId/outline"
                  element={
                    <OutlineProvider>
                      <AudioProvider>
                        <ResearchAssistant />
                      </AudioProvider>
                    </OutlineProvider>
                  }
                />
                <Route path="graph" element={<KnowledgeGraph />} />
                <Route path="risk" element={<RiskControl />} />
                <Route path="skills" element={<Skills />} />
                <Route path="import" element={<Import />} />
                <Route path="products" element={<Products />} />
                <Route path="projects" element={<ProjectManagement />} />
                <Route path="settings" element={<Settings />} />
              </Route>
            </Routes>
          </AgentProvider>
        </ProjectProvider>
      </ToastProvider>
    </ErrorBoundary>
  )
}

export default App
