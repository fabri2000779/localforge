/**
 * Cloud auth state. Optional — the app runs perfectly without an account.
 * `me === null` is the unauthenticated state; it's the steady state for
 * users who never sign up.
 *
 * Source of truth = the Rust `cloud_me` command + the
 * `cloud://signed-in` event emitted by the deep-link handler after an
 * OAuth callback. The store re-hydrates from `cloud_me` at startup so
 * users stay signed in across app launches (the JWT lives in the OS
 * keychain — see src-tauri/src/cloud/keychain.rs).
 */
import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export interface Subscription {
  plan: 'free' | 'hobby' | 'team';
  currentPeriodEnd: number | null;
  cancelAtPeriodEnd: boolean;
  trialEndsAt: number | null;
  /** Unix ms when our cloud-side data will be hard-deleted. Set when
   *  the user dropped to free; null while on a paid plan. */
  purgeAt: number | null;
}

export interface Me {
  id: string;
  email: string;
  displayName: string | null;
  emailVerifiedAt: number | null;
  createdAt: number;
  subscription: Subscription;
  /** Envelope-encryption material. null when the user hasn't set up
   *  their sync key — typical for fresh OAuth accounts. The desktop
   *  prompts them via SyncKeyDialog the first time they log in. */
  syncKey: {
    wrappedDek: string;
    kekSalt: string;
    kekParams?: { algo: string; n: number; r: number; p: number; len: number } | null;
  } | null;
}

/** Three-state diagnostic that drives the SyncKeyDialog visibility:
 *  - 'not_set_up': nothing on the cloud → setup mode (pick a passphrase)
 *  - 'locked':     cloud has a wrap but local keychain is empty → unlock mode
 *  - 'unlocked':   ready to sync
 *  - null: not signed in / not yet checked
 */
export type SyncKeyStatus = 'not_set_up' | 'locked' | 'unlocked' | null;

export interface ApiErrorShape {
  status: number;
  code: string;
  message: string | null;
}

export type AuthError = ApiErrorShape | { code: string; message: string };

interface AuthState {
  /** null = not signed in, undefined = haven't checked yet, Me = signed in. */
  me: Me | null | undefined;
  loading: boolean;
  /** Last user-facing error from a command call. UI may surface it. */
  error: AuthError | null;
  /** Hydrate from OS keychain. Call once at app startup. */
  hydrate: () => Promise<void>;
  /** Subscribe to deep-link / OAuth events. Returns an unsubscribe fn. */
  subscribeToEvents: () => Promise<() => void>;
  signupEmail: (email: string, password: string, displayName?: string) => Promise<boolean>;
  loginEmail: (email: string, password: string) => Promise<boolean>;
  loginOAuth: (provider: 'apple' | 'discord' | 'google' | 'github') => Promise<void>;
  logout: () => Promise<void>;
  refreshMe: () => Promise<void>;
  requestPasswordReset: (email: string) => Promise<boolean>;
  resendVerification: () => Promise<boolean>;
  openCheckout: (plan: 'hobby' | 'team') => Promise<boolean>;
  openPortal: () => Promise<boolean>;

  // Phase 5 — multi-org / sub-user mode
  /** Every org the user belongs to (own + invited). Refreshed via fetchOrgs(). */
  orgs: OrgSummary[];
  /** Active org id. Defaults to the user's primary org on hydrate.
   *  Switching is persisted in localStorage so it survives restart. */
  currentOrgId: string | null;
  /** Caller's role in the current org. Cached so role gating is sync. */
  currentRole: OrgRole | null;
  fetchOrgs: () => Promise<void>;
  setCurrentOrg: (orgId: string) => void;
  /** (Re)connect the cloud relay to the ACTIVE org so a sub-user observing
   *  someone else's machines is routed into the right Durable Object — not
   *  their own primary org. Idempotent: skips when already on that org. */
  ensureRelay: () => void;

  // Tier 1 — cloud sync
  syncing: boolean;
  lastSyncedAt: number | null;
  lastSyncResult: SyncResult | null;
  syncNow: () => Promise<SyncResult | null>;
  syncPull: () => Promise<RemoteServer[] | null>;
  // Vault key
  vaultExportKey: () => Promise<string | null>;
  vaultImportKey: (b64: string) => Promise<boolean>;
  vaultHasKey: () => Promise<boolean>;
  /** Trigger the API export + native save dialog. Returns the chosen path or null. */
  exportData: () => Promise<string | null>;

  // Envelope-encryption sync key status + setup/unlock helpers.
  syncKeyStatus: SyncKeyStatus;
  /** Counter the SyncKeyDialog watches to re-open after a user
   *  clicked "Skip for now". Bumping it forces a re-show without
   *  changing any other state. */
  openSyncKeyTick: number;
  openSyncKeyDialog: () => void;
  refreshSyncKeyStatus: () => Promise<SyncKeyStatus>;
  /** Setup a brand-new sync key — used by OAuth signups picking a
   *  passphrase. Returns true on success. */
  setupSyncKey: (secret: string) => Promise<boolean>;
  /** Unlock with the passphrase on a second device. Returns true on
   *  success, false on wrong secret (UI re-prompts). */
  unlockSyncKey: (secret: string) => Promise<boolean>;
}

export interface RemoteServer {
  id: string;
  name: string;
  updated_at: number;
  decrypted: {
    id: string;
    name: string;
    game_type: string;
    port: number;
    memory_mb: number;
    config: Record<string, string>;
  } | null;
  exists_locally: boolean;
  decrypt_error: string | null;
}

export interface SyncResult {
  pushed: number;
  conflicts: string[];
  remote: RemoteServer[];
}

export type OrgRole = 'owner' | 'admin' | 'operator' | 'viewer';

export interface OrgSummary {
  id: string;
  name: string;
  role: OrgRole;
  isOwner: boolean;
  createdAt: number;
  joinedAt: number;
}

/** Role ranks for gating helpers. Higher = more permission. */
const ROLE_RANK: Record<OrgRole, number> = { viewer: 0, operator: 1, admin: 2, owner: 3 };
export function roleAtLeast(role: OrgRole | null | undefined, min: OrgRole): boolean {
  if (!role) return false;
  return ROLE_RANK[role] >= ROLE_RANK[min];
}

const CURRENT_ORG_KEY = 'localforge_current_org';

/** The org the relay loop is currently pointed at (module-scoped so
 *  `ensureRelay` can skip redundant reconnects). `'__primary__'` means
 *  "started before orgs loaded, using the Rust primary-org fallback".
 *  Reset to null on sign-in / sign-out so the next `ensureRelay` always
 *  (re)connects fresh (catches plan upgrades mid-session). */
let relayOrg: string | null = null;

/** True while an org switch is mid-pivot (Rust active-org header + decryption
 *  DEK are being swapped). Sync pulls are skipped during this window so a
 *  relay `sync-changed` can't run a pull against a half-applied org+DEK pair
 *  and overwrite the visible server list with the wrong org's data. The switch
 *  runs its own pull once the pivot completes, so nothing is missed. */
let orgSwitchInFlight = false;

/** Pivot the Rust-side active org and the decryption DEK together so the HTTP
 *  active-org header can never disagree with the key the pull/push uses.
 *  DEK FIRST: `cloud_unlock_org_dek` doesn't depend on the header, and
 *  installing the borrowed-org override before the header means the push path
 *  (which skips while an override is active) can't fire with the wrong key in
 *  the gap. Then the header. Errors are non-fatal (a member with no grant yet
 *  just sees undecrypted rows until the owner seals one). */
async function applyOrgScope(orgId: string, isOwner: boolean): Promise<void> {
  try {
    if (isOwner) await invoke('cloud_clear_org_dek');
    else await invoke('cloud_unlock_org_dek', { orgId });
  } catch {
    /* no grant / no keypair yet — pull shows undecrypted rows */
  }
  try {
    await invoke('cloud_set_active_org', { orgId });
  } catch {
    /* server falls back to the caller's primary org */
  }
}

function asErr(e: unknown): AuthError {
  if (e && typeof e === 'object') {
    const o = e as Record<string, unknown>;
    if (typeof o.code === 'string') {
      return {
        status: typeof o.status === 'number' ? o.status : 0,
        code: o.code,
        message: typeof o.message === 'string' ? o.message : null,
      };
    }
  }
  return { code: 'unknown', message: String(e) };
}

export const useAuthStore = create<AuthState>((set, get) => ({
  me: undefined,
  loading: false,
  error: null,

  hydrate: async () => {
    set({ loading: true, error: null });
    try {
      const me = await invoke<Me | null>('cloud_me');
      relayOrg = null;
      set({ me, loading: false });
      // Auto-connect the relay on startup so this device is reachable for
      // sync pushes from elsewhere as soon as the app opens. Uses the
      // primary org here; `fetchOrgs` re-points it at the active org once
      // the membership list (and the stored active org) is known.
      if (me) {
        get().ensureRelay();
      }
      // Also pull the list of orgs we belong to — needed by the
      // titlebar switcher + per-role gating across the app.
      if (me) {
        void get().fetchOrgs();
        void get().refreshSyncKeyStatus();
        // Claim THIS machine in the cloud so it gets a stable, addressable
        // identity (idempotent + once-per-session server/client-side).
        void invoke('cloud_claim_desktop').catch(() => {});
      }
    } catch (e) {
      // Network failure / token rejected → land in "not signed in" so
      // the UI shows the sign-in affordance rather than getting stuck
      // in a loading state.
      set({ me: null, loading: false, error: asErr(e) });
    }
  },

  subscribeToEvents: async () => {
    const unSignedIn = await listen<Me>('cloud://signed-in', (event) => {
      relayOrg = null;
      set({ me: event.payload, error: null, loading: false });
      // Spin up the relay so other devices' edits land here instantly.
      // `ensureRelay` no-ops for free owners; `fetchOrgs` re-points it at
      // the active org once memberships load.
      get().ensureRelay();
      void get().fetchOrgs();
      // OAuth users will land here with syncKey=null on first device
      // (or with syncKey set but no local DEK on a second device).
      // refreshSyncKeyStatus drives the SyncKeyDialog to appear.
      void get().refreshSyncKeyStatus();
      void invoke('cloud_claim_desktop').catch(() => {});
    });
    const unPartial = await listen('cloud://signed-in-partial', () => {
      // OAuth landed but /me failed — pull fresh once so the UI catches up.
      void get().refreshMe();
    });
    const unErr = await listen<{ code: string; message: string }>('cloud://auth-error', (event) => {
      set({ error: event.payload, loading: false });
    });
    // Tier 2: when the relay sees a sync_changed it auto-pulls, we just
    // patch the store so the visible "Last synced X ago" updates.
    const unSync = await listen('cloud://sync-changed', () => {
      // sync_changed comes from another device pushing — refresh our
      // remote list to reflect it. The Rust side already pulled the
      // decrypted view; we just re-read it cheaply.
      void get().syncPull();
    });
    return () => {
      unSignedIn();
      unPartial();
      unErr();
      unSync();
    };
  },

  signupEmail: async (email, password, displayName) => {
    set({ loading: true, error: null });
    try {
      const me = await invoke<Me>('cloud_signup', { email, password, displayName });
      set({ me, loading: false });
      void invoke('cloud_claim_desktop').catch(() => {});
      return true;
    } catch (e) {
      set({ loading: false, error: asErr(e) });
      return false;
    }
  },

  loginEmail: async (email, password) => {
    set({ loading: true, error: null });
    try {
      const me = await invoke<Me>('cloud_login', { email, password });
      set({ me, loading: false });
      void invoke('cloud_claim_desktop').catch(() => {});
      return true;
    } catch (e) {
      set({ loading: false, error: asErr(e) });
      return false;
    }
  },

  loginOAuth: async (provider) => {
    set({ loading: true, error: null });
    try {
      await invoke<void>('cloud_oauth_start', { provider });
      // We DON'T flip loading=false here — the user is now in their
      // browser. When they return, the deep-link event flips us via
      // subscribeToEvents.
    } catch (e) {
      set({ loading: false, error: asErr(e) });
    }
  },

  logout: async () => {
    set({ loading: true });
    relayOrg = null;
    // Clear the active-org pin + any borrowed org DEK so nothing leaks across
    // a sign-out/sign-in into a different account.
    void invoke('cloud_set_active_org', { orgId: null }).catch(() => {});
    void invoke('cloud_clear_org_dek').catch(() => {});
    // Disconnect the relay first so we don't keep an authed WS dangling.
    try { await invoke<void>('cloud_relay_stop'); } catch { /* ignore */ }
    try { await invoke<void>('cloud_logout'); } catch { /* ignore */ }
    set({
      me: null,
      loading: false,
      error: null,
      lastSyncResult: null,
      lastSyncedAt: null,
      syncKeyStatus: null,
    });
  },

  refreshMe: async () => {
    try {
      const me = await invoke<Me | null>('cloud_me');
      set({ me });
    } catch (e) {
      set({ error: asErr(e) });
    }
  },

  requestPasswordReset: async (email) => {
    set({ loading: true, error: null });
    try {
      await invoke<void>('cloud_request_password_reset', { email });
      set({ loading: false });
      return true;
    } catch (e) {
      set({ loading: false, error: asErr(e) });
      return false;
    }
  },

  resendVerification: async () => {
    try {
      await invoke<void>('cloud_resend_verification');
      return true;
    } catch (e) {
      set({ error: asErr(e) });
      return false;
    }
  },

  openCheckout: async (plan) => {
    set({ error: null });
    try {
      await invoke<void>('cloud_open_checkout', { plan });
      return true;
    } catch (e) {
      set({ error: asErr(e) });
      return false;
    }
  },

  openPortal: async () => {
    set({ error: null });
    try {
      await invoke<void>('cloud_open_portal');
      return true;
    } catch (e) {
      set({ error: asErr(e) });
      return false;
    }
  },

  // -------------------------------------------------------------------------
  // Cloud sync (Tier 1)
  // -------------------------------------------------------------------------
  syncing: false,
  lastSyncedAt: null,
  lastSyncResult: null,

  syncNow: async () => {
    set({ syncing: true, error: null });
    try {
      const r = await invoke<SyncResult>('cloud_sync_now');
      set({ syncing: false, lastSyncedAt: Date.now(), lastSyncResult: r });
      return r;
    } catch (e) {
      set({ syncing: false, error: asErr(e) });
      return null;
    }
  },

  syncPull: async () => {
    // Skip while an org switch is mid-pivot — a pull here would run against a
    // half-applied org+DEK pair. setCurrentOrg pulls once the pivot completes.
    if (orgSwitchInFlight) return null;
    try {
      const r = await invoke<RemoteServer[]>('cloud_sync_pull');
      // Patch the cached SyncResult so the UI's "remote servers" list
      // updates on relay-driven pulls too.
      const prev = get().lastSyncResult;
      set({
        lastSyncedAt: Date.now(),
        lastSyncResult: prev
          ? { ...prev, remote: r }
          : { pushed: 0, conflicts: [], remote: r },
      });
      return r;
    } catch (e) {
      set({ error: asErr(e) });
      return null;
    }
  },

  vaultExportKey: async () => {
    try {
      return await invoke<string>('cloud_vault_export_key');
    } catch (e) {
      set({ error: asErr(e) });
      return null;
    }
  },

  vaultImportKey: async (b64) => {
    try {
      await invoke<void>('cloud_vault_import_key', { keyB64: b64 });
      return true;
    } catch (e) {
      set({ error: asErr(e) });
      return false;
    }
  },

  vaultHasKey: async () => {
    try {
      return await invoke<boolean>('cloud_vault_has_key');
    } catch {
      return false;
    }
  },

  // -------------------------------------------------------------------------
  // Multi-org / sub-user mode
  // -------------------------------------------------------------------------
  orgs: [],
  currentOrgId: typeof localStorage !== 'undefined'
    ? localStorage.getItem(CURRENT_ORG_KEY)
    : null,
  currentRole: null,

  fetchOrgs: async () => {
    if (!get().me) return;
    try {
      const orgs = await invoke<OrgSummary[]>('cloud_orgs_list');
      let current = get().currentOrgId;
      // If the stored org id is no longer in the list (user got
      // removed, or first launch), fall back to primary.
      if (!current || !orgs.some((o) => o.id === current)) {
        current = orgs[0]?.id ?? null;
        if (current) localStorage.setItem(CURRENT_ORG_KEY, current);
      }
      const cur = orgs.find((o) => o.id === current);
      const role = cur?.role ?? null;
      set({ orgs, currentOrgId: current, currentRole: role });
      // Pin the Rust active-org header + unlock the right DEK for the active
      // org BEFORE the relay / any pull, so a borrowed org decrypts on restart
      // (previously you had to manually re-switch to re-unlock its DEK).
      if (current) await applyOrgScope(current, cur?.isOwner ?? false);
      // Now that we know the active org, make sure the relay is pointed at
      // it (rather than the primary-org fallback used at startup).
      get().ensureRelay();
      // Owner side: seal the org DEK to any member who's published a key but
      // isn't granted yet, so teammates can decrypt our org's servers. 403s
      // (not owner) are swallowed inside the command.
      for (const o of orgs) {
        if (o.isOwner) void invoke('cloud_process_grants', { orgId: o.id }).catch(() => {});
      }
    } catch (e) {
      set({ error: asErr(e) });
    }
  },

  setCurrentOrg: (orgId) => {
    const o = get().orgs.find((x) => x.id === orgId);
    if (!o) return;
    // Optimistic UI flip so the switcher highlights the new org immediately.
    localStorage.setItem(CURRENT_ORG_KEY, orgId);
    set({ currentOrgId: orgId, currentRole: o.role });
    // Gate sync until the Rust active-org header + decryption DEK have actually
    // pivoted (applyOrgScope does DEK-then-header), so a pull/push can't run
    // against a half-applied org+key pair. The switch then pulls once itself.
    orgSwitchInFlight = true;
    void (async () => {
      await applyOrgScope(orgId, o.isOwner);
      orgSwitchInFlight = false;
      // Owner: seal the org DEK to any members still waiting (best-effort).
      if (o.isOwner) void invoke('cloud_process_grants', { orgId }).catch(() => {});
      get().ensureRelay();
      void get().syncPull();
    })();
  },

  ensureRelay: () => {
    const { me, orgs, currentOrgId } = get();
    // NB: the active-org HTTP header is set by `applyOrgScope` (in
    // fetchOrgs / setCurrentOrg) and cleared in logout() — NOT here. ensureRelay
    // used to also call cloud_set_active_org, which raced the explicit call and
    // could briefly point the header at a stale org. It now only manages the
    // relay socket.
    if (!me) {
      relayOrg = null;
      return;
    }
    const cur = orgs.find((o) => o.id === currentOrgId);
    // Owner of the active org needs their OWN paid plan; a sub-user (member)
    // is allowed through — the cloud gates them on the host owner being on
    // Team and 402s otherwise (the loop then just backs off). Before orgs
    // load, fall back to "own plan must be paid" + the Rust primary-org.
    const allowed = cur
      ? cur.isOwner
        ? me.subscription.plan !== 'free'
        : true
      : me.subscription.plan !== 'free';
    if (!allowed) {
      if (relayOrg !== null) {
        relayOrg = null;
        void invoke('cloud_relay_stop').catch(() => {});
      }
      return;
    }
    const orgId = cur?.id;
    const target = orgId ?? '__primary__';
    if (target === relayOrg) return; // already connected there
    relayOrg = target;
    void invoke('cloud_relay_start', { orgId }).catch(() => {});
  },

  exportData: async () => {
    set({ error: null });
    try {
      return await invoke<string>('cloud_export_data');
    } catch (e) {
      // User-cancelled save is not really an error; swallow.
      const ae = asErr(e);
      if (ae.code === 'decode' && ae.message === 'cancelled') return null;
      set({ error: ae });
      return null;
    }
  },

  // -------------------------------------------------------------------------
  // Envelope-encryption sync key
  // -------------------------------------------------------------------------
  syncKeyStatus: null,
  openSyncKeyTick: 0,
  openSyncKeyDialog: () => set((s) => ({ openSyncKeyTick: s.openSyncKeyTick + 1 })),

  refreshSyncKeyStatus: async () => {
    try {
      const s = await invoke<SyncKeyStatus>('cloud_sync_key_status');
      set({ syncKeyStatus: s });
      return s;
    } catch {
      // Treat any failure as not-yet-known; the dialog stays hidden
      // rather than nagging the user about a transient network blip.
      return get().syncKeyStatus;
    }
  },

  setupSyncKey: async (secret) => {
    set({ error: null });
    try {
      await invoke<void>('cloud_sync_key_setup', { secret });
      await get().refreshSyncKeyStatus();
      return true;
    } catch (e) {
      set({ error: asErr(e) });
      return false;
    }
  },

  unlockSyncKey: async (secret) => {
    set({ error: null });
    try {
      await invoke<void>('cloud_sync_key_unlock', { secret });
      await get().refreshSyncKeyStatus();
      return true;
    } catch (e) {
      const ae = asErr(e);
      // wrong_secret is the expected non-failure path — let the UI
      // re-prompt cleanly without showing it as a global error.
      if (ae.code === 'wrong_secret') return false;
      set({ error: ae });
      return false;
    }
  },
}));
