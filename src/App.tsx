import { Route, Routes } from "react-router-dom";
import Layout from "./components/Layout";
import ErrorBoundary from "./components/ErrorBoundary";
import Home from "./pages/Home";
import Settings from "./pages/Settings";
import Browse from "./pages/Browse";
import Search from "./pages/Search";
import Import from "./pages/Import";
import Chat from "./pages/Chat";
import Templates from "./pages/Templates";
import Wizard from "./pages/Wizard";
import Products from "./pages/Products";
import ResearchAssistant from "./pages/ResearchAssistant";
import RiskControl from "./pages/RiskControl";
import Skills from "./pages/Skills";

function App() {
  return (
    <ErrorBoundary>
      <Routes>
      <Route path="/" element={<Layout />}>
        <Route index element={<Home />} />
        <Route path="browse" element={<Browse />} />
        <Route path="search" element={<Search />} />
        <Route path="chat" element={<Chat />} />
        <Route path="research" element={<ResearchAssistant />} />
        <Route path="risk" element={<RiskControl />} />
        <Route path="skills" element={<Skills />} />
        <Route path="import" element={<Import />} />
        <Route path="templates" element={<Templates />} />
        <Route path="wizard/:templateId" element={<Wizard />} />
        <Route path="products" element={<Products />} />
        <Route path="settings" element={<Settings />} />
      </Route>
    </Routes>
    </ErrorBoundary>
  );
}

export default App;
