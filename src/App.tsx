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
import { AcceptInviteToast } from './components/AcceptInviteToast';
import { RelayCommandExecutor } from './components/RelayCommandExecutor';
import { RelayLogBridge } from './components/RelayLogBridge';
import { RelayStateBridge } from './components/RelayStateBridge';
import { RelayFleetBridge } from './components/RelayFleetBridge';
import { MachineNameDialog } from './components/MachineNameDialog';
import { SyncKeyDialog } from './components/SyncKeyDialog';
import { UpdateChecker } from './components/UpdateChecker';
import { OnboardingWizard } from './components/OnboardingWizard';
import { AppDialog } from './components/AppDialog';
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
    // Auth is optional; never block the app on it.
    void hydrateAuth();
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

  // Always inside BrowserRouter (TitleBar uses useNavigate) with the Sidebar always mounted, so an
  // unreachable active node gates only the content area and the node switcher stays reachable.
  const activeDown = status != null && !status.running;
  return (
    <BrowserRouter>
      <div className="h-screen flex flex-col app-shell">
        <TitleBar />
        <div className="flex flex-1 overflow-hidden">
          <Sidebar />
          {activeDown ? (
            <DockerRequired status={status} onRetry={checkStatus} />
          ) : (
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
          )}
        </div>
        <OAuthToast />
        <AcceptInviteToast />
        <RelayCommandExecutor />
        <RelayLogBridge />
        <RelayStateBridge />
        <RelayFleetBridge />
        <SyncKeyDialog />
        <MachineNameDialog />
        <OnboardingWizard />
        <UpdateChecker />
        <AppDialog />
      </div>
    </BrowserRouter>
  );
}

export default App;
