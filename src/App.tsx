import { useEffect } from 'react';
import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom';
import { useDockerStore } from './stores/dockerStore';
import { useGamesStore } from './stores/gamesStore';
import { useServerStore } from './stores/serverStore';
import { TitleBar } from './components/TitleBar';
import { Sidebar } from './components/Sidebar';
import { Home } from './pages/Home';
import { Servers } from './pages/Servers';
import { CreateServer } from './pages/CreateServer';
import { ServerDetail } from './pages/ServerDetail';
import { GamesPage } from './pages/Games';
import { Settings } from './pages/Settings';
import { NodesPage } from './pages/Nodes';
import { DockerRequired } from './components/DockerRequired';
import { OAuthToast } from './components/OAuthToast';
import { UpdateChecker } from './components/UpdateChecker';
import './App.css';

function App() {
  const { status, checkStatus } = useDockerStore();
  const { fetchGames } = useGamesStore();
  const { fetchServers } = useServerStore();

  useEffect(() => {
    checkStatus();
    fetchGames();
  }, [checkStatus, fetchGames]);

  useEffect(() => {
    if (status?.running) {
      fetchServers();
      const interval = setInterval(fetchServers, 10000);
      return () => clearInterval(interval);
    }
  }, [status?.running, fetchServers]);

  // Show Docker requirement screen if Docker is not available
  if (status && !status.running) {
    return (
      <div className="h-screen flex flex-col bg-slate-900">
        <TitleBar />
        <DockerRequired status={status} onRetry={checkStatus} />
        <OAuthToast />
        <UpdateChecker />
      </div>
    );
  }

  return (
    <BrowserRouter>
      <div className="h-screen flex flex-col bg-slate-900">
        <TitleBar />
        <div className="flex flex-1 overflow-hidden">
          <Sidebar />
          <main className="main-content">
            <Routes>
              <Route path="/" element={<Home />} />
              <Route path="/servers" element={<Servers />} />
              <Route path="/servers/create" element={<CreateServer />} />
              <Route path="/servers/:id" element={<ServerDetail />} />
              <Route path="/games" element={<GamesPage />} />
              <Route path="/nodes" element={<NodesPage />} />
              <Route path="/settings" element={<Settings />} />
              <Route path="*" element={<Navigate to="/" replace />} />
            </Routes>
          </main>
        </div>
        <OAuthToast />
        <UpdateChecker />
      </div>
    </BrowserRouter>
  );
}

export default App;
