/** Fire-and-forget audit emitter; the Rust side no-ops when signed out. */
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
