import { Route, Routes } from "react-router-dom";
import Layout from "./components/Layout";
import Home from "./pages/Home";
import Settings from "./pages/Settings";
import Browse from "./pages/Browse";
import Search from "./pages/Search";
import Import from "./pages/Import";
import Chat from "./pages/Chat";
import Templates from "./pages/Templates";
import Wizard from "./pages/Wizard";
import Products from "./pages/Products";

function App() {
  return (
    <Routes>
      <Route path="/" element={<Layout />}>
        <Route index element={<Home />} />
        <Route path="browse" element={<Browse />} />
        <Route path="search" element={<Search />} />
        <Route path="chat" element={<Chat />} />
        <Route path="import" element={<Import />} />
        <Route path="templates" element={<Templates />} />
        <Route path="wizard/:templateId" element={<Wizard />} />
        <Route path="products" element={<Products />} />
        <Route path="settings" element={<Settings />} />
      </Route>
    </Routes>
  );
}

export default App;
