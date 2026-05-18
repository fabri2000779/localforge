import { useEffect } from 'react';
import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom';
import { useDockerStore } from './stores/dockerStore';
import { useGamesStore } from './stores/gamesStore';
import { useServerStore } from './stores/serverStore';
import { useAuthStore } from './stores/authStore';
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
  const hydrateAuth = useAuthStore((s) => s.hydrate);
  const subscribeToAuthEvents = useAuthStore((s) => s.subscribeToEvents);

  useEffect(() => {
    checkStatus();
    fetchGames();
    // Auth is optional — try to re-hydrate from the OS keychain, but
    // never block the rest of the app on it.
    void hydrateAuth();
    // Subscribe to the OAuth deep-link events so the modal closes
    // automatically when the user signs in via their browser.
    let unsubscribe: (() => void) | null = null;
    subscribeToAuthEvents().then((fn) => { unsubscribe = fn; });
    return () => { if (unsubscribe) unsubscribe(); };
  }, [checkStatus, fetchGames, hydrateAuth, subscribeToAuthEvents]);

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
      <div className="h-screen flex flex-col app-shell">
        <TitleBar />
        <DockerRequired status={status} onRetry={checkStatus} />
        <OAuthToast />
        <UpdateChecker />
      </div>
    );
  }

  return (
    <BrowserRouter>
      <div className="h-screen flex flex-col app-shell">
        <TitleBar />
        <div className="flex flex-1 overflow-hidden">
          <Sidebar />
          <main className="main-content">
            <div className="page-container">
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
            </div>
          </main>
        </div>
        <OAuthToast />
        <UpdateChecker />
      </div>
    </BrowserRouter>
  );
}

export default App;
