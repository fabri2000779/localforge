import { useNavigate } from 'react-router-dom';
import { Plus, RefreshCw } from 'lucide-react';
import { useServerStore } from '../stores/serverStore';
import { ServerCard } from '../components/ServerCard';

export function Servers() {
  const navigate = useNavigate();
  const { servers, isLoading, fetchServers } = useServerStore();

  return (
    <div className="animate-fade-in">
      <header className="flex items-center justify-between mb-8">
        <div>
          <h1 className="text-3xl font-bold">My Servers</h1>
          <p className="text-slate-400 mt-2">
            Manage all your game servers in one place
          </p>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={() => fetchServers()}
            disabled={isLoading}
            className="btn btn-secondary"
          >
            <RefreshCw size={20} className={isLoading ? 'animate-spin' : ''} />
          </button>
          <button
            onClick={() => navigate('/servers/create')}
            className="btn btn-primary"
          >
            <Plus size={20} />
            Create Server
          </button>
        </div>
      </header>

      {servers.length === 0 ? (
        <div className="card text-center py-12">
          <div className="text-5xl mb-4">📦</div>
          <h2 className="text-xl font-semibold mb-2">No servers yet</h2>
          <p className="text-slate-400 mb-6">
            You haven't created any servers. Get started by creating your first one!
          </p>
          <button
            onClick={() => navigate('/servers/create')}
            className="btn btn-primary"
          >
            <Plus size={20} />
            Create Your First Server
          </button>
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          {servers.map(server => (
            <ServerCard key={server.id} server={server} />
          ))}
        </div>
      )}
    </div>
  );
}
