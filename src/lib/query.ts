import { QueryClient } from "@tanstack/react-query";

export const queryKeys = {
  appStatus: ["app-status"] as const,
  instances: ["instances"] as const,
  history: (instanceId: string | null) => ["history", instanceId ?? "all"] as const,
  historyAll: ["history"] as const,
  settings: ["settings"] as const,
  driveStatus: ["drive-status"] as const,
};

export function createQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
        refetchOnWindowFocus: false,
        staleTime: 30_000,
      },
      mutations: { retry: false },
    },
  });
}
