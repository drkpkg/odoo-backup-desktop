import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState, type FormEvent } from "react";

import { Alert } from "../../components/Alert";
import { Button } from "../../components/Button";
import { Field, PasswordInput, Select, Switch, TextInput } from "../../components/Field";
import { useToast } from "../../components/Toast";
import { errorMessage } from "../../lib/errors";
import { ipc } from "../../lib/ipc";
import { queryKeys } from "../../lib/query";
import type { PluginSettings, SchemaProperty } from "../../lib/types";
import { useReportDirty } from "../settings/sections";
import {
  buildSubmission,
  fieldKind,
  FORM_ERROR_KEY,
  initialDraft,
  isRequired,
  orderedKeys,
  parseServerErrors,
  validateDraft,
  type Draft,
  type FieldErrors,
} from "./settingsSchema";

const TEXTAREA =
  "min-h-24 w-full rounded-md border border-border-strong bg-surface px-2.5 py-2 text-sm text-fg placeholder:text-subtle transition-colors hover:border-subtle focus:border-accent focus:outline-none focus:ring-2 focus:ring-accent/25 aria-[invalid=true]:border-danger";

/** Formulario generado desde el esquema de ajustes de un plugin. */
export function SchemaForm({ pluginId, settings }: { pluginId: string; settings: PluginSettings }) {
  const queryClient = useQueryClient();
  const toast = useToast();
  const { schema } = settings;
  const keys = useMemo(() => orderedKeys(schema), [schema]);

  const [draft, setDraft] = useState<Draft>(() => initialDraft(schema, settings));
  const [removed, setRemoved] = useState<string[]>([]);
  const [errors, setErrors] = useState<FieldErrors>({});
  const [saving, setSaving] = useState(false);
  const dirty = removed.length > 0 || JSON.stringify(draft) !== JSON.stringify(initialDraft(schema, settings));
  useReportDirty("plugins", pluginId, dirty);

  // Nuevos datos del backend (guardado o recarga): reiniciar el borrador y descartar secretos escritos.
  useEffect(() => {
    setDraft(initialDraft(schema, settings));
    setRemoved([]);
    setErrors({});
  }, [schema, settings]);

  const update = (key: string, value: string | boolean) => {
    setDraft((current) => ({ ...current, [key]: value }));
    setErrors((current) => {
      if (!(key in current)) return current;
      const next = { ...current };
      delete next[key];
      return next;
    });
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const clientErrors = validateDraft(schema, draft, settings.secretsSet, removed);
    setErrors(clientErrors);
    if (Object.keys(clientErrors).length > 0) return;
    setSaving(true);
    try {
      const next = await ipc.savePluginSettings(pluginId, buildSubmission(schema, draft, removed));
      queryClient.setQueryData(queryKeys.pluginSettings(pluginId), next);
      toast.success("Ajustes del plugin guardados");
    } catch (err) {
      const fieldErrors = parseServerErrors(err, schema);
      if (fieldErrors) setErrors(fieldErrors);
      else toast.error("No se pudieron guardar los ajustes", errorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  if (keys.length === 0) return <p className="text-[13px] text-muted">Este plugin no declara ajustes editables.</p>;

  return (
    <form onSubmit={submit} noValidate className="space-y-4">
      {schema.description ? <p className="text-[13px] text-muted">{schema.description}</p> : null}
      {errors[FORM_ERROR_KEY] ? <Alert tone="danger">{errors[FORM_ERROR_KEY]}</Alert> : null}
      {keys.map((key) => {
        const prop = schema.properties[key];
        if (!prop) return null;
        return (
          <SchemaField
            key={key}
            name={key}
            prop={prop}
            required={isRequired(schema, key)}
            value={draft[key]}
            error={errors[key]}
            secretSet={settings.secretsSet.includes(key)}
            removed={removed.includes(key)}
            onChange={(value) => update(key, value)}
            onToggleRemove={(remove) =>
              setRemoved((current) => (remove ? [...current.filter((k) => k !== key), key] : current.filter((k) => k !== key)))
            }
          />
        );
      })}
      <div className="flex justify-end gap-2 pt-1">
        <Button
          type="button"
          variant="ghost"
          disabled={saving}
          onClick={() => {
            setDraft(initialDraft(schema, settings));
            setRemoved([]);
            setErrors({});
          }}
        >
          Descartar cambios
        </Button>
        <Button type="submit" variant="primary" loading={saving}>
          Guardar ajustes
        </Button>
      </div>
    </form>
  );
}

function SchemaField({
  name,
  prop,
  required,
  value,
  error,
  secretSet,
  removed,
  onChange,
  onToggleRemove,
}: {
  name: string;
  prop: SchemaProperty;
  required: boolean;
  value: string | boolean | undefined;
  error: string | undefined;
  secretSet: boolean;
  removed: boolean;
  onChange: (value: string | boolean) => void;
  onToggleRemove: (remove: boolean) => void;
}) {
  const kind = fieldKind(prop);
  const label = prop.title ?? name;
  const text = typeof value === "string" ? value : "";

  if (kind === "boolean") {
    return (
      <div className="space-y-1">
        <Switch checked={value === true} onChange={onChange} label={label} description={prop.description} />
        {error ? <p className="text-xs text-danger">{error}</p> : null}
      </div>
    );
  }

  const labelNode = (
    <>
      {label}
      {required ? (
        <span className="text-danger" aria-hidden="true">
          *
        </span>
      ) : null}
    </>
  );

  return (
    <Field label={labelNode} hint={prop.description} error={error}>
      {({ id, describedBy, invalid }) => {
        const common = {
          id,
          "aria-describedby": describedBy,
          "aria-invalid": invalid || undefined,
          "aria-required": required || undefined,
        };
        switch (kind) {
          case "secret":
            return (
              <div className="space-y-1.5">
                <PasswordInput
                  {...common}
                  value={text}
                  disabled={removed}
                  placeholder={removed ? "Se quitará al guardar" : secretSet ? "•••••• (sin cambios)" : (prop.placeholder ?? "")}
                  onChange={(event) => onChange(event.target.value)}
                />
                {secretSet ? (
                  <label className="flex w-fit cursor-pointer items-center gap-2 text-xs text-muted">
                    <input
                      type="checkbox"
                      checked={removed}
                      onChange={(event) => {
                        onToggleRemove(event.target.checked);
                        if (event.target.checked) onChange("");
                      }}
                      className="h-3.5 w-3.5 accent-[var(--app-accent)]"
                    />
                    Quitar el valor guardado
                  </label>
                ) : null}
              </div>
            );
          case "multiline":
            return (
              <textarea
                {...common}
                value={text}
                placeholder={prop.placeholder}
                maxLength={prop.maxLength}
                onChange={(event) => onChange(event.target.value)}
                className={TEXTAREA}
              />
            );
          case "enum":
            return (
              <Select {...common} value={text} onChange={(event) => onChange(event.target.value)}>
                <option value="">{required ? "Elige una opción…" : "(sin valor)"}</option>
                {(prop.enum ?? []).map((option, index) => (
                  <option key={String(option)} value={String(index)}>
                    {prop.enumLabels?.[index] ?? String(option)}
                  </option>
                ))}
              </Select>
            );
          case "number":
          case "integer":
            return (
              <TextInput
                {...common}
                type="number"
                inputMode={kind === "integer" ? "numeric" : "decimal"}
                step={kind === "integer" ? 1 : "any"}
                min={prop.minimum}
                max={prop.maximum}
                value={text}
                placeholder={prop.placeholder}
                onChange={(event) => onChange(event.target.value)}
                className="w-40 tabular"
              />
            );
          default:
            return (
              <TextInput
                {...common}
                type={kind === "url" ? "url" : kind === "email" ? "email" : "text"}
                value={text}
                placeholder={prop.placeholder}
                maxLength={prop.maxLength}
                spellCheck={kind === "text"}
                onChange={(event) => onChange(event.target.value)}
              />
            );
        }
      }}
    </Field>
  );
}
