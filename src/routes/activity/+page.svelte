<script lang="ts">
  import { onMount } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import {
    Activity as ActivityIcon,
    CheckCircle2,
    CircleAlert,
    Filter,
    FilterX,
    LoaderCircle,
    RefreshCw,
    Trash2,
  } from "@lucide/svelte";
  import { clearActivity, getActivity, listActivity } from "$lib/api/activity";
  import { workspaces } from "$lib/stores/app";
  import { showToast } from "$lib/stores/toast";
  import type { ActivitySnapshot, ActivityTrace } from "$lib/types";

  const EMPTY_SNAPSHOT: ActivitySnapshot = {
    traces: [],
    totalMatching: 0,
    retained: 0,
    maxEntries: 500,
  };

  let snapshot = $state<ActivitySnapshot>(EMPTY_SNAPSHOT);
  let selected = $state<ActivityTrace | null>(null);
  let loading = $state(true);
  let refreshing = $state(false);
  let now = $state(Date.now());
  let workspaceFilter = $state("");
  let toolFilter = $state("");
  let statusFilter = $state("");

  const runningCount = $derived(snapshot.traces.filter((trace) => trace.status === "running").length);
  const failedCount = $derived(snapshot.traces.filter((trace) => trace.status === "failed").length);

  async function load(reportError = true) {
    if (refreshing) return;
    refreshing = true;
    try {
      snapshot = await listActivity({
        workspace: workspaceFilter,
        tool: toolFilter,
        status: statusFilter,
        limit: 250,
      });
      if (selected) {
        selected = await getActivity(selected.traceId);
      }
    } catch (error) {
      if (reportError) {
        showToast(String(error), { title: "读取活动失败", kind: "error", duration: 6000 });
      }
    } finally {
      refreshing = false;
      loading = false;
    }
  }

  async function selectTrace(trace: ActivityTrace) {
    try {
      selected = (await getActivity(trace.traceId)) ?? trace;
    } catch {
      selected = trace;
    }
  }

  function resetFilters() {
    workspaceFilter = "";
    toolFilter = "";
    statusFilter = "";
    void load();
  }

  async function clearAll() {
    const confirmed = await ask("清空当前进程中保留的 MCP 活动记录？", {
      title: "清空活动记录",
      kind: "warning",
      okLabel: "清空",
      cancelLabel: "取消",
    });
    if (!confirmed) return;
    try {
      const removed = await clearActivity();
      snapshot = { ...EMPTY_SNAPSHOT };
      selected = null;
      showToast(`已清空 ${removed} 条活动记录。`, { title: "活动记录", kind: "success" });
    } catch (error) {
      showToast(String(error), { title: "清空失败", kind: "error" });
    }
  }

  function durationMs(trace: ActivityTrace): number {
    return trace.durationMs ?? Math.max(0, now - trace.startedAtMs);
  }

  function formatDuration(value: number): string {
    if (value < 1000) return `${value} ms`;
    if (value < 60_000) return `${(value / 1000).toFixed(1)} s`;
    return `${Math.floor(value / 60_000)}m ${Math.floor((value % 60_000) / 1000)}s`;
  }

  function formatTime(value: number): string {
    return new Date(value).toLocaleTimeString("zh-CN", {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  }

  function formatJson(value: unknown): string {
    if (value === null || value === undefined) return "";
    try {
      return JSON.stringify(value, null, 2);
    } catch {
      return String(value);
    }
  }

  function statusLabel(status: string): string {
    if (status === "running") return "运行中";
    if (status === "completed") return "已完成";
    if (status === "failed") return "失败";
    return status || "未知";
  }

  onMount(() => {
    void load();
    const onVisibility = () => {
      if (!document.hidden) void load(false);
    };
    document.addEventListener("visibilitychange", onVisibility);
    const timer = window.setInterval(() => {
      now = Date.now();
      if (!document.hidden) void load(false);
    }, 5_000);
    return () => {
      document.removeEventListener("visibilitychange", onVisibility);
      window.clearInterval(timer);
    };
  });
</script>

<section class="page-scroll activity-page">
  <header class="page-header activity-header">
    <div class="activity-heading">
      <p class="page-kicker">运行诊断</p>
      <div class="activity-title-row">
        <h2 class="page-title">MCP 活动</h2>
        <span class="activity-live"><span></span>{runningCount > 0 ? `${runningCount} 运行中` : "已就绪"}</span>
      </div>
    </div>
    <div class="activity-actions">
      <button
        type="button"
        class="activity-icon-button"
        title="刷新活动"
        aria-label="刷新活动"
        disabled={refreshing}
        onclick={() => void load()}
      >
        <RefreshCw size={17} class={refreshing ? "animate-spin" : ""} />
      </button>
      <button
        type="button"
        class="activity-icon-button danger"
        title="清空活动记录"
        aria-label="清空活动记录"
        disabled={snapshot.retained === 0}
        onclick={() => void clearAll()}
      >
        <Trash2 size={17} />
      </button>
    </div>
  </header>

  <div class="page-body activity-body">
    <div class="activity-summary" aria-label="活动摘要">
      <span><ActivityIcon size={15} />保留 <strong>{snapshot.retained}</strong> / {snapshot.maxEntries}</span>
      <span><LoaderCircle size={15} />运行 <strong>{runningCount}</strong></span>
      <span class:has-failures={failedCount > 0}><CircleAlert size={15} />失败 <strong>{failedCount}</strong></span>
      <span><CheckCircle2 size={15} />匹配 <strong>{snapshot.totalMatching}</strong></span>
    </div>

    <form
      class="activity-filters"
      onsubmit={(event) => {
        event.preventDefault();
        void load();
      }}
    >
      <label>
        <span>工作区</span>
        <select class="tx-select" bind:value={workspaceFilter}>
          <option value="">全部工作区</option>
          {#each $workspaces as workspace (workspace.id)}
            <option value={workspace.id}>{workspace.name}</option>
          {/each}
        </select>
      </label>
      <label>
        <span>工具</span>
        <input class="tx-input tx-mono" bind:value={toolFilter} placeholder="exec_command" />
      </label>
      <label>
        <span>状态</span>
        <select class="tx-select" bind:value={statusFilter}>
          <option value="">全部状态</option>
          <option value="running">运行中</option>
          <option value="completed">已完成</option>
          <option value="failed">失败</option>
        </select>
      </label>
      <button type="submit" class="tx-btn-primary"><Filter size={15} />筛选</button>
      <button
        type="button"
        class="activity-icon-button"
        title="重置筛选"
        aria-label="重置筛选"
        onclick={resetFilters}
      >
        <FilterX size={17} />
      </button>
    </form>

    <div class="activity-console">
      <section class="activity-list" aria-label="最近 MCP 调用">
        <div class="activity-section-header">
          <h3>最近调用</h3>
          <span>{snapshot.traces.length} 条</span>
        </div>
        <div class="activity-table-wrap">
          {#if loading}
            <div class="activity-empty"><LoaderCircle size={18} class="animate-spin" />正在读取</div>
          {:else if snapshot.traces.length === 0}
            <div class="activity-empty">暂无匹配记录</div>
          {:else}
            <table class="activity-table">
              <thead>
                <tr><th>状态</th><th>工具 / 方法</th><th>工作区</th><th>开始</th><th>耗时</th></tr>
              </thead>
              <tbody>
                {#each snapshot.traces as trace (trace.traceId)}
                  <tr
                    class:selected={selected?.traceId === trace.traceId}
                    onclick={() => void selectTrace(trace)}
                    onkeydown={(event) => {
                      if (event.key === "Enter" || event.key === " ") void selectTrace(trace);
                    }}
                    tabindex="0"
                  >
                    <td><span class="activity-status {trace.status}"><i></i>{statusLabel(trace.status)}</span></td>
                    <td>
                      <strong class="activity-tool">{trace.tool || trace.method || "未命名调用"}</strong>
                      <small class="tx-mono">{trace.rpcId || trace.traceId}</small>
                    </td>
                    <td><span class="activity-workspace">{trace.workspaceName || trace.workspaceId || "未绑定"}</span></td>
                    <td class="tx-mono">{formatTime(trace.startedAtMs)}</td>
                    <td class="tx-mono">{formatDuration(durationMs(trace))}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          {/if}
        </div>
      </section>

      <aside class="activity-detail" aria-label="调用详情">
        {#if selected}
          <div class="activity-section-header detail-heading">
            <div>
              <h3>{selected.tool || selected.method || "调用详情"}</h3>
              <span class="tx-mono">{selected.traceId}</span>
            </div>
            <span class="activity-status {selected.status}"><i></i>{statusLabel(selected.status)}</span>
          </div>
          <dl class="activity-meta">
            <div><dt>工作区</dt><dd>{selected.workspaceName || selected.workspaceId || "未绑定"}</dd></div>
            <div><dt>路由</dt><dd class="tx-mono">{selected.route}</dd></div>
            <div><dt>开始</dt><dd class="tx-mono">{formatTime(selected.startedAtMs)}</dd></div>
            <div><dt>耗时</dt><dd class="tx-mono">{formatDuration(durationMs(selected))}</dd></div>
          </dl>
          <div class="activity-payloads">
            <section>
              <h4>Request</h4>
              <pre>{formatJson(selected.request)}</pre>
            </section>
            <section>
              <h4>Response</h4>
              <pre>{formatJson(selected.response)}</pre>
            </section>
            {#if selected.status === "failed"}
              <section>
                <h4>Error</h4>
                <pre>{formatJson(selected.error)}</pre>
              </section>
            {/if}
          </div>
        {:else}
          <div class="activity-empty detail-empty"><ActivityIcon size={22} />选择一条调用查看详情</div>
        {/if}
      </aside>
    </div>
  </div>
</section>

<style>
  .activity-page { min-width: 0; }
  .activity-header { display: flex; align-items: center; justify-content: space-between; gap: 16px; }
  .activity-title-row, .activity-actions, .activity-summary, .activity-filters { display: flex; align-items: center; }
  .activity-title-row { gap: 12px; }
  .activity-live { display: inline-flex; align-items: center; gap: 6px; color: var(--text-secondary); font-size: 12px; }
  .activity-live span { width: 7px; height: 7px; border-radius: 50%; background: var(--success); box-shadow: 0 0 0 3px color-mix(in srgb, var(--success) 16%, transparent); }
  .activity-actions { gap: 8px; }
  .activity-icon-button { width: 36px; height: 36px; display: inline-flex; align-items: center; justify-content: center; flex: 0 0 36px; border: 1px solid var(--border); border-radius: 8px; background: var(--card-bg); color: var(--text-secondary); cursor: pointer; }
  .activity-icon-button:hover:not(:disabled) { color: var(--text-main); background: var(--surface-hover); }
  .activity-icon-button.danger:hover:not(:disabled) { color: var(--danger); border-color: color-mix(in srgb, var(--danger) 35%, var(--border)); }
  .activity-icon-button:disabled { opacity: 0.45; cursor: default; }
  .activity-body { display: grid; gap: 14px; min-width: 0; }
  .activity-summary { gap: 20px; flex-wrap: wrap; min-height: 26px; color: var(--text-secondary); font-size: 12px; }
  .activity-summary span { display: inline-flex; align-items: center; gap: 6px; }
  .activity-summary strong { color: var(--text-main); font-variant-numeric: tabular-nums; }
  .activity-summary .has-failures { color: var(--danger); }
  .activity-filters { display: grid; grid-template-columns: minmax(150px, 1fr) minmax(170px, 1fr) minmax(130px, .65fr) auto auto; gap: 10px; padding: 12px; border: 1px solid var(--border); border-radius: 8px; background: var(--card-bg); }
  .activity-filters label { display: grid; gap: 5px; min-width: 0; }
  .activity-filters label > span { color: var(--text-secondary); font-size: 11px; font-weight: 600; }
  .activity-filters .tx-input, .activity-filters .tx-select { min-height: 36px; padding: 7px 9px; border-radius: 7px; }
  .activity-filters .tx-btn-primary { align-self: end; min-height: 36px; border-radius: 8px; }
  .activity-filters > .activity-icon-button { align-self: end; }
  .activity-console { display: grid; grid-template-columns: minmax(520px, 1.35fr) minmax(330px, .85fr); min-height: 520px; max-height: calc(100vh - 310px); border: 1px solid var(--border); border-radius: 8px; overflow: hidden; background: var(--card-bg); }
  .activity-list, .activity-detail { display: flex; flex-direction: column; min-width: 0; min-height: 0; }
  .activity-list { border-right: 1px solid var(--border); }
  .activity-section-header { min-height: 50px; padding: 12px 14px; display: flex; align-items: center; justify-content: space-between; gap: 12px; border-bottom: 1px solid var(--border); }
  .activity-section-header h3 { margin: 0; font-size: 13px; font-weight: 700; }
  .activity-section-header > span, .detail-heading > div > span { color: var(--text-muted); font-size: 11px; }
  .activity-table-wrap { flex: 1; min-height: 0; overflow: auto; }
  .activity-table { width: 100%; min-width: 700px; border-collapse: collapse; table-layout: fixed; }
  .activity-table th { position: sticky; top: 0; z-index: 1; padding: 8px 10px; background: var(--page-bg); border-bottom: 1px solid var(--border); color: var(--text-muted); font-size: 10px; font-weight: 700; text-align: left; }
  .activity-table th:nth-child(1) { width: 92px; } .activity-table th:nth-child(2) { width: 220px; } .activity-table th:nth-child(3) { width: 150px; } .activity-table th:nth-child(4) { width: 86px; } .activity-table th:nth-child(5) { width: 78px; }
  .activity-table td { padding: 10px; border-bottom: 1px solid var(--border); color: var(--text-secondary); font-size: 12px; vertical-align: middle; overflow: hidden; }
  .activity-table tbody tr { cursor: pointer; transition: background 120ms ease; }
  .activity-table tbody tr:hover, .activity-table tbody tr.selected { background: var(--surface-hover); }
  .activity-table tbody tr.selected { box-shadow: inset 3px 0 var(--primary); }
  .activity-tool, .activity-workspace, .activity-table small { display: block; max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .activity-tool { color: var(--text-main); font-size: 12px; }
  .activity-table small { margin-top: 2px; color: var(--text-muted); }
  .activity-status { display: inline-flex; align-items: center; gap: 5px; min-width: 0; white-space: nowrap; color: var(--text-secondary); font-size: 11px; }
  .activity-status i { width: 7px; height: 7px; flex: 0 0 7px; border-radius: 50%; background: var(--text-muted); }
  .activity-status.running i { background: var(--warning); } .activity-status.completed i { background: var(--success); } .activity-status.failed { color: var(--danger); } .activity-status.failed i { background: var(--danger); }
  .activity-detail { overflow: hidden; }
  .detail-heading > div { min-width: 0; }
  .detail-heading h3, .detail-heading > div > span { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .activity-meta { margin: 0; padding: 12px 14px; display: grid; grid-template-columns: 1fr 1fr; gap: 10px; border-bottom: 1px solid var(--border); }
  .activity-meta div { min-width: 0; }
  .activity-meta dt { color: var(--text-muted); font-size: 10px; font-weight: 700; }
  .activity-meta dd { margin: 2px 0 0; overflow: hidden; color: var(--text-main); font-size: 12px; text-overflow: ellipsis; white-space: nowrap; }
  .activity-payloads { flex: 1; min-height: 0; overflow: auto; padding: 14px; display: grid; align-content: start; gap: 14px; }
  .activity-payloads section { min-width: 0; }
  .activity-payloads h4 { margin: 0 0 6px; color: var(--text-secondary); font-size: 11px; font-weight: 700; }
  .activity-payloads pre { max-height: 230px; margin: 0; padding: 10px; overflow: auto; border: 1px solid var(--border); border-radius: 6px; background: var(--page-bg); color: var(--text-main); font-family: "Cascadia Code", Consolas, monospace; font-size: 11px; line-height: 1.55; white-space: pre-wrap; overflow-wrap: anywhere; }
  .activity-empty { min-height: 180px; display: flex; align-items: center; justify-content: center; gap: 8px; color: var(--text-muted); font-size: 12px; }
  .detail-empty { flex: 1; min-height: 300px; flex-direction: column; }
  @media (max-width: 1100px) { .activity-console { grid-template-columns: 1fr; max-height: none; } .activity-list { min-height: 430px; border-right: 0; border-bottom: 1px solid var(--border); } .activity-detail { min-height: 430px; } }
  @media (max-width: 760px) { .activity-header { align-items: flex-start; } .activity-filters { grid-template-columns: 1fr 1fr; } .activity-filters label:first-child { grid-column: 1 / -1; } .activity-console { min-height: 0; } .activity-body { padding: 16px; } }
</style>
