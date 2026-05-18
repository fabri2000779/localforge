/**
 * Fire-and-forget audit event emitter. The Rust side decides whether
 * to actually send (no-op when signed-out) so we don't have to check
 * auth state here.
 *
 * Used by the server lifecycle actions in serverStore. Adding new
 * actions: just call emitAudit(action, target, metadata) — the cloud
 * side validates the action against a fixed allowlist.
 */
import { invoke } from '@tauri-apps/api/core';

export type AuditAction =
  | 'server.start'
  | 'server.stop'
  | 'server.restart'
  | 'server.delete'
  | 'server.send_command'
  | 'server.update_config';

export function emitAudit(
  action: AuditAction,
  target?: string,
  metadata?: Record<string, unknown>,
): void {
  void invoke('cloud_audit_emit', { action, target, metadata }).catch(() => { /* ignore */ });
}
