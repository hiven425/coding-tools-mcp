<script lang="ts">
  import { RefreshCw, ShieldCheck } from "@lucide/svelte";
  import { runHealthChecks, type HealthItem } from "$lib/api/health";
  import { getRuntimeStatus } from "$lib/api/workspaces";
  import type { RuntimeStatus, WorkspaceProfile } from "$lib/types";

  interface Props {
    workspaces: WorkspaceProfile[];
  }

  interface ServiceState {
    runtime?: RuntimeStatus;
    health?: HealthItem[];
    checkedAtMs?: number;
    checking?: boolean;
    listenerError?: string;
    healthError?: string;
  }

  let { workspaces }: Props = $props();
  let states = $state<Record<string, ServiceState>>({});
  let listenerBusy = $state(false);
  let loadedKey = $state("");
  let requestSerial = 0;

  async function refreshListeners() {
    const serial = ++requestSerial;
    listenerBusy = true;
    const entries = await Promise.all(
      workspaces.map(async (workspace) => {
        try {
          return [workspace.id, await getRuntimeStatus(workspace.id), ""] as const;
        } catch (error) {
          return [workspace.id, undefined, String(error)] as const;
        }
      }),
    );
    if (serial !== requestSerial) return;
    const next = { ...states };
    for (const [id, runtime, error] of entries) {
      next[id] = { ...next[id], runtime, listenerError: error };
    }
    states = next;
    listenerBusy = false;
  }

  async function verify(workspaceId: string) {
    if (states[workspaceId]?.checking) return;
    states = {
      ...states,
      [workspaceId]: { ...states[workspaceId], checking: true, healthError: "" },
    };
    try {
      const health = await runHealthChecks(workspaceId);
      states = {
        ...states,
        [workspaceId]: {
          ...states[workspaceId],
          health,
          checkedAtMs: Date.now(),
          checking: false,
        },
      };
    } catch (error) {
      states = {
        ...states,
        [workspaceId]: {
          ...states[workspaceId],
          checking: false,
          healthError: String(error),
        },
      };
    }
  }

  function listenerLabel(runtime?: RuntimeStatus): string {
    if (!runtime) return "状态未知";
    if (runtime.state === "running") return "正在监听";
    if (runtime.state === "starting") return "正在启动";
    if (runtime.state === "stopping") return "正在停止";
    if (runtime.state === "error") return "监听异常";
    return "已停止";
  }

  function publicLabel(runtime?: RuntimeStatus): string {
    if (!runtime) return "状态未知";
    if (runtime.publicState === "public-ready") return "隧道已注册";
    if (runtime.publicState === "not-configured") return "未配置";
    if (runtime.publicState === "public-starting") return "正在注册";
    if (runtime.publicState === "public-degraded") return "连接降级";
    if (runtime.publicState === "public-error") return "连接异常";
    return "未运行";
  }

  function verificationLabel(state?: ServiceState): string {
    if (state?.checking) return "验证中";
    if (!state?.health) return "尚未验证";
    if (state.health.some((item) => item.status === "fail")) return "握手失败";
    if (state.health.some((item) => item.status === "warn")) return "握手需认证";
    return "握手通过";
  }

  function verificationDetail(state?: ServiceState): string {
    if (state?.healthError) return state.healthError;
    const issue = state?.health?.find((item) => item.status === "fail" || item.status === "warn");
    if (issue) return issue.detail || issue.hint;
    if (state?.checkedAtMs) {
      return new Date(state.checkedAtMs).toLocaleTimeString("zh-CN", {
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
      });
    }
    return "";
  }

  $effect(() => {
    const key = workspaces.map((workspace) => workspace.id).join("|");
    if (key === loadedKey) return;
    loadedKey = key;
    void refreshListeners();
  });
</script>

<section class="service-health" aria-label="MCP 服务状态">
  <header>
    <div>
      <h3><ShieldCheck size={15} />服务真实性</h3>
      <p>监听器、隧道、MCP 协议</p>
    </div>
    <button
      type="button"
      title="刷新监听器状态"
      aria-label="刷新监听器状态"
      disabled={listenerBusy}
      onclick={() => void refreshListeners()}
    >
      <RefreshCw size={15} class={listenerBusy ? "animate-spin" : ""} />
    </button>
  </header>

  {#if workspaces.length === 0}
    <div class="service-empty">暂无工作区</div>
  {:else}
    <div class="service-table-wrap">
      <table>
        <thead><tr><th>工作区</th><th>监听器</th><th>公网隧道</th><th>MCP 握手</th><th></th></tr></thead>
        <tbody>
          {#each workspaces as workspace (workspace.id)}
            {@const state = states[workspace.id]}
            <tr>
              <td><strong>{workspace.name}</strong></td>
              <td>
                <span class:ok={state?.runtime?.state === "running"} class:error={state?.runtime?.state === "error"}>
                  <i></i>{listenerLabel(state?.runtime)}
                </span>
                {#if state?.listenerError}<small title={state.listenerError}>{state.listenerError}</small>{/if}
              </td>
              <td>
                <span class:ok={state?.runtime?.publicState === "public-ready"} class:error={state?.runtime?.publicState === "public-error"}>
                  <i></i>{publicLabel(state?.runtime)}
                </span>
              </td>
              <td>
                <span
                  class:ok={state?.health && !state.health.some((item) => item.status === "fail" || item.status === "warn")}
                  class:warn={state?.health?.some((item) => item.status === "warn")}
                  class:error={state?.health?.some((item) => item.status === "fail")}
                >
                  <i></i>{verificationLabel(state)}
                </span>
                {#if verificationDetail(state)}<small title={verificationDetail(state)}>{verificationDetail(state)}</small>{/if}
              </td>
              <td>
                <button
                  type="button"
                  class="verify-button"
                  disabled={state?.checking}
                  onclick={() => void verify(workspace.id)}
                >
                  <ShieldCheck size={13} />验证协议
                </button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</section>

<style>
  .service-health { min-width: 0; border: 1px solid var(--border); border-radius: 8px; background: var(--card-bg); overflow: hidden; }
  header { min-height: 52px; padding: 10px 12px; display: flex; align-items: center; justify-content: space-between; gap: 12px; border-bottom: 1px solid var(--border); }
  h3 { margin: 0; display: flex; align-items: center; gap: 7px; color: var(--text-main); font-size: 13px; }
  p { margin: 3px 0 0; color: var(--text-muted); font-size: 10px; }
  header button { width: 32px; height: 32px; display: inline-flex; align-items: center; justify-content: center; border: 1px solid var(--border); border-radius: 7px; background: transparent; color: var(--text-secondary); cursor: pointer; }
  header button:hover:not(:disabled) { color: var(--text-main); background: var(--surface-hover); }
  button:disabled { opacity: .45; cursor: default; }
  .service-empty { min-height: 88px; display: flex; align-items: center; justify-content: center; color: var(--text-muted); font-size: 12px; }
  .service-table-wrap { max-height: 210px; overflow: auto; }
  table { width: 100%; min-width: 650px; border-collapse: collapse; table-layout: fixed; }
  th { padding: 7px 9px; border-bottom: 1px solid var(--border); background: var(--page-bg); color: var(--text-muted); font-size: 9px; text-align: left; }
  th:first-child { width: 20%; } th:last-child { width: 96px; }
  td { padding: 9px; border-bottom: 1px solid var(--border); color: var(--text-secondary); font-size: 10px; overflow: hidden; }
  tr:last-child td { border-bottom: 0; }
  td strong, td small { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  td strong { color: var(--text-main); font-size: 11px; }
  td span { display: inline-flex; align-items: center; gap: 5px; white-space: nowrap; }
  td span i { width: 6px; height: 6px; flex: 0 0 6px; border-radius: 50%; background: var(--text-muted); }
  td span.ok { color: var(--success); } td span.ok i { background: var(--success); }
  td span.warn { color: var(--warning); } td span.warn i { background: var(--warning); }
  td span.error { color: var(--danger); } td span.error i { background: var(--danger); }
  td small { max-width: 180px; margin-top: 3px; color: var(--text-muted); font-size: 9px; }
  .verify-button { min-height: 28px; padding: 5px 7px; display: inline-flex; align-items: center; gap: 5px; border: 1px solid var(--border); border-radius: 7px; background: transparent; color: var(--text-secondary); font-size: 10px; cursor: pointer; white-space: nowrap; }
  .verify-button:hover:not(:disabled) { color: var(--primary); border-color: color-mix(in srgb, var(--primary) 35%, var(--border)); }
</style>
