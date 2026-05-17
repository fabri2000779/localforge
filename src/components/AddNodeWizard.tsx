import { useEffect, useState } from 'react';
import { useNodesStore } from '../stores/nodesStore';
import type { DockerInfo } from '../types';
import {
  X,
  Copy,
  Check,
  ChevronRight,
  ChevronLeft,
  Terminal,
  Server,
  Loader2,
  AlertCircle,
  CheckCircle2,
} from 'lucide-react';

interface Props {
  isOpen: boolean;
  onClose: () => void;
  onComplete?: () => void;
}

type Step = 'configure' | 'install' | 'pair' | 'done';

export function AddNodeWizard({ isOpen, onClose, onComplete }: Props) {
  const { installCommand, testRemote, addRemote } = useNodesStore();

  const [step, setStep] = useState<Step>('configure');

  // Step 1
  const [label, setLabel] = useState('');
  const [domain, setDomain] = useState('');

  // Step 2
  const [command, setCommand] = useState('');
  const [copied, setCopied] = useState(false);

  // Step 3
  const [url, setUrl] = useState('');
  const [token, setToken] = useState('');
  const [fingerprint, setFingerprint] = useState('');
  const [probeInfo, setProbeInfo] = useState<DockerInfo | null>(null);
  const [pairError, setPairError] = useState<string | null>(null);
  const [isTesting, setIsTesting] = useState(false);
  const [isSaving, setIsSaving] = useState(false);

  // Reset on close so the next open starts fresh.
  useEffect(() => {
    if (!isOpen) {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setStep('configure');
      setLabel('');
      setDomain('');
      setCommand('');
      setCopied(false);
      setUrl('');
      setToken('');
      setFingerprint('');
      setProbeInfo(null);
      setPairError(null);
    }
  }, [isOpen]);

  // Compute the install command whenever the user lands on step 2.
  useEffect(() => {
    if (step !== 'install') return;
    installCommand({
      domain: domain.trim() || undefined,
      label: label.trim() || undefined,
    })
      .then(setCommand)
      .catch((e) => setCommand(`# Failed to build install command: ${e}`));
  }, [step, domain, label, installCommand]);

  if (!isOpen) return null;

  const copyCommand = async () => {
    try {
      await navigator.clipboard.writeText(command);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // Clipboard API not available; the user can still select & copy.
    }
  };

  const handleTest = async () => {
    setIsTesting(true);
    setPairError(null);
    setProbeInfo(null);
    try {
      const info = await testRemote({
        label,
        url: url.trim(),
        token: token.trim(),
        fingerprint: fingerprint.trim() || undefined,
      });
      setProbeInfo(info);
    } catch (e) {
      setPairError(String(e));
    } finally {
      setIsTesting(false);
    }
  };

  const handleSave = async () => {
    setIsSaving(true);
    setPairError(null);
    try {
      await addRemote({
        label,
        url: url.trim(),
        token: token.trim(),
        fingerprint: fingerprint.trim() || undefined,
      });
      setStep('done');
    } catch (e) {
      setPairError(String(e));
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
      role="dialog"
      aria-modal="true"
    >
      <div className="bg-slate-900 border border-slate-700 rounded-xl w-full max-w-2xl mx-4 shadow-2xl">
        {/* Header */}
        <header className="flex items-center justify-between px-6 py-4 border-b border-slate-800">
          <div className="flex items-center gap-3">
            <Server className="text-blue-400" size={20} />
            <h2 className="text-lg font-semibold">Add a remote node</h2>
          </div>
          <button
            onClick={onClose}
            className="text-slate-500 hover:text-white"
            aria-label="Close"
          >
            <X size={20} />
          </button>
        </header>

        <StepIndicator current={step} />

        <div className="p-6 max-h-[60vh] overflow-y-auto">
          {step === 'configure' && (
            <ConfigureStep
              label={label}
              setLabel={setLabel}
              domain={domain}
              setDomain={setDomain}
            />
          )}
          {step === 'install' && (
            <InstallStep
              command={command}
              copied={copied}
              onCopy={copyCommand}
            />
          )}
          {step === 'pair' && (
            <PairStep
              url={url}
              setUrl={setUrl}
              token={token}
              setToken={setToken}
              fingerprint={fingerprint}
              setFingerprint={setFingerprint}
              isTesting={isTesting}
              probeInfo={probeInfo}
              error={pairError}
              onTest={handleTest}
            />
          )}
          {step === 'done' && <DoneStep label={label} />}
        </div>

        {/* Footer */}
        <footer className="flex items-center justify-between px-6 py-4 border-t border-slate-800">
          <div>
            {step !== 'configure' && step !== 'done' && (
              <button
                className="btn btn-secondary text-sm"
                onClick={() =>
                  setStep(step === 'pair' ? 'install' : 'configure')
                }
              >
                <ChevronLeft size={16} /> Back
              </button>
            )}
          </div>
          <div className="flex items-center gap-2">
            {step === 'configure' && (
              <button
                className="btn btn-primary text-sm"
                disabled={!label.trim()}
                onClick={() => setStep('install')}
              >
                Continue <ChevronRight size={16} />
              </button>
            )}
            {step === 'install' && (
              <button
                className="btn btn-primary text-sm"
                onClick={() => setStep('pair')}
              >
                I've run it, continue <ChevronRight size={16} />
              </button>
            )}
            {step === 'pair' && (
              <button
                className="btn btn-primary text-sm"
                disabled={
                  !probeInfo || !url.trim() || !token.trim() || isSaving
                }
                onClick={handleSave}
              >
                {isSaving ? (
                  <>
                    <Loader2 size={16} className="animate-spin" /> Saving…
                  </>
                ) : (
                  <>Save &amp; connect</>
                )}
              </button>
            )}
            {step === 'done' && (
              <button
                className="btn btn-primary text-sm"
                onClick={() => {
                  onComplete?.();
                  onClose();
                }}
              >
                Done
              </button>
            )}
          </div>
        </footer>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Sub-components
// ---------------------------------------------------------------------------

function StepIndicator({ current }: { current: Step }) {
  const order: Step[] = ['configure', 'install', 'pair', 'done'];
  const labels: Record<Step, string> = {
    configure: 'Configure',
    install: 'Run installer',
    pair: 'Pair',
    done: 'Done',
  };
  const idx = order.indexOf(current);
  return (
    <ol className="flex items-center gap-2 px-6 py-3 text-xs text-slate-500 border-b border-slate-800">
      {order.map((s, i) => (
        <li key={s} className="flex items-center gap-2">
          <span
            className={`flex items-center justify-center w-5 h-5 rounded-full border ${
              i < idx
                ? 'bg-emerald-500/20 border-emerald-500 text-emerald-300'
                : i === idx
                ? 'bg-blue-500/20 border-blue-500 text-blue-300'
                : 'border-slate-700'
            }`}
          >
            {i < idx ? '✓' : i + 1}
          </span>
          <span
            className={i === idx ? 'text-slate-200 font-medium' : undefined}
          >
            {labels[s]}
          </span>
          {i < order.length - 1 && (
            <ChevronRight size={12} className="text-slate-700" />
          )}
        </li>
      ))}
    </ol>
  );
}

function ConfigureStep(props: {
  label: string;
  setLabel: (v: string) => void;
  domain: string;
  setDomain: (v: string) => void;
}) {
  return (
    <div className="space-y-4">
      <p className="text-sm text-slate-400">
        Two questions and we'll generate a one-line install command for your
        server. The script auto-detects the OS, installs Docker if missing,
        sets up the agent service, and prints back what you need here.
      </p>

      <div>
        <label className="input-label">Node label</label>
        <input
          className="input"
          autoFocus
          placeholder="My Hetzner VPS"
          value={props.label}
          onChange={(e) => props.setLabel(e.target.value)}
        />
        <p className="text-xs text-slate-500 mt-1">
          How it'll show up in your list of nodes.
        </p>
      </div>

      <div>
        <label className="input-label">
          Domain <span className="text-slate-500 font-normal">(optional)</span>
        </label>
        <input
          className="input"
          placeholder="node.example.com"
          value={props.domain}
          onChange={(e) => props.setDomain(e.target.value)}
        />
        <p className="text-xs text-slate-500 mt-1">
          Leave empty and we'll use the server's public IP (auto-detected by
          the installer). Provide a domain only if it already resolves to
          this server.
        </p>
      </div>
    </div>
  );
}

function InstallStep({
  command,
  copied,
  onCopy,
}: {
  command: string;
  copied: boolean;
  onCopy: () => void;
}) {
  return (
    <div className="space-y-4">
      <p className="text-sm text-slate-400">
        SSH into your server and paste this command. It needs <code>sudo</code>
        — the installer creates a system user, generates a TLS cert, registers
        a <code>localforge-agent</code> systemd service, and prints the
        pairing values you'll paste back in the next step.
      </p>

      <div className="relative bg-slate-950 border border-slate-800 rounded-lg overflow-hidden">
        {/* Toolbar with copy button — separated so the command never has to
            share horizontal space with the button, and the long curl
            URL wraps cleanly inside the code block below. */}
        <div className="flex items-center justify-between px-3 py-2 border-b border-slate-800/80 bg-slate-900/50">
          <span className="text-[10.5px] uppercase tracking-wider font-semibold text-slate-500">
            One-liner
          </span>
          <button
            onClick={onCopy}
            className="inline-flex items-center gap-1.5 text-[11px] font-medium px-2 py-1 rounded bg-slate-800 hover:bg-slate-700 text-slate-300 transition-colors"
          >
            {copied ? (
              <>
                <Check size={12} className="text-emerald-400" /> Copied
              </>
            ) : (
              <>
                <Copy size={12} /> Copy
              </>
            )}
          </button>
        </div>
        <pre className="p-3.5 text-[12px] leading-relaxed font-mono text-slate-200 whitespace-pre-wrap [overflow-wrap:anywhere]">
{command || '# building command…'}
        </pre>
      </div>

      <details className="text-xs text-slate-500">
        <summary className="cursor-pointer hover:text-slate-400">
          What does this script actually do?
        </summary>
        <ul className="mt-2 ml-4 list-disc space-y-1">
          <li>Detects your distro (Debian, Ubuntu, RHEL, Fedora, Alpine, Arch, SUSE)</li>
          <li>Installs Docker if it's not already there</li>
          <li>Downloads the matching <code>localforge-agent</code> binary</li>
          <li>Creates a <code>localforge</code> system user in the <code>docker</code> group</li>
          <li>Generates a bearer token and a self-signed TLS cert</li>
          <li>Writes <code>/etc/localforge/agent.toml</code> with mode 0600</li>
          <li>Registers and starts a systemd service</li>
        </ul>
      </details>

      <div className="flex items-center gap-2 text-xs text-slate-500">
        <Terminal size={14} />
        Once it finishes you'll see a "Paste these three values" block.
      </div>
    </div>
  );
}

function PairStep(props: {
  url: string;
  setUrl: (v: string) => void;
  token: string;
  setToken: (v: string) => void;
  fingerprint: string;
  setFingerprint: (v: string) => void;
  isTesting: boolean;
  probeInfo: DockerInfo | null;
  error: string | null;
  onTest: () => void;
}) {
  return (
    <div className="space-y-4">
      <p className="text-sm text-slate-400">
        Paste the three lines printed by the installer. We'll connect to the
        agent over HTTPS and pin its self-signed certificate by fingerprint.
      </p>

      <div>
        <label className="input-label">URL</label>
        <input
          className="input font-mono text-sm"
          placeholder="https://1.2.3.4:7878"
          value={props.url}
          onChange={(e) => props.setUrl(e.target.value)}
        />
      </div>

      <div>
        <label className="input-label">Token</label>
        <input
          className="input font-mono text-sm"
          placeholder="lf_agent_…"
          value={props.token}
          onChange={(e) => props.setToken(e.target.value)}
        />
      </div>

      <div>
        <label className="input-label">
          Certificate fingerprint{' '}
          <span className="text-slate-500 font-normal">
            (leave empty if the agent uses a CA-signed cert)
          </span>
        </label>
        <input
          className="input font-mono text-sm"
          placeholder="SHA256:AB:CD:…"
          value={props.fingerprint}
          onChange={(e) => props.setFingerprint(e.target.value)}
        />
      </div>

      <div className="flex items-center gap-3 pt-2">
        <button
          className="btn btn-secondary text-sm"
          disabled={!props.url.trim() || !props.token.trim() || props.isTesting}
          onClick={props.onTest}
        >
          {props.isTesting ? (
            <>
              <Loader2 size={16} className="animate-spin" /> Connecting…
            </>
          ) : (
            <>Test connection</>
          )}
        </button>
        {props.probeInfo && (
          <span className="flex items-center gap-2 text-xs text-emerald-400">
            <CheckCircle2 size={14} />
            Connected — Docker {props.probeInfo.version} on{' '}
            {props.probeInfo.os} ({props.probeInfo.arch})
          </span>
        )}
        {props.error && (
          <span className="flex items-center gap-2 text-xs text-red-400">
            <AlertCircle size={14} />
            {props.error}
          </span>
        )}
      </div>
    </div>
  );
}

function DoneStep({ label }: { label: string }) {
  return (
    <div className="flex flex-col items-center justify-center py-6 text-center">
      <CheckCircle2 size={48} className="text-emerald-400 mb-4" />
      <h3 className="text-lg font-semibold mb-1">
        {label || 'Node'} is paired
      </h3>
      <p className="text-sm text-slate-400 max-w-md">
        You can now create and manage servers on this node from the dashboard
        and Servers pages.
      </p>
    </div>
  );
}
