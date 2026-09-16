import { useQuery } from "@tanstack/react-query";

import { Alert } from "../../components/Alert";
import { PageHeader } from "../../components/Layout";
import { Spinner } from "../../components/Spinner";
import { errorMessage } from "../../lib/errors";
import { ipc } from "../../lib/ipc";
import { queryKeys } from "../../lib/query";
import type { AppStatus } from "../../lib/types";
import { DriveSection } from "./DriveSection";
import { GeneralSection } from "./GeneralSection";
import { SecuritySection } from "./SecuritySection";

export function SettingsPage({ status }: { status: AppStatus }) {
  const settings = useQuery({ queryKey: queryKeys.settings, queryFn: () => ipc.getSettings() });

  return (
    <>
      <PageHeader title="Ajustes" description="Carpeta de descarga, retención, seguridad de la bóveda y Google Drive." />
      <div className="mx-auto max-w-3xl space-y-5 px-6 py-5">
        {settings.isPending ? <Spinner label="Cargando ajustes…" /> : null}
        {settings.isError ? <Alert tone="danger">{errorMessage(settings.error)}</Alert> : null}
        {settings.data ? (
          <>
            <GeneralSection settings={settings.data} />
            <SecuritySection status={status} />
            <DriveSection settings={settings.data} />
          </>
        ) : null}
      </div>
    </>
  );
}
