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
import { DockerRequired } from './components/DockerRequired';
import './App.css';

function App() {
  const { status, checkStatus } = useDockerStore();
  const { fetchGames } = useGamesStore();
  const { fetchServers } = useServerStore();

  useEffect(() => {
    checkStatus();
    fetchGames();
  }, []);

  useEffect(() => {
    if (status?.running) {
      fetchServers();
      // Refresh server list every 10 seconds
      const interval = setInterval(fetchServers, 10000);
      return () => clearInterval(interval);
    }
  }, [status?.running]);

  // Show Docker requirement screen if Docker is not available
  if (status && !status.running) {
    return (
      <div className="h-screen flex flex-col bg-slate-900">
        <TitleBar />
        <DockerRequired status={status} onRetry={checkStatus} />
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
              <Route path="/settings" element={<Settings />} />
              <Route path="*" element={<Navigate to="/" replace />} />
            </Routes>
          </main>
        </div>
      </div>
    </BrowserRouter>
  );
}

export default App;
