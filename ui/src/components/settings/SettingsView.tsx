import { cn } from "cn";
import { FolderPlus, Trash2 } from "lucide-react";
import { useEffect, useId, useRef, useState, type JSX, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { ShortcutList } from "@/components/controls/ShortcutList";
import { Button } from "@/components/ui/button";
import { i18n, LANGUAGES } from "@/i18n";
import { backendLabel } from "@/lib/format";
import { api } from "@/lib/ipc";
import { notifyError } from "@/lib/notify";
import type { ModelStatus, StemsBackendPreference } from "@/lib/types";
import { useDevices } from "@/stores/devices";
import { useSettings } from "@/stores/settings";

const SECTIONS = [
  { id: "general", labelKey: "settings.sections.general" },
  { id: "audio", labelKey: "settings.sections.audio" },
  { id: "library", labelKey: "settings.sections.library" },
  { id: "processing", labelKey: "settings.sections.processing" },
  { id: "models", labelKey: "settings.sections.models" },
  { id: "controller", labelKey: "settings.sections.controller" },
  { id: "keyboard", labelKey: "settings.sections.keyboard" },
] as const;

type SectionId = (typeof SECTIONS)[number]["id"];

/** Buffer sizes offered; smaller is lower latency, larger is safer on slow machines. */
const BUFFER_SIZES = [128, 256, 512, 1024] as const;

/** Saves the buffer size; the store shows the error and reverts on failure. */
function pickBuffer(frames: number): void {
  void useSettings.getState().set("bufferSize", frames);
}

/** The model list, or an empty list (with an error toast) when Rust cannot answer. */
async function readModels(): Promise<ModelStatus[]> {
  try {
    return await api().modelsStatus();
  } catch (error) {
    notifyError(error, i18n.t("errors.settings.readModels"));
    return [];
  }
}

/** Callback ref: focuses the element when it mounts (React calls it once for a stable function). */
function focusOnMount(el: HTMLButtonElement | null): void {
  el?.focus();
}

/** Callback ref for a model row button: takes focus when its list asks for that model. */
function focusIfPending(el: HTMLButtonElement | null): void {
  if (!el) return;
  const wanted = el.closest<HTMLElement>("[data-focus-model]")?.dataset.focusModel;
  if (wanted !== undefined && wanted === el.dataset.model) el.focus();
}

function megabytes(bytes: number): string {
  return i18n.t("settings.models.size", { value: Math.round(bytes / 1_000_000) });
}

/**
 * Settings (SPEC, Module 7): one section at a time, picked from the list on
 * the left. Every change goes to Rust through the settings store (optimistic,
 * reverted on error); Rust broadcasts it to the other window.
 */
export function SettingsView(): JSX.Element {
  const { t } = useTranslation();
  const [section, setSection] = useState<SectionId>("general");
  const settings = useSettings((s) => s.settings);

  return (
    <div className="grid h-full min-h-0 grid-cols-[180px_1fr]">
      <nav
        aria-label={t("settings.sectionsLabel")}
        className="border-r border-border bg-sidebar p-2"
      >
        <ul className="flex flex-col gap-0.5">
          {SECTIONS.map((s) => (
            <li key={s.id}>
              <button
                type="button"
                aria-current={section === s.id ? "page" : undefined}
                onClick={() => setSection(s.id)}
                className={cn(
                  "w-full rounded-md px-2.5 py-1.5 text-left text-muted-foreground hover:bg-card hover:text-foreground",
                  section === s.id && "bg-card text-foreground",
                )}
              >
                {t(s.labelKey)}
              </button>
            </li>
          ))}
        </ul>
      </nav>
      <div className="min-h-0 overflow-y-auto p-5">
        {!settings ? (
          <p className="text-muted-foreground">{t("settings.loading")}</p>
        ) : section === "general" ? (
          <GeneralSection />
        ) : section === "audio" ? (
          <AudioSection />
        ) : section === "library" ? (
          <LibrarySection />
        ) : section === "processing" ? (
          <ProcessingSection />
        ) : section === "models" ? (
          <ModelsSection />
        ) : section === "controller" ? (
          <ControllerSection />
        ) : (
          <Section
            title={t("settings.sections.keyboard")}
            description={t("settings.keyboard.description")}
          >
            <ShortcutList bare />
          </Section>
        )}
      </div>
    </div>
  );
}

function Section({
  title,
  description,
  children,
}: {
  title: string;
  description?: string;
  children: ReactNode;
}): JSX.Element {
  return (
    <section className="flex max-w-xl flex-col gap-4">
      <header>
        <h2 className="text-base font-semibold">{title}</h2>
        {description && <p className="mt-0.5 text-muted-foreground">{description}</p>}
      </header>
      {children}
    </section>
  );
}

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: (id: string) => ReactNode;
}): JSX.Element {
  const id = useId();
  return (
    <div className="grid grid-cols-[160px_1fr] items-start gap-3">
      <label htmlFor={id} className="pt-1.5">
        {label}
      </label>
      <div className="flex flex-col gap-1">
        {children(id)}
        {hint && <p className="text-[11px] text-muted-foreground">{hint}</p>}
      </div>
    </div>
  );
}

const SELECT =
  "h-7 rounded-md border border-border bg-secondary px-2 text-[13px] text-foreground";

function GeneralSection(): JSX.Element {
  const { t } = useTranslation();
  const language = useSettings((s) => s.settings?.language ?? "en");
  const set = useSettings((s) => s.set);

  const pickLanguage = (code: string): void => {
    // Narrow the select's string to a language we offer.
    const picked = LANGUAGES.find((l) => l.code === code);
    if (picked) void set("language", picked.code);
  };

  return (
    <Section
      title={t("settings.sections.general")}
      description={t("settings.general.description")}
    >
      <Field label={t("settings.general.language")}>
        {(id) => (
          <select
            id={id}
            className={SELECT}
            value={language}
            onChange={(e) => pickLanguage(e.target.value)}
          >
            {LANGUAGES.map((l) => (
              <option key={l.code} value={l.code} lang={l.code}>
                {l.name}
              </option>
            ))}
          </select>
        )}
      </Field>
    </Section>
  );
}

function AudioSection(): JSX.Element {
  const { t } = useTranslation();
  const devices = useDevices((s) => s.audio);
  const loadDevices = useDevices((s) => s.load);
  const deviceId = useSettings((s) => s.settings?.audioDeviceId ?? null);
  const bufferSize = useSettings((s) => s.settings?.bufferSize ?? 512);
  const systemDefault = devices.find((d) => d.isDefault);
  // Empty: follow the system default (the setting is null).
  const selected =
    deviceId !== null && devices.some((d) => d.id === deviceId) ? deviceId : "";
  // The device in use: the chosen one when it is there, else the default.
  const current = devices.find((d) => d.id === selected) ?? systemDefault;

  const pickDevice = (id: string): void => {
    if (id === "") {
      void useSettings.getState().set("audioDeviceId", null).then(loadDevices);
      return;
    }
    api()
      .audioSetDevice(id)
      .then(loadDevices)
      .catch((error: unknown) => notifyError(error, t("errors.settings.audioOutput")));
  };

  return (
    <Section
      title={t("settings.sections.audio")}
      description={t("settings.audio.description")}
    >
      <Field label={t("settings.audio.device")}>
        {(id) => (
          <select
            id={id}
            className={SELECT}
            value={selected}
            onChange={(e) => pickDevice(e.target.value)}
          >
            {devices.length === 0 ? (
              <option value="">{t("settings.audio.noDevices")}</option>
            ) : (
              <option value="">
                {t("settings.audio.systemDefault", { name: systemDefault?.name ?? "" })}
              </option>
            )}
            {devices.map((d) => (
              <option key={d.id} value={d.id}>
                {d.name}
              </option>
            ))}
          </select>
        )}
      </Field>
      <Field label={t("settings.audio.bufferSize")} hint={t("settings.audio.bufferHint")}>
        {(id) => (
          <select
            id={id}
            className={SELECT}
            value={bufferSize}
            onChange={(e) => pickBuffer(Number(e.target.value))}
          >
            {BUFFER_SIZES.map((frames) => (
              <option key={frames} value={frames}>
                {t("settings.audio.samples", { frames })}
              </option>
            ))}
          </select>
        )}
      </Field>
      <Field label={t("settings.audio.sampleRate")}>
        {(id) => (
          <output id={id} className="pt-1.5 font-mono tabular-nums">
            {current ? `${current.sampleRate / 1000} kHz` : "--"}
          </output>
        )}
      </Field>
    </Section>
  );
}

function LibrarySection(): JSX.Element {
  const { t } = useTranslation();
  const folders = useSettings((s) => s.settings?.watchedFolders ?? []);
  const watch = useSettings((s) => s.settings?.watchFolders ?? false);
  const output = useSettings((s) => s.settings?.outputFolder ?? "");
  const set = useSettings((s) => s.set);

  const addFolder = async (): Promise<void> => {
    try {
      const folder = await api().dialogOpenFolder();
      if (folder && !folders.includes(folder))
        await set("watchedFolders", [...folders, folder]);
    } catch (error) {
      notifyError(error, t("errors.settings.folderPicker"));
    }
  };
  const chooseOutput = async (): Promise<void> => {
    try {
      const folder = await api().dialogOpenFolder();
      if (folder) await set("outputFolder", folder);
    } catch (error) {
      notifyError(error, t("errors.settings.folderPicker"));
    }
  };

  return (
    <Section
      title={t("settings.sections.library")}
      description={t("settings.library.description")}
    >
      <div className="flex flex-col gap-2">
        <h3 className="font-medium">{t("settings.library.watched")}</h3>
        {folders.length === 0 ? (
          <p className="text-muted-foreground">{t("settings.library.empty")}</p>
        ) : (
          <ul className="flex flex-col gap-1">
            {folders.map((folder) => (
              <li
                key={folder}
                className="flex items-center gap-2 rounded-md border border-border px-2 py-1"
              >
                <span
                  className="min-w-0 flex-1 truncate font-mono text-[12px]"
                  title={folder}
                >
                  {folder}
                </span>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label={t("settings.library.stopWatching", { folder })}
                  onClick={() =>
                    void set(
                      "watchedFolders",
                      folders.filter((f) => f !== folder),
                    )
                  }
                >
                  <Trash2 aria-hidden="true" />
                </Button>
              </li>
            ))}
          </ul>
        )}
        <div className="flex items-center gap-3">
          <Button variant="outline" size="sm" onClick={() => void addFolder()}>
            <FolderPlus aria-hidden="true" data-icon="inline-start" />
            {t("settings.library.addFolder")}
          </Button>
          <label className="flex items-center gap-1.5">
            <input
              type="checkbox"
              checked={watch}
              onChange={(e) => void set("watchFolders", e.target.checked)}
              className="accent-primary"
            />
            {t("settings.library.autoAdd")}
          </label>
        </div>
      </div>
      <Field label={t("settings.library.output")} hint={t("settings.library.outputHint")}>
        {(id) => (
          <div className="flex items-center gap-2">
            <output
              id={id}
              className="min-w-0 flex-1 truncate font-mono text-[12px]"
              title={output}
            >
              {output || t("settings.library.notSet")}
            </output>
            <Button variant="outline" size="sm" onClick={() => void chooseOutput()}>
              {t("settings.library.choose")}
            </Button>
          </div>
        )}
      </Field>
    </Section>
  );
}

function ProcessingSection(): JSX.Element {
  const { t } = useTranslation();
  const preference = useSettings((s) => s.settings?.stemsBackend ?? "auto");
  const set = useSettings((s) => s.set);
  const backend = useDevices((s) => s.backend);
  const options: ReadonlyArray<{
    value: StemsBackendPreference;
    label: string;
    hint: string;
  }> = [
    {
      value: "auto",
      label: t("settings.processing.auto"),
      hint: t("settings.processing.autoHint"),
    },
    {
      value: "cpu",
      label: t("settings.processing.cpu"),
      hint: t("settings.processing.cpuHint"),
    },
  ];

  return (
    <Section
      title={t("settings.sections.processing")}
      description={t("settings.processing.description")}
    >
      <fieldset className="flex flex-col gap-2">
        <legend className="mb-1 font-medium">{t("settings.processing.backend")}</legend>
        {options.map((o) => (
          <label key={o.value} className="flex items-start gap-2">
            <input
              type="radio"
              name="stems-backend"
              value={o.value}
              checked={preference === o.value}
              onChange={() => void set("stemsBackend", o.value)}
              className="mt-1 accent-primary"
            />
            <span>
              {o.label}
              <span className="block text-[11px] text-muted-foreground">{o.hint}</span>
            </span>
          </label>
        ))}
      </fieldset>
      <p className="text-muted-foreground">
        {!backend
          ? t("settings.processing.checking")
          : backend.reason
            ? t("settings.processing.inUseReason", {
                backend: backendLabel(backend.active),
                reason: t(`settings.processing.cpuReasons.${backend.reason}`),
              })
            : t("settings.processing.inUse", { backend: backendLabel(backend.active) })}
      </p>
    </Section>
  );
}

function ModelsSection(): JSX.Element {
  const { t } = useTranslation();
  const [models, setModels] = useState<ModelStatus[] | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);
  /** Running downloads: model name to percent (this window may not share the main window's jobs). */
  const [progress, setProgress] = useState<Record<string, number>>({});
  const jobToModel = useRef(new Map<string, string>());
  /** The model whose row button should take focus once it renders (after Keep or Delete). */
  const [focusRow, setFocusRow] = useState<string | null>(null);
  const refresh = async (): Promise<void> => setModels(await readModels());

  useEffect(() => {
    let current = true;
    const load = async (): Promise<void> => {
      const list = await readModels();
      if (current) setModels(list);
    };
    void load();
    const subs = [
      api().onJobProgress((p) => {
        const name = jobToModel.current.get(p.jobId);
        if (name === undefined) return;
        const pct = p.total > 0 ? Math.round((p.done / p.total) * 100) : 0;
        setProgress((prev) => ({ ...prev, [name]: pct }));
      }),
      api().onJobDone((done) => {
        const name = jobToModel.current.get(done.jobId);
        if (name === undefined) return;
        jobToModel.current.delete(done.jobId);
        setProgress(({ [name]: _finished, ...rest }) => rest);
        void load();
      }),
    ];
    return () => {
      current = false;
      for (const sub of subs) void sub.then((stop) => stop());
    };
  }, []);

  const download = async (name: string): Promise<void> => {
    if (name in progress) return;
    setProgress((prev) => ({ ...prev, [name]: 0 }));
    try {
      jobToModel.current.set(await api().modelsDownload(name), name);
    } catch (error) {
      setProgress(({ [name]: _failed, ...rest }) => rest);
      notifyError(error, t("errors.settings.download"));
    }
  };

  const remove = (name: string): void => {
    setFocusRow(name);
    setConfirming(null);
    api()
      .modelsDelete(name)
      .then(refresh)
      .catch((error: unknown) => notifyError(error, t("errors.settings.deleteModel")));
  };

  return (
    <Section
      title={t("settings.sections.models")}
      description={t("settings.models.description")}
    >
      {models === null ? (
        <p className="text-muted-foreground">{t("settings.models.checking")}</p>
      ) : models.length === 0 ? (
        <p className="flex items-center gap-2 text-muted-foreground">
          {t("settings.models.empty")}
          <Button variant="outline" size="sm" onClick={() => void refresh()}>
            {t("settings.models.retry")}
          </Button>
        </p>
      ) : (
        <ul data-focus-model={focusRow ?? undefined} className="flex flex-col gap-2">
          {models.map((m) => {
            const pct = progress[m.name];
            return (
              <li
                key={m.name}
                className="flex items-center gap-3 rounded-md border border-border px-3 py-2"
              >
                <span className="flex-1">
                  <span className="font-mono">{m.name}</span>
                  <span className="block text-[11px] text-muted-foreground">
                    {t("settings.models.status", {
                      size: megabytes(m.sizeBytes),
                      state:
                        pct !== undefined
                          ? t("settings.models.stateDownloading", { pct })
                          : m.downloaded
                            ? t("settings.models.stateDownloaded")
                            : t("settings.models.stateNotDownloaded"),
                    })}
                  </span>
                </span>
                {!m.downloaded ? (
                  <Button
                    ref={focusIfPending}
                    data-model={m.name}
                    size="sm"
                    disabled={pct !== undefined}
                    onClick={() => void download(m.name)}
                  >
                    {pct !== undefined
                      ? t("settings.models.downloading", { pct })
                      : t("settings.models.download")}
                  </Button>
                ) : confirming === m.name ? (
                  <>
                    <Button
                      variant="destructive"
                      size="sm"
                      onClick={() => remove(m.name)}
                    >
                      {t("settings.models.deleteForGood")}
                    </Button>
                    <Button
                      ref={focusOnMount}
                      variant="ghost"
                      size="sm"
                      onClick={() => {
                        setFocusRow(m.name);
                        setConfirming(null);
                      }}
                    >
                      {t("settings.models.keep")}
                    </Button>
                  </>
                ) : (
                  <Button
                    ref={focusIfPending}
                    data-model={m.name}
                    variant="outline"
                    size="sm"
                    onClick={() => {
                      setFocusRow(null);
                      setConfirming(m.name);
                    }}
                  >
                    {t("settings.models.delete")}
                  </Button>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </Section>
  );
}

function ControllerSection(): JSX.Element {
  const { t } = useTranslation();
  const devices = useDevices((s) => s.midi);
  return (
    <Section
      title={t("settings.sections.controller")}
      description={t("settings.controller.description")}
    >
      {devices.length === 0 ? (
        <p className="text-muted-foreground">{t("settings.controller.empty")}</p>
      ) : (
        <ul className="flex flex-col gap-1">
          {devices.map((d) => (
            <li key={d.id} className="flex items-center gap-2">
              <span
                aria-hidden="true"
                className={cn(
                  "size-1.5 rounded-full",
                  d.connected ? "bg-primary" : "bg-secondary",
                )}
              />
              {d.name}
              <span className="text-[11px] text-muted-foreground">
                {d.connected
                  ? t("settings.controller.connected")
                  : t("settings.controller.notConnected")}
              </span>
            </li>
          ))}
        </ul>
      )}
    </Section>
  );
}
