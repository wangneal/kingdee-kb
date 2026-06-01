import { createContext, useContext, useState, useCallback, type ReactNode } from "react";

const STORAGE_KEY = "kingdee_kb_active_project";

interface ProjectContextValue {
  projectId: string | undefined;
  setProjectId: (id: string | undefined) => void;
}

const ProjectContext = createContext<ProjectContextValue | null>(null);

export function ProjectProvider({ children }: { children: ReactNode }) {
  const [projectId, setProjectIdState] = useState<string | undefined>(() => {
    try {
      return localStorage.getItem(STORAGE_KEY) || undefined;
    } catch {
      return undefined;
    }
  });

  const setProjectId = useCallback((id: string | undefined) => {
    setProjectIdState(id);
    try {
      if (id) localStorage.setItem(STORAGE_KEY, id);
      else localStorage.removeItem(STORAGE_KEY);
    } catch { /* ignore */ }
  }, []);

  return (
    <ProjectContext.Provider value={{ projectId, setProjectId }}>
      {children}
    </ProjectContext.Provider>
  );
}

export function useProject(): ProjectContextValue {
  const ctx = useContext(ProjectContext);
  if (!ctx) throw new Error("useProject must be used within ProjectProvider");
  return ctx;
}
