<script lang="ts">
  import { Clock3, TerminalSquare } from "@lucide/svelte";
  import type { ActivityProcess } from "$lib/types";

  interface Props {
    processes: ActivityProcess[];
    now: number;
  }

  let { processes, now }: Props = $props();

  function age(process: ActivityProcess): string {
    const seconds = Math.max(0, Math.floor((now - process.updatedAtMs) / 1000));
    if (seconds < 60) return `${seconds}s`;
    if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
    return `${Math.floor(seconds / 3600)}h`;
  }

  function statusLabel(status: string): string {
    if (status === "running") return "等待终态";
    if (status === "terminating") return "正在终止";
    return status || "状态未知";
  }
</script>

<section class="process-panel" aria-label="后台命令">
  <header>
    <div>
      <h3><TerminalSquare size={15} />后台命令</h3>
      <p>活动关联 {processes.length} / 100</p>
    </div>
    <span class:active={processes.length > 0}>{processes.length > 0 ? "等待终态" : "无活动会话"}</span>
  </header>

  {#if processes.length === 0}
    <div class="process-empty">当前没有等待终态的后台命令</div>
  {:else}
    <div class="process-list">
      {#each processes as process (process.sessionId)}
        <article>
          <div class="process-main">
            <strong class="tx-mono" title={process.command}>{process.command || "未记录命令"}</strong>
            <span>{process.workspaceName || "未绑定工作区"}</span>
          </div>
          <div class="process-meta">
            <span class="process-state"><i></i>{statusLabel(process.status)}</span>
            <span title="距离最近状态更新"><Clock3 size={12} />{age(process)}</span>
            <code title={process.sessionId}>{process.sessionId}</code>
          </div>
        </article>
      {/each}
    </div>
  {/if}
</section>

<style>
  .process-panel { min-width: 0; border: 1px solid var(--border); border-radius: 8px; background: var(--card-bg); overflow: hidden; }
  header { min-height: 52px; padding: 10px 12px; display: flex; align-items: center; justify-content: space-between; gap: 12px; border-bottom: 1px solid var(--border); }
  header > div { min-width: 0; }
  h3 { margin: 0; display: flex; align-items: center; gap: 7px; color: var(--text-main); font-size: 13px; }
  p { margin: 3px 0 0; color: var(--text-muted); font-size: 10px; }
  header > span { color: var(--text-muted); font-size: 11px; white-space: nowrap; }
  header > span.active { color: var(--warning); }
  .process-empty { min-height: 88px; display: flex; align-items: center; justify-content: center; padding: 16px; color: var(--text-muted); font-size: 12px; text-align: center; }
  .process-list { max-height: 210px; overflow: auto; }
  article { padding: 10px 12px; display: grid; gap: 7px; border-bottom: 1px solid var(--border); }
  article:last-child { border-bottom: 0; }
  .process-main, .process-meta { min-width: 0; display: flex; align-items: center; justify-content: space-between; gap: 10px; }
  .process-main strong { min-width: 0; overflow: hidden; color: var(--text-main); font-size: 11px; text-overflow: ellipsis; white-space: nowrap; }
  .process-main > span { flex: 0 0 auto; color: var(--text-secondary); font-size: 10px; }
  .process-meta { justify-content: flex-start; color: var(--text-muted); font-size: 10px; }
  .process-meta > span { display: inline-flex; align-items: center; gap: 4px; white-space: nowrap; }
  .process-state { color: var(--warning); }
  .process-state i { width: 6px; height: 6px; border-radius: 50%; background: var(--warning); }
  code { min-width: 0; margin-left: auto; overflow: hidden; color: var(--text-muted); font-size: 9px; text-overflow: ellipsis; white-space: nowrap; }
</style>
