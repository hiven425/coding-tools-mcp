import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ActivityEvent,
  ActivityFilters,
  ActivitySnapshot,
  ActivityTrace,
} from "$lib/types";

export function listActivity(filters: ActivityFilters = {}): Promise<ActivitySnapshot> {
  return invoke<ActivitySnapshot>("list_activity", {
    workspace: filters.workspace ?? null,
    tool: filters.tool ?? null,
    status: filters.status ?? null,
    limit: filters.limit ?? 200,
  });
}

export function getActivity(traceId: string): Promise<ActivityTrace | null> {
  return invoke<ActivityTrace | null>("get_activity", { traceId });
}

export function clearActivity(): Promise<number> {
  return invoke<number>("clear_activity");
}

export function listenActivityEvents(
  handler: (event: ActivityEvent) => void,
): Promise<UnlistenFn> {
  return listen<ActivityEvent>("activity://event", ({ payload }) => handler(payload));
}
