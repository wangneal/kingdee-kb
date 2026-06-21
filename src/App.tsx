import { lazy, Suspense } from "react"
import { Route, Routes } from "react-router-dom"
import ErrorBoundary from "./components/ErrorBoundary"
import Layout from "./components/Layout"
import { ToastProvider } from "./components/Toast"
import { AgentProvider } from "./contexts/AgentContext"
import { AppErrorProvider } from "./contexts/AppErrorContext"
import { AsrConfigProvider } from "./contexts/AsrConfigContext"
import { AudioProvider } from "./contexts/AudioContext"
import Home from "./pages/Home"
import { KbCompilationProvider } from "./contexts/KbCompilationContext"
import { OutlineProvider } from "./contexts/OutlineContext"
import { ProjectProvider } from "./contexts/ProjectContext"

// 路由懒加载：非首屏页面按需加载，减少初始包体积
const Browse = lazy(() => import("./pages/Browse"))
const Chat = lazy(() => import("./pages/Chat"))
const Import = lazy(() => import("./pages/Import"))
const KnowledgeGraph = lazy(() => import("./pages/KnowledgeGraph"))
const Meetings = lazy(() => import("./pages/Meetings"))
const Products = lazy(() => import("./pages/Products"))
const ProjectManagement = lazy(() => import("./pages/ProjectManagement"))
const ResearchAssistant = lazy(() => import("./pages/ResearchAssistant"))
const RiskControl = lazy(() => import("./pages/RiskControl"))
const Search = lazy(() => import("./pages/Search"))
const Settings = lazy(() => import("./pages/Settings"))
const Skills = lazy(() => import("./pages/Skills"))

function App() {
  return (
    <ErrorBoundary>
      <ToastProvider>
        <ProjectProvider>
          <KbCompilationProvider>
            <AsrConfigProvider>
              <AppErrorProvider>
                <AgentProvider>
                  <Suspense
                    fallback={
                      <div className="flex h-screen items-center justify-center text-neutral-400">
                        加载中…
                      </div>
                    }
                  >
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
                        <Route path="meetings" element={<Meetings />} />
                        <Route path="settings" element={<Settings />} />
                      </Route>
                    </Routes>
                  </Suspense>
                </AgentProvider>
              </AppErrorProvider>
            </AsrConfigProvider>
          </KbCompilationProvider>
        </ProjectProvider>
      </ToastProvider>
    </ErrorBoundary>
  )
}

export default App
