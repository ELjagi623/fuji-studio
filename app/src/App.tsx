import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, AS_SHOT, CameraStatus, DEFAULT_SETTINGS, RafEntry, Recipe, RecipeEntry, Settings } from "./api";
import { useHistory } from "./history";
import { GridPrefs } from "./components/PhotoGrid";
import { FieldGroup, mergeRecipe } from "./recipeFields";
import { L, useLang } from "./i18n";
import { Sidebar } from "./components/Sidebar";
import { Toaster } from "./components/Toaster";
import { ModeGuide } from "./components/ModeGuide";
import { SelectPage } from "./pages/SelectPage";
import { ConfigurePage } from "./pages/ConfigurePage";
import { RenderPage, useRenderJob } from "./pages/RenderPage";

export type Page = "select" | "configure" | "render";

const PAGES: { id: Page; label: () => string; icon: string }[] = [
  { id: "select", label: () => L("选择", "Select"), icon: "▦" },
  { id: "configure", label: () => L("配置", "Configure"), icon: "◐" },
  { id: "render", label: () => L("渲染", "Render"), icon: "▶" },
];

function useCamera() {
  const [status, setStatus] = useState<CameraStatus | null>(null);
  const [probeError, setProbeError] = useState<string | null>(null);
  const probing = useRef(false);

  const refresh = useCallback(async () => {
    try {
      const s = await api.cameraStatus();
      setStatus(s);
      if (s.present && !s.info && !s.busy && !probing.current) {
        probing.current = true;
        try {
          await api.probeCamera();
          setProbeError(null);
          setStatus(await api.cameraStatus());
        } catch (e) {
          setProbeError(String(e));
        } finally {
          probing.current = false;
        }
      }
      if (!s.present) setProbeError(null);
    } catch (e) {
      setStatus(null);
    }
  }, []);

  useEffect(() => {
    refresh();
    const t = setInterval(refresh, 2000);
    return () => clearInterval(t);
  }, [refresh]);

  return { status, probeError, refresh };
}

export default function App() {
  useLang(); // re-render the whole tree when the language changes
  const [page, setPage] = useState<Page>("select");
  const camera = useCamera();

  // selection basket (ordered, across folders)
  const [selected, setSelected] = useState<Map<string, RafEntry>>(new Map());
  // recipe batch-applied to each photo (Configure page); only these photos get rendered
  const [assigned, setAssigned] = useState<Map<string, Recipe>>(new Map());

  // a photo taken out of the selection also loses its applied recipe
  useEffect(() => {
    setAssigned((prev) => {
      if ([...prev.keys()].every((p) => selected.has(p))) return prev;
      return new Map([...prev].filter(([p]) => selected.has(p)));
    });
  }, [selected]);

  // With exactly one photo selected, the recipe being edited IS that photo's recipe: editing
  // applies it automatically. Batch apply is for pushing one recipe onto many photos.
  const [recipe, setRecipe] = useState<Recipe>({ name: AS_SHOT });
  const lastRecipe = useRef(recipe);
  useEffect(() => {
    const changed = lastRecipe.current !== recipe;
    lastRecipe.current = recipe;
    if (selected.size !== 1) return;
    const path = [...selected.keys()][0];
    setAssigned((prev) => {
      if (!changed && prev.has(path)) return prev;
      return new Map(prev).set(path, recipe);
    });
  }, [recipe, selected]);

  // settings.json: last folder, view / sort of the photo grids
  const [settings, setSettings] = useState<Settings | null>(null);
  useEffect(() => {
    api.getSettings().then(
      (s) => setSettings({ ...DEFAULT_SETTINGS, ...s }),
      () => setSettings(DEFAULT_SETTINGS),
    );
  }, []);
  const updateSettings = useCallback((patch: Partial<Settings>) => {
    setSettings((s) => {
      const next = { ...(s ?? DEFAULT_SETTINGS), ...patch };
      api.setSettings(next).catch(() => {});
      return next;
    });
  }, []);
  const prefs: GridPrefs = useMemo(
    () => ({ view: settings?.view || "large", sortBy: settings?.sort_by || "name", desc: settings?.sort_desc ?? false }),
    [settings?.view, settings?.sort_by, settings?.sort_desc],
  );
  const onPrefs = useCallback(
    (p: Partial<GridPrefs>) =>
      updateSettings({
        ...(p.view !== undefined && { view: p.view }),
        ...(p.sortBy !== undefined && { sort_by: p.sortBy }),
        ...(p.desc !== undefined && { sort_desc: p.desc }),
      }),
    [updateSettings],
  );

  // recipes
  const [recipes, setRecipes] = useState<RecipeEntry[]>([]);
  const reloadRecipes = useCallback(async () => setRecipes(await api.listRecipes()), []);
  useEffect(() => {
    reloadRecipes();
  }, [reloadRecipes]);

  function applyBatch(paths: string[], src: Recipe, srcName: string, groups: FieldGroup[], all: boolean) {
    setAssigned((prev) => {
      const next = new Map(prev);
      for (const p of paths) next.set(p, mergeRecipe(prev.get(p), src, groups, srcName, all));
      return next;
    });
  }

  // ---- undo / redo (selection, the recipe being edited, applied recipes) ----
  const restore = useCallback((s: { selected: Map<string, RafEntry>; recipe: Recipe; assigned: Map<string, Recipe> }) => {
    setSelected(s.selected);
    setRecipe(s.recipe);
    setAssigned(s.assigned);
  }, []);
  const history = useHistory({ selected, recipe, assigned }, restore);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.ctrlKey && !e.metaKey) return;
      const t = e.target as HTMLElement | null;
      // inside a text field, let the field undo its own typing
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT" || t.isContentEditable)) return;
      const k = e.key.toLowerCase();
      if (k === "z" && !e.shiftKey) {
        e.preventDefault();
        history.undo();
      } else if (k === "y" || (k === "z" && e.shiftKey)) {
        e.preventDefault();
        history.redo();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [history.undo, history.redo]);

  const job = useRenderJob();
  const cameraModel = camera.status?.info?.model ?? null;
  // connected and identified, but not in USB RAW CONV mode
  const wrongMode = !!camera.status?.present && !!camera.status.info && !camera.status.info.raw_conv;

  return (
    <div className="app">
      <Sidebar
        camera={camera.status}
        probeError={camera.probeError}
        selectedCount={selected.size}
        appliedCount={assigned.size}
        recipe={recipe}
        job={job.state}
        onRetry={camera.refresh}
        canUndo={history.canUndo}
        canRedo={history.canRedo}
        onUndo={history.undo}
        onRedo={history.redo}
      />
      <main className="main">
        <div className="page" hidden={page !== "select"}>
          <SelectPage
            selected={selected}
            setSelected={setSelected}
            cameraModel={cameraModel}
            settings={settings}
            updateSettings={updateSettings}
            prefs={prefs}
            onPrefs={onPrefs}
          />
        </div>
        <div className="page" hidden={page !== "configure"}>
          <ConfigurePage
            active={page === "configure"}
            selected={selected}
            recipe={recipe}
            setRecipe={setRecipe}
            recipes={recipes}
            reloadRecipes={reloadRecipes}
            cameraReady={!!camera.status?.info?.raw_conv}
            cameraModel={cameraModel}
            wrongMode={wrongMode}
            onCameraError={camera.refresh}
            assigned={assigned}
            onApply={applyBatch}
            prefs={prefs}
            onPrefs={onPrefs}
          />
        </div>
        <div className="page" hidden={page !== "render"}>
          <RenderPage
            selected={selected}
            assigned={assigned}
            job={job}
            cameraReady={!!camera.status?.info?.raw_conv}
            cameraModel={cameraModel}
            wrongMode={wrongMode}
          />
        </div>
        <nav className="pagebar">
          {PAGES.map((p, i) => (
            <button key={p.id} className={page === p.id ? "active" : ""} onClick={() => setPage(p.id)}>
              <span className="pageicon">{p.icon}</span>
              <span>
                {i + 1}. {p.label()}
              </span>
            </button>
          ))}
        </nav>
      </main>
      <Toaster />
      <ModeGuide camera={camera.status} onRecheck={camera.refresh} />
    </div>
  );
}
