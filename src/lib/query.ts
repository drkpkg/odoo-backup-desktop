import { QueryClient } from "@tanstack/react-query";

export const queryKeys = {
  appStatus: ["app-status"] as const,
  instances: ["instances"] as const,
  history: (instanceId: string | null) => ["history", instanceId ?? "all"] as const,
  historyAll: ["history"] as const,
  settings: ["settings"] as const,
  driveStatus: ["drive-status"] as const,
  plugins: ["plugins"] as const,
  pluginConfig: ["plugin-config"] as const,
  pluginSettings: (pluginId: string) => ["plugin-settings", pluginId] as const,
  pluginSettingsAll: ["plugin-settings"] as const,
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
