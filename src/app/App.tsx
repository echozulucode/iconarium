import { useEffect } from "react";
import { AssetContextMenu } from "../components/AssetContextMenu";
import { AssetGrid } from "../components/AssetGrid";
import { EmptyLibrary, NoLibrary, NoResults, Scanning } from "../components/EmptyState";
import { MultiSelectionToolbar } from "../components/MultiSelectionToolbar";
import { PropertiesDialog } from "../components/PropertiesDialog";
import { StatusBar } from "../components/StatusBar";
import { Toasts } from "../components/Toasts";
import { TopBar } from "../components/TopBar";
import { useLibrary } from "../stores/libraryStore";
import { useSearch } from "../stores/searchStore";
import { useSelection } from "../stores/selectionStore";
import { summaryCache } from "../stores/summaryCache";
import { useViewer } from "../stores/viewerStore";
import { SvgViewer } from "../viewer/SvgViewer";
import { backend } from "./backendRef";

export function App() {
  const ready = useLibrary((s) => s.ready);
  const libraryId = useLibrary((s) => s.library?.id ?? null);

  // Bootstrap + backend events.
  useEffect(() => {
    const b = backend();
    const unlisten: Promise<() => void>[] = [
      b.onScanProgress((s) => useLibrary.getState().setScan(s)),
      b.onCatalogChanged((c) => {
        useLibrary.getState().setTotal(c.total);
        if (c.reason === "load") {
          // A different library was loaded: start fresh.
          useSelection.getState().reset();
          useViewer.getState().close();
          useSearch.getState().reset();
          void useSearch.getState().runNow();
        } else {
          useSearch.getState().refresh();
        }
      }),
      b.onThumbReady((t) => summaryCache.thumbsReady(t.ids)),
    ];
    void useLibrary.getState().init();
    return () => {
      unlisten.forEach((p) => void p.then((u) => u()));
    };
  }, []);

  // Library switched (or first load): reset and run the current query.
  useEffect(() => {
    if (!ready) return;
    useSelection.getState().reset();
    useViewer.getState().close();
    useSearch.getState().reset();
    if (libraryId !== null) void useSearch.getState().runNow();
  }, [ready, libraryId]);

  // Ctrl+O: open folder.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && !e.shiftKey && e.key.toLowerCase() === "o") {
        e.preventDefault();
        void useLibrary.getState().pickLibrary();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className="flex h-full flex-col bg-bg text-fg">
      <TopBar />
      <main className="relative min-h-0 flex-1">{ready && <MainContent />}</main>
      <StatusBar />
      <AssetContextMenu />
      <PropertiesDialog />
      <SvgViewer />
      <Toasts />
    </div>
  );
}

function MainContent() {
  const library = useLibrary((s) => s.library);
  const scanPhase = useLibrary((s) => s.scan.phase);
  const total = useLibrary((s) => s.totalAssets);
  const loaded = useSearch((s) => s.loaded);
  const count = useSearch((s) => s.results.length);
  const hasQuery = useSearch((s) => s.resultsQuery.trim().length > 0);

  if (!library) return <NoLibrary />;
  if (count === 0 && loaded) {
    if (hasQuery) return <NoResults />;
    if (scanPhase === "discovering" || scanPhase === "loading" || total > 0) return <Scanning path={library.path} />;
    return <EmptyLibrary path={library.path} />;
  }
  if (!loaded) return <Scanning path={library.path} />;
  return (
    <>
      <AssetGrid />
      <MultiSelectionToolbar />
    </>
  );
}
