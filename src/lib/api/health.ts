import { invoke } from "@tauri-apps/api/core";

export type HealthStatus = "pass" | "warn" | "skip" | "fail";

export interface HealthItem {
  key: string;
  layer: "config" | "local" | "public" | "oauth" | "handshake";
  status: HealthStatus;
  traceId: string;
  retryable: boolean;
  label: string;
  ok: boolean;
  detail: string;
  hint: string;
}

export async function runHealthChecks(workspaceId: string): Promise<HealthItem[]> {
  return invoke<HealthItem[]>("run_health_checks", { id: workspaceId });
}
