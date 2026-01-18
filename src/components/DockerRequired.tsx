import { RefreshCw, Download } from 'lucide-react';
import type { DockerStatus } from '../types';

interface Props {
  status: DockerStatus;
  onRetry: () => void;
}

export function DockerRequired({ status, onRetry }: Props) {
  return (
    <div className="flex-1 flex items-center justify-center p-8 bg-slate-900">
      <div className="max-w-lg text-center animate-fade-in">
        <div className="text-6xl mb-6">🐳</div>
        <h1 className="text-3xl font-bold mb-4">Docker Required</h1>
        <p className="text-slate-400 mb-6">
          Serverwave Anywhere uses Docker to run game servers in isolated containers.
          {status.error && (
            <span className="block mt-2 text-red-400 text-sm">
              Error: {status.error}
            </span>
          )}
        </p>

        <div className="space-y-4">
          {!status.available ? (
            <a
              href="https://www.docker.com/products/docker-desktop/"
              target="_blank"
              rel="noopener noreferrer"
              className="btn btn-primary w-full justify-center"
            >
              <Download size={20} />
              Install Docker Desktop
            </a>
          ) : (
            <div className="card text-left">
              <h3 className="font-semibold mb-2">Docker is installed but not running</h3>
              <p className="text-sm text-slate-400">
                Please start Docker Desktop and wait for it to fully initialize.
              </p>
            </div>
          )}

          <button onClick={onRetry} className="btn btn-secondary w-full justify-center">
            <RefreshCw size={20} />
            Check Again
          </button>
        </div>

        <div className="mt-8 p-4 bg-slate-800 rounded-lg text-left">
          <h4 className="font-medium mb-2 text-sm">Why Docker?</h4>
          <ul className="text-xs text-slate-400 space-y-1">
            <li>• Isolated environments for each game server</li>
            <li>• No Java/Python/runtime conflicts</li>
            <li>• Easy cleanup - just delete the container</li>
            <li>• Same experience on Windows, Mac, and Linux</li>
          </ul>
        </div>
      </div>
    </div>
  );
}
