import { Fragment, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { Card } from "@flowntier/ui";
import {
  deleteSecret,
  listProviders,
  toggleProvider,
  listRouterRoles,
  listRouterModels,
  updateRouterRoles,
  removeCustomProvider,
  getQuotaStatus,
} from "../lib/api.js";
import type { QuotaStatusEntry } from "../lib/api.js";
import { useCustomModels } from "../hooks/useCustomModels.js";
import { useDisabledModels } from "../hooks/useDisabledModels.js";
import { appVersion, buildSha } from "../lib/version.js";
import { SearchBugPanel } from "../components/SearchBugPanel.js";
import type { RuntimeSnapshot } from "./settings/types.js";
import { EMPTY } from "./settings/constants.js";
import { normalizeSnapshotModels, safeStr } from "./settings/helpers.js";
import { QuickAddAI } from "./settings/QuickAddAI.js";
import { CustomProviderForm } from "./settings/CustomProviderForm.js";
import { Field, KeyBadge, Toggle } from "./settings/primitives.js";
import { ModelOverrideEditor } from "./settings/ModelOverrideEditor.js";
import { ProviderModelManager } from "./settings/ProviderModelManager.js";
import { QuotaStatusBlock } from "./settings/QuotaStatusBlock.js";
import { RoleAssignmentCard } from "./settings/RoleAssignmentCard.js";

export interface SettingsProps {
  open: boolean;
  onClose: () => void;
  /** Current workdir, if set. Used by the About card's
   *  "Change workdir" UI (BUG-018 fix). */
  workdir?: string | null;
}

export function Settings({ open, onClose, workdir }: SettingsProps) {
  const { t } = useTranslation();
  const [snapshot, setSnapshot] = useState<RuntimeSnapshot>(EMPTY);
  const [selected, setSelected] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [savedAt, setSavedAt] = useState<string | null>(null);
  // v0.4.20 (event 000056): quota_failures tracker. Polled on
  // open + after every role-config save so the StatusLine stays
  // current. The scheduler can flip rows between ticks, so we
  // also poll every 30s while Settings is open.
  const [quotaRows, setQuotaRows] = useState<QuotaStatusEntry[]>([]);
  // BUG-FRONTEND-RT-6 (event 000038): double-confirmation flow
  // for destructive "Clear local data" — the user must type a
  // specific phrase before the destructive button activates.
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const [deletePhrase, setDeletePhrase] = useState("");
  const [deleteBusy, setDeleteBusy] = useState(false);
  const customModels = useCustomModels();
  // v0.4.22 (event 000110): per-provider model deletion. Renders
  // an × button next to each `sel.models` entry; backend persists
  // via `PUT /api/providers/{id}/models/{model}/disable` and
  // filters the entry out of subsequent role/catalog reads.
  const disabledModels = useDisabledModels();
  // v0.4.30 (audit 000130): which (providerId, modelId) row is
  // currently in inline-edit mode. Only one at a time. Set to
  // null when the editor collapses.
  const [editingModel, setEditingModel] = useState<{ providerId: string; modelId: string } | null>(
    null,
  );

  // BUG-FRONTEND-RT-6 (event 000038): the destructive "Clear
  // local data" action. The dialog has its own phrase check;
  // this function only runs after the user has typed the
  // expected phrase. We do a final defence-in-depth check here
  // in case the function is ever called from a different code
  // path (e.g. a refactor) that bypasses the dialog.
  const confirmWipe = async () => {
    if (deletePhrase !== t("settings.about.deletePhrase")) {
      console.warn("[Settings] wipe blocked: phrase mismatch");
      return;
    }
    setDeleteBusy(true);
    try {
      await invoke("wipe_all_data");
      setDeleteDialogOpen(false);
      setDeletePhrase("");
      // Reload the page so the React state re-initializes from
      // an empty data dir. The Welcome screen will appear next
      // launch (kv.first_run is null => first run path).
      setTimeout(() => window.location.reload(), 100);
    } catch (e) {
      alert(t("settings.about.clearDataError", { error: String(e) }));
    } finally {
      setDeleteBusy(false);
    }
  };
  // Best-effort: ask the Rust side for the resolved data dir
  // (so the About card can show "Logs are at..."). Falls back to
  // a friendly placeholder if the runtime is offline.
  const [dataDir, setDataDir] = useState<string>("");
  const [logDir, setLogDir] = useState<string>("");

  const refresh = async () => {
    try {
      const [prov, roles, models] = await Promise.all([
        listProviders(),
        listRouterRoles(),
        listRouterModels(),
      ]);
      console.log("[Settings] refresh:", { prov, roles, models });
      if (prov && roles && models) {
        // Merge the user-curated custom models per provider into the
        // model picker so newly-released models (e.g. DeepSeek r2)
        // are selectable without us shipping a preset update.
        const enabledIds = new Set(prov.providers.filter((p) => p.enabled).map((p) => p.id));
        const displayNameById = new Map(prov.providers.map((p) => [p.id, p.display_name]));
        const userModels: typeof models.models = [];
        for (const pid of enabledIds) {
          for (const m of customModels.getForProvider(pid)) {
            userModels.push({
              provider: pid,
              provider_display: displayNameById.get(pid) ?? pid,
              model: m.id,
              display_name: m.display_name,
            });
          }
        }
        // v0.4.18 (event 000054, chairman directive): if the user has
        // manually added ANY model for a provider, hide the
        // fallback catalog for that provider entirely. The user
        // wants their curated list, not a union.
        const userAddedProviders = new Set(userModels.map((m) => m.provider));
        const merged = models.models.filter((m) => !userAddedProviders.has(m.provider));
        for (const um of userModels) {
          if (!merged.some((m) => m.provider === um.provider && m.model === um.model)) {
            merged.push(um);
          }
        }
        setSnapshot(
          normalizeSnapshotModels({
            providers: prov.providers,
            roles: roles.roles,
            available_models: merged,
          }),
        );
        setSavedAt(new Date().toLocaleTimeString());
      }
    } catch (e) {
      console.error("[Settings] refresh failed:", e);
    }
  };

  // v0.4.20 (event 000056): quota status refresh + 30s poll
  // while Settings is open. The scheduler can flip a row to
  // rate_limited without the user saving anything, so we need
  // to re-fetch to keep the StatusLine honest.
  const refreshQuota = async () => {
    try {
      const r = await getQuotaStatus();
      if (r.ok && r.rows) setQuotaRows(r.rows);
    } catch (e) {
      console.error("[Settings] refreshQuota failed:", e);
    }
  };
  useEffect(() => {
    if (!open) return;
    void refreshQuota();
    const id = setInterval(() => void refreshQuota(), 30_000);
    return () => clearInterval(id);
  }, [open]);

  // Re-merge when custom models change (after add/remove in the modal)
  // so the model picker updates without a full server refresh.
  useEffect(() => {
    if (snapshot.providers.length > 0) void refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [customModels.totalCount]);

  useEffect(() => {
    if (!open) return;
    void refresh();
  }, [open]);
  useEffect(() => {
    // v0.4.12 (event 000048): replaced the UA-sniff hack with
    // a real call to get_diagnostics. This returns both the
    // data_dir and log_dir that the running process is actually
    // using — not a platform-default guess.
    let cancelled = false;
    void (async () => {
      try {
        const diag = (await invoke("get_diagnostics")) as {
          data_dir?: string | null;
          log_dir?: string | null;
        };
        if (cancelled) return;
        if (diag.data_dir && diag.data_dir !== "<unknown>") {
          setDataDir(diag.data_dir);
        }
        if (diag.log_dir) {
          setLogDir(diag.log_dir);
        }
      } catch (e) {
        console.warn("[Settings] get_diagnostics failed:", e);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const toggle = async (id: string, enabled: boolean) => {
    setSaving(true);
    try {
      await toggleProvider(id, enabled);
      setSnapshot((prev) => ({
        ...prev,
        providers: prev.providers.map((p) => (p.id === id ? { ...p, enabled } : p)),
      }));
      setSavedAt(new Date().toLocaleTimeString());
    } catch (e) {
      console.error("[Settings] toggle failed:", e);
      setSnapshot((prev) => ({
        ...prev,
        // Revert the optimistic UI update on failure.
        providers: prev.providers.map((p) => (p.id === id ? { ...p, enabled: !enabled } : p)),
      }));
    } finally {
      setSaving(false);
    }
  };

  const setRoleDefault = async (role: string, model: string) => {
    setSaving(true);
    try {
      const newRoles = snapshot.roles.map((r) =>
        r.role === role ? { ...r, default_model: model } : r,
      );
      await updateRouterRoles(newRoles);
      setSnapshot((prev) => ({ ...prev, roles: newRoles }));
      setSavedAt(new Date().toLocaleTimeString());
    } finally {
      setSaving(false);
    }
  };

  // Add or remove a fallback for a single role. The full roles array
  // is sent so a single round-trip commits the new state — per-role
  // patching would require a new server endpoint.
  const setRoleFallback = async (role: string, chain: string[]) => {
    setSaving(true);
    try {
      const newRoles = snapshot.roles.map((r) =>
        r.role === role ? { ...r, fallback_chain: chain } : r,
      );
      await updateRouterRoles(newRoles);
      setSnapshot((prev) => ({ ...prev, roles: newRoles }));
      setSavedAt(new Date().toLocaleTimeString());
    } finally {
      setSaving(false);
    }
  };

  if (!open) return null;

  const sel = snapshot.providers.find((p) => p.id === selected);

  return (
    <div
      className="fixed inset-0 z-50 flex bg-black/60 backdrop-blur-sm"
      role="dialog"
      aria-modal="true"
      aria-label={t("topbar.settings")}
      onClick={onClose}
    >
      <div
        className="ml-auto flex h-full w-[1100px] max-w-[95vw] flex-col border-l border-border bg-surface-1 shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="flex h-14 shrink-0 items-center justify-between border-b border-border bg-surface-2 px-5">
          <div>
            <h2 className="text-base font-semibold text-primary">{t("topbar.settings")}</h2>
            <p className="text-xs text-text-secondary">{t("settings.headerSubtitle")}</p>
          </div>
          <div className="flex items-center gap-3">
            {/* BUG-FRONTEND-4 (audit 000026 #54): the previous
                code rendered both "Saving..." and "Saved · HH:MM:SS"
                simultaneously when saving completes, producing
                a confusing visual. Now they are mutually exclusive
                via an if/else. */}
            {saving ? (
              <span className="text-xs text-text-secondary">{t("settings.action.save")}</span>
            ) : savedAt !== null ? (
              <span className="text-xs text-text-secondary">
                {t("settings.action.savedAt", { time: savedAt })}
              </span>
            ) : null}
            <button
              type="button"
              onClick={onClose}
              className="rounded-md border border-border bg-surface-1 px-3 py-1.5 text-xs text-text-secondary hover:text-primary"
            >
              {t("settings.action.close")}
            </button>
          </div>
        </header>

        <div className="flex flex-1 overflow-hidden">
          <aside className="w-[380px] shrink-0 overflow-y-auto border-r border-border bg-surface-2 p-3">
            <div className="mb-3 flex flex-col gap-2">
              <QuickAddAI onSaved={() => void refresh()} />
              <CustomProviderForm onSaved={() => void refresh()} />
            </div>
            <h3 className="mb-2 px-1 text-xs font-semibold uppercase tracking-wide text-text-secondary">
              {t("settings.providers.titleWithCount", {
                count: snapshot.providers.filter((p) => p.has_secret || p.is_local).length,
              })}
            </h3>
            <div className="flex flex-col gap-2">
              {snapshot.providers
                .filter((p) => p.has_secret || p.is_local)
                .map((p) => {
                  const isSel = p.id === selected;
                  // v0.4.15: was `p.notes.includes` — but Rust
                  // emits `note` (singular). The old key was
                  // undefined, so custom-provider rows never
                  // showed the ✕ delete button.
                  // v0.4.22 (event 000118, post-fix): coalesce
                  // `p.note` to '' so a missing field can't
                  // throw `Cannot read properties of undefined
                  // (reading 'includes')` and crash the whole
                  // Settings panel.
                  const isCustom = (p.note ?? "").includes("Custom relay");
                  const usable = p.has_secret || p.is_local;
                  const handleDeleteCustom = async (e: React.MouseEvent) => {
                    e.stopPropagation();
                    // v0.4.22 (event 000118, follow-up): `p.display_name`
                    // is typed as `string` but the runtime can hand
                    // us `undefined` when the server omits the key.
                    // i18next's interpolation coerces undefined to
                    // the string "undefined" — better to substitute
                    // the id as a stable fallback.
                    if (
                      !confirm(
                        t("settings.confirm.deleteCustom.title", { name: p.display_name ?? p.id }),
                      )
                    )
                      return;
                    try {
                      await removeCustomProvider(p.id);
                      if (selected === p.id) setSelected(null);
                      refresh();
                    } catch (err) {
                      alert(
                        err instanceof Error ? err.message : t("settings.error.deleteCustomFailed"),
                      );
                    }
                  };
                  // v0.4.30 (audit 000130): clear the keychain
                  // entry for ANY provider (preset or custom),
                  // not just custom providers. The chairman
                  // wanted to wipe an old polluted key without
                  // removing the whole provider row.
                  const handleClearKey = async (e: React.MouseEvent) => {
                    e.stopPropagation();
                    if (
                      !confirm(
                        t("settings.confirm.deleteKey.title", { name: p.display_name ?? p.id }),
                      )
                    )
                      return;
                    try {
                      await deleteSecret(p.secret_name);
                      refresh();
                    } catch (err) {
                      alert(
                        err instanceof Error ? err.message : t("settings.error.deleteKeyFailed"),
                      );
                    }
                  };
                  return (
                    <button
                      key={p.id}
                      type="button"
                      onClick={() => setSelected(p.id)}
                      className={`flex flex-col items-start gap-1 rounded-md border p-2 text-left transition-colors ${isSel ? "border-chief bg-surface-1" : "border-border bg-surface-1 hover:border-text-secondary"} ${!usable ? "opacity-60" : ""}`}
                    >
                      <div className="flex w-full items-center justify-between">
                        <div className="flex items-center gap-2">
                          <span className="font-medium text-sm">{p.display_name}</span>
                          {p.is_local && (
                            <span className="rounded bg-surface-3 px-1.5 py-0.5 text-[10px] text-text-secondary">
                              {t("settings.models.local")}
                            </span>
                          )}
                          {isCustom && (
                            <span className="rounded bg-chief/20 px-1.5 py-0.5 text-[10px] text-chief">
                              {t("settings.custom.kindLabel")}
                            </span>
                          )}
                        </div>
                        <div className="flex items-center gap-1.5">
                          <Toggle
                            on={p.enabled}
                            onChange={(v) => void toggle(p.id, v)}
                            disabled={!usable}
                          />
                          {p.has_secret && (
                            // v0.4.30: wipe the keychain entry
                            // for this provider (any kind). Custom
                            // providers keep the ✕ button alongside
                            // it for removing the whole row.
                            <button
                              type="button"
                              onClick={handleClearKey}
                              title={String(t("settings.providers.clearKey"))}
                              aria-label={String(t("settings.providers.clearKey"))}
                              className="rounded p-0.5 text-[11px] text-text-secondary hover:bg-status-warn/20 hover:text-status-warn"
                            >
                              🗑
                            </button>
                          )}
                          {isCustom && (
                            <button
                              type="button"
                              onClick={handleDeleteCustom}
                              title={String(t("settings.action.deleteCustom"))}
                              className="rounded p-0.5 text-[10px] text-status-failed hover:bg-status-failed/15 hover:text-status-failed"
                            >
                              ✕
                            </button>
                          )}
                        </div>
                      </div>
                      {/* v0.4.15: server emits default_model and a
                            hardcoded empty models array; we surface the
                            default model name (and the env var it reads)
                            so the row isn't empty. The "↻ discover"
                            button lives in the detail panel to keep this
                            row compact. */}
                      <div className="text-[11px] text-text-secondary">
                        {t("settings.quickAdd.modelCount", { count: p.models.length })}
                      </div>
                      <KeyBadge present={p.has_secret} />
                      {!p.has_secret && (
                        <div className="text-[10px] text-chief">
                          {t("settings.quickAdd.addKeyHint")}
                        </div>
                      )}
                    </button>
                  );
                })}
            </div>
          </aside>

          <main className="flex-1 overflow-y-auto p-5">
            {sel && (
              <Card className="mb-4">
                <h3 className="mb-1 text-sm font-semibold">{sel.display_name}</h3>
                <p className="mb-3 text-xs text-text-secondary">{sel.note}</p>
                <div className="grid grid-cols-2 gap-3 text-xs">
                  <Field label={t("settings.custom.kindLabel")}>{sel.api_kind}</Field>
                  <Field label={t("settings.custom.baseUrlLabel")}>
                    <code className="font-mono">{sel.base_url}</code>
                  </Field>
                  {/* v0.4.30 (audit 000130): the secret_name
                          shown here lives in the internal
                          `flowntier/<id>` namespace — it cannot
                          collide with a shell env var. The
                          tooltip reinforces that explicitly. */}
                  <Field
                    label={
                      <span className="inline-flex items-center gap-1">
                        {t("settings.custom.apiKeyLabel")}
                        <span
                          title={String(t("settings.custom.keyNotEnv"))}
                          className="cursor-help text-[10px] text-text-secondary"
                        >
                          ⓘ
                        </span>
                      </span>
                    }
                  >
                    <code className="font-mono">{sel.secret_name}</code>
                  </Field>
                  <Field label={t("settings.field.keyConfigured")}>
                    {sel.has_secret ? (
                      <span className="text-status-done">{t("settings.field.keyYes")}</span>
                    ) : (
                      <span className="text-status-warn">
                        {t("settings.field.keyNo", { env: sel.secret_name })}
                      </span>
                    )}
                  </Field>
                </div>
                {sel.models.length > 0 && (
                  <div className="mt-3">
                    <h4 className="mb-1 text-xs font-semibold uppercase tracking-wide text-text-secondary">
                      {t("settings.models.available")}
                    </h4>
                    <ul className="grid grid-cols-2 gap-1 text-xs">
                      {sel.models
                        .filter((m) => !disabledModels.isDisabled(sel.id, m.id))
                        .map((m) => {
                          const isEditing =
                            editingModel?.providerId === sel.id && editingModel?.modelId === m.id;
                          return (
                            <Fragment key={m.id}>
                              <li className="flex items-center justify-between rounded bg-surface-2 px-2 py-1 font-mono">
                                <span className="truncate">
                                  {m.display_name}{" "}
                                  <span className="ml-1 text-text-secondary">({m.id})</span>
                                </span>
                                <div className="flex items-center gap-0.5">
                                  {/* v0.4.30: ✎ edits the per-model
                                          metadata (context_length +
                                          thinking_strength). Sends a
                                          PUT to the runtime override
                                          table. */}
                                  <button
                                    type="button"
                                    aria-label={t("settings.models.editRow", {
                                      name: m.display_name ?? m.id,
                                    })}
                                    title={String(
                                      t("settings.models.editRow", {
                                        name: m.display_name ?? m.id,
                                      }),
                                    )}
                                    onClick={() =>
                                      setEditingModel(
                                        isEditing ? null : { providerId: sel.id, modelId: m.id },
                                      )
                                    }
                                    className="ml-1 rounded px-1 text-text-secondary hover:bg-primary/15 hover:text-primary"
                                  >
                                    ✎
                                  </button>
                                  <button
                                    type="button"
                                    aria-label={t("settings.models.deleteAria", {
                                      name: m.display_name ?? m.id,
                                    })}
                                    title={t("settings.models.deleteTitle", {
                                      defaultValue: "从列表中隐藏（可恢复）",
                                    })}
                                    disabled={disabledModels.busy}
                                    onClick={() => disabledModels.disable(sel.id, m.id)}
                                    className="rounded px-1 text-text-secondary hover:bg-status-fail/20 hover:text-status-fail"
                                  >
                                    ×
                                  </button>
                                </div>
                              </li>
                              {isEditing && (
                                <ModelOverrideEditor
                                  providerId={sel.id}
                                  model={m}
                                  onClose={() => setEditingModel(null)}
                                  onSaved={() => {
                                    setEditingModel(null);
                                    refresh();
                                  }}
                                />
                              )}
                            </Fragment>
                          );
                        })}
                    </ul>
                  </div>
                )}

                <ProviderModelManager
                  providerId={sel.id}
                  providerDisplay={sel.display_name}
                  customModels={customModels.getForProvider(sel.id)}
                  onAdd={(models) => customModels.addMany(sel.id, models)}
                  onRemove={(modelId) => customModels.remove(sel.id, modelId)}
                  onClear={() => customModels.clear(sel.id)}
                />
              </Card>
            )}

            <Card className="mt-4">
              <h3 className="mb-1 text-sm font-semibold">{t("settings.about.title")}</h3>
              <p className="mb-3 text-xs text-text-secondary">
                {t("settings.about.version", {
                  version: appVersion,
                  build: buildSha ? " (" + buildSha + ")" : "",
                })}
              </p>
              <p className="mb-2 text-xs text-text-secondary">{dataDir}</p>
              {logDir && (
                <>
                  <p className="mb-1 break-all text-[10px] text-text-secondary">
                    {t("settings.about.logDir")}: {logDir}
                  </p>
                  <p className="mb-3 text-[10px] text-text-tertiary">
                    {t("settings.about.logDirHint")}
                  </p>
                </>
              )}
              {/* BUG-018 fix (event 000024): Settings → About
                      now exposes a "Change workdir" button that
                      calls `set_workdir_with_nwt` (atomic) with a
                      new path. Previously this code path didn't
                      exist; users who picked a workdir on first
                      launch were stuck with it. */}
              <button
                type="button"
                onClick={async () => {
                  // Use the Tauri dialog plugin to pick a
                  // directory. Falls back to a manual text
                  // prompt if the plugin is unavailable.
                  let newPath: string | null = null;
                  try {
                    const { open } = await import("@tauri-apps/plugin-dialog");
                    newPath = await open({ directory: true, multiple: false });
                  } catch (e) {
                    console.warn("[Settings] dialog plugin unavailable:", e);
                  }
                  if (!newPath) {
                    const input = window.prompt(t("settings.about.changeWorkdirPrompt"));
                    if (!input) return;
                    newPath = input.trim();
                  }
                  if (!newPath) return;
                  try {
                    await invoke("set_workdir_with_nwt", { path: newPath });
                    // Reload to re-fetch workdir state in App.tsx.
                    setTimeout(() => window.location.reload(), 100);
                  } catch (e) {
                    alert(t("settings.about.changeWorkdirError", { error: String(e) }));
                  }
                }}
                className="mb-2 rounded-md border border-border bg-surface-2 px-3 py-2 text-xs text-text-primary hover:bg-surface-3"
              >
                {t("settings.about.changeWorkdir")}
              </button>
              {workdir && (
                <p className="mb-3 break-all text-[10px] text-text-secondary">
                  {t("settings.about.currentWorkdir")}: {workdir}
                </p>
              )}
              <button
                type="button"
                onClick={async () => {
                  // BUG-FRONTEND-RT-6 (event 000038): a
                  // second confirmation is now required —
                  // the user has to type a specific phrase
                  // ("DELETE" in en-US / "删除" in zh-CN)
                  // to prevent accidental data loss. The
                  // destructive button stays disabled until
                  // the input matches.
                  setDeletePhrase("");
                  setDeleteDialogOpen(true);
                }}
                className="rounded-md border border-status-failed/40 bg-status-failed/10 px-3 py-2 text-xs text-status-failed hover:bg-status-failed/20"
              >
                {t("settings.about.clearData")}
              </button>
            </Card>
            <SearchBugPanel />
            <QuotaStatusBlock rows={quotaRows} onReset={refreshQuota} />

            {/* v0.4.12 (event 000048): RoleAssignmentCard moved
                    here (was above the About card). Keeps the
                    "Search bug" UI immediately above the role
                    assignments, since both are read-mostly
                    diagnostic / configuration views. */}
            <h3 className="mb-2 mt-4 px-1 text-xs font-semibold uppercase tracking-wide text-text-secondary">
              {t("settings.roles.title")}
            </h3>
            <p className="mb-2 px-1 text-[10px] text-text-secondary"></p>
            <div className="flex flex-col gap-2">
              {snapshot.roles.map((r) => (
                <RoleAssignmentCard
                  key={r.role}
                  role={r}
                  // v0.4.24 (event 000119): the backend
                  // /v1/models endpoint sometimes returns rows
                  // with empty `model` / `provider` / both
                  // `display_name` AND `provider_display`. Those
                  // rendered as "(unnamed provider) · (unnamed
                  // model)" in the dropdown — visually noise that
                  // the user can't act on. Filter them out so the
                  // selector only shows entries the user can
                  // actually pick.
                  availableModels={snapshot.available_models.filter((m) => {
                    const id = safeStr(m.model);
                    const prov = safeStr(m.provider) || safeStr(m.provider_display);
                    return id.length > 0 && prov.length > 0;
                  })}
                  saving={saving}
                  onDefaultChange={(model) => void setRoleDefault(r.role, model)}
                  onFallbackChange={(chain) => void setRoleFallback(r.role, chain)}
                />
              ))}
            </div>
          </main>

          {/* BUG-FRONTEND-RT-6 (event 000038): double-confirmation
          modal for destructive "Clear local data". The user must
          type the specific phrase (`DELETE` in en-US, `删除` in
          zh-CN) to activate the destructive button. Prevents
          accidental data loss from a stray click. The phrase is
          localized via `settings.about.deletePhrase`. */}
          {deleteDialogOpen && (
            <div
              role="dialog"
              aria-modal="true"
              aria-labelledby="delete-dialog-title"
              className="fixed inset-0 z-[60] flex items-center justify-center bg-black/60 backdrop-blur-sm"
              onClick={(e) => {
                if (e.target === e.currentTarget) setDeleteDialogOpen(false);
              }}
            >
              <div className="w-[420px] max-w-[90vw] rounded-lg border border-status-failed/40 bg-surface-1 p-5 shadow-xl">
                <h2 id="delete-dialog-title" className="text-base font-semibold text-status-failed">
                  {t("settings.about.clearData")}
                </h2>
                <p className="mt-2 text-xs text-text-secondary">
                  {t("settings.about.clearDataConfirmBody")}
                </p>
                <p className="mt-3 text-xs text-text-secondary">
                  {t("settings.about.deletePhraseHint", {
                    phrase: t("settings.about.deletePhrase"),
                  })}
                </p>
                <input
                  type="text"
                  value={deletePhrase}
                  onChange={(e) => setDeletePhrase(e.target.value)}
                  placeholder={t("settings.about.deletePhrase")}
                  autoFocus
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && deletePhrase === t("settings.about.deletePhrase")) {
                      confirmWipe();
                    } else if (e.key === "Escape") {
                      setDeleteDialogOpen(false);
                    }
                  }}
                  className="mt-2 w-full rounded border border-border bg-surface-2 px-2 py-1.5 font-mono text-sm focus:border-status-failed focus:outline-none"
                />
                <div className="mt-4 flex items-center justify-end gap-2">
                  <button
                    type="button"
                    onClick={() => setDeleteDialogOpen(false)}
                    className="rounded-md border border-border bg-surface-2 px-3 py-1.5 text-xs text-text-secondary hover:text-primary"
                  >
                    {t("settings.action.close")}
                  </button>
                  <button
                    type="button"
                    disabled={deleteBusy || deletePhrase !== t("settings.about.deletePhrase")}
                    onClick={confirmWipe}
                    className="rounded-md border border-status-failed/40 bg-status-failed px-3 py-1.5 text-xs font-medium text-white hover:bg-status-failed/90 disabled:cursor-not-allowed disabled:opacity-40"
                  >
                    {deleteBusy ? t("settings.action.save") : t("settings.about.clearData")}
                  </button>
                </div>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
