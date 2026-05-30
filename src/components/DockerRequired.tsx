import { RefreshCw, Download, AlertCircle, Cloud } from 'lucide-react';
import type { DockerStatus } from '../types';
import { useNodesStore } from '../stores/nodesStore';

interface Props {
  status: DockerStatus;
  onRetry: () => void;
}

export function DockerRequired({ status, onRetry }: Props) {
  // Distinguish "the user's local Docker is down" (a real Docker problem)
  // from "the selected REMOTE node is unreachable" (not a Docker problem —
  // they just need to switch back to a live machine in the sidebar).
  const { nodes, activeNodeId } = useNodesStore();
  const active = nodes.find((n) => n.id === activeNodeId);
  const isRemote = active?.kind.kind === 'remote';

  return (
    <div className="flex-1 flex items-center justify-center p-8 main-content">
      <div className="max-w-lg w-full text-center animate-fade-in">
        <div
          className="w-16 h-16 rounded-2xl mx-auto mb-6 flex items-center justify-center"
          style={{
            background: isRemote
              ? 'linear-gradient(135deg, rgba(56,189,248,0.18), rgba(249,115,22,0.18))'
              : 'linear-gradient(135deg, rgba(249,115,22,0.18), rgba(249,115,22,0.18))',
            boxShadow:
              '0 0 0 1px rgba(249, 115, 22, 0.22), 0 18px 40px -16px rgba(249,115,22,0.45)',
          }}
        >
          {isRemote ? (
            <Cloud size={34} className="text-sky-300" />
          ) : (
            <svg viewBox="0 0 24 24" fill="none" className="w-9 h-9">
              <path
                d="M5 12.5h2v2H5v-2zm0-3h2v2H5v-2zm3 3h2v2H8v-2zm0-3h2v2H8v-2zm3 3h2v2h-2v-2zm0-3h2v2h-2v-2zm3 3h2v2h-2v-2zm-3-6h2v2h-2v-2z"
                fill="#fdba74"
              />
              <path
                d="M21.5 13c-.4-.3-1.3-.4-2-.3-.1-.6-.5-1.2-1-1.6l-.4-.3-.3.4c-.6.8-.8 2.1-.2 2.9-.2.1-.7.4-1.6.4H2.6c-.3 1.5-.1 4.6 2.1 6.2 1.6 1.2 4 1.7 6.7 1.4 5.9-.7 9.4-4.6 10.3-9.6-.1 0-.1 0-.2-.1l.1.1z"
                fill="#fdba74"
              />
            </svg>
          )}
        </div>

        <h1 className="text-2xl font-bold tracking-tight text-slate-100 mb-2">
          {isRemote ? 'This node is offline' : 'Docker is required'}
        </h1>
        <p className="text-sm text-slate-400 mb-6 max-w-md mx-auto leading-relaxed">
          {isRemote ? (
            <>
              LocalForge can&apos;t reach
              {active?.label ? ` “${active.label}”` : ' this node'}. Switch to
              another machine from the node selector in the top-left, or bring
              this one back online and check again.
            </>
          ) : (
            'LocalForge uses Docker to run game servers in isolated containers.'
          )}
        </p>

        {status.error && (
          <div className="card mb-5 text-left flex items-start gap-3 !p-3.5 !border-red-900/40 !bg-red-950/20">
            <AlertCircle
              size={15}
              className="text-red-400 shrink-0 mt-0.5"
            />
            <div className="text-[12.5px] text-red-300 break-all font-mono">
              {status.error}
            </div>
          </div>
        )}

        <div className="space-y-3">
          {!isRemote &&
            (!status.available ? (
              <a
                href="https://www.docker.com/products/docker-desktop/"
                target="_blank"
                rel="noopener noreferrer"
                className="btn btn-primary w-full justify-center"
              >
                <Download size={15} strokeWidth={2.2} />
                Install Docker Desktop
              </a>
            ) : (
              <div className="card !text-left !py-4">
                <h3 className="font-semibold text-sm text-slate-100 mb-1">
                  Docker is installed but not running
                </h3>
                <p className="text-[12.5px] text-slate-400 leading-relaxed">
                  Start Docker Desktop and wait for it to fully initialize,
                  then check again.
                </p>
              </div>
            ))}

          <button
            onClick={onRetry}
            className="btn btn-secondary w-full justify-center"
          >
            <RefreshCw size={15} />
            Check again
          </button>
        </div>

        {!isRemote && (
          <div className="mt-8 card !text-left !py-4 !px-5">
            <div className="eyebrow mb-2">Why Docker?</div>
            <ul className="text-[12.5px] text-slate-400 space-y-1.5 leading-relaxed">
              <li>· Isolated environments for each game server</li>
              <li>· No Java / Python / runtime conflicts</li>
              <li>· Easy cleanup — just delete the container</li>
              <li>· Same experience on Windows, Mac and Linux</li>
            </ul>
          </div>
        )}
      </div>
    </div>
  );
}
