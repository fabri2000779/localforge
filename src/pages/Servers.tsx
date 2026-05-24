import { useNavigate } from 'react-router-dom';
import { Plus, RefreshCw, Server } from 'lucide-react';
import { useServerStore } from '../stores/serverStore';
import { useCanAct, useDisplayedServers } from '../utils/subUser';
import { ServerCard } from '../components/ServerCard';

export function Servers() {
  const navigate = useNavigate();
  // Owner → local Docker servers; sub-user → the owner's cloud-synced
  // servers (decrypted). `useDisplayedServers` returns the local list
  // unchanged when you're not a sub-user, so the owner path is identical.
  const servers = useDisplayedServers();
  const { isLoading, fetchServers } = useServerStore();
  // Sub-users below admin role can't create servers on the owner's host;
  // hide the CTA for them. Always true for the owner / local mode.
  const canCreate = useCanAct('server.create');

  return (
    <div className="animate-fade-in">
      <header className="page-header flex items-end justify-between gap-4 flex-wrap">
        <div>
          <div className="eyebrow mb-2">Library</div>
          <h1 className="page-title">My Servers</h1>
          <p className="page-subtitle">
            Manage all your game servers in one place.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={() => fetchServers()}
            disabled={isLoading}
            className="btn btn-secondary"
            aria-label="Refresh"
          >
            <RefreshCw size={15} className={isLoading ? 'animate-spin' : ''} />
          </button>
          {canCreate && (
            <button
              onClick={() => navigate('/servers/create')}
              className="btn btn-primary"
            >
              <Plus size={15} strokeWidth={2.2} />
              Create Server
            </button>
          )}
        </div>
      </header>

      {servers.length === 0 ? (
        <div className="card card-elevated text-center py-14 px-6 flex flex-col items-center">
          <div
            className="w-14 h-14 rounded-2xl flex items-center justify-center mb-5"
            style={{
              background:
                'linear-gradient(135deg, rgba(59,130,246,0.18), rgba(139,92,246,0.18))',
              boxShadow:
                '0 0 0 1px rgba(99, 102, 241, 0.18), 0 14px 30px -10px rgba(59,130,246,0.35)',
            }}
          >
            <Server size={24} className="text-blue-200" />
          </div>
          <h2 className="text-lg font-semibold text-slate-100 mb-1.5">
            No servers yet
          </h2>
          <p className="text-sm text-slate-400 mb-6 max-w-sm">
            {canCreate
              ? "You haven't created any servers. Get started by creating your first one."
              : 'No servers have been shared with you yet. Ask the owner to add one.'}
          </p>
          {canCreate && (
            <button
              onClick={() => navigate('/servers/create')}
              className="btn btn-primary"
            >
              <Plus size={15} strokeWidth={2.2} />
              Create your first server
            </button>
          )}
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          {servers.map((server) => (
            <ServerCard key={server.id} server={server} />
          ))}
        </div>
      )}
    </div>
  );
}
