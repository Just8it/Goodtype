import { describe, expect, it } from "vitest";
import ts from "typescript";
import component from "./PagePrototype.svelte?raw";

type Action = { kind: "structure" } | { kind: "page"; pageId: string };

// Run the real handlers with controlled IPC/reopen boundaries, without mounting the canvas.
function movedNotebook(undo: Action[], redo: Action[] = [], reopen = true) {
  const script = component.split('<script lang="ts">')[1].split("</script>")[0];
  const ast = ts.createSourceFile("PagePrototype.ts", script, ts.ScriptTarget.Latest, true);
  const handlers = ["moveLibrarySelection", "routeHistory"].map(name => {
    const node = ast.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === name);
    if (!node) throw new Error(`Missing handler: ${name}`);
    return node.getText(ast);
  }).join("\n");
  const javascript = ts.transpileModule(handlers, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
  return new Function("undo", "redo", "reopen", `
    let root = "old", activePageId = "page-1", activeSnapshot = {};
    let notebookUndoOrder = undo, notebookRedoOrder = redo;
    let canUndo = undo.length > 0, canRedo = redo.length > 0;
    let openTabs = [{ root, title: "Notes" }], notebookChosen = false;
    const settings = { undoScope: "notebook" }, calls = [];
    const $t = key => key;
    const persist = async () => true;
    const recordNotebookPage = async () => {};
    const rememberNotebookSession = async () => true;
    const moveLibraryEntries = async () => ({ rootMoves: [{ from: "old", to: "new" }], warning: null });
    const openNotebookAt = async next => {
      root = next;
      notebookUndoOrder = []; notebookRedoOrder = [];
      canUndo = false; canRedo = false;
      return reopen;
    };
    const queueStructureHistory = command => calls.push({ command });
    const queueHistory = command => calls.push({ command, root, page: activePageId });
    const activatePage = async page => { activePageId = page; };
    const scrollToPage = () => {};
    ${javascript}
    return { moveLibrarySelection, routeHistory, calls,
      state: () => ({ root, notebookUndoOrder, notebookRedoOrder, canUndo, canRedo }) };
  `)(undo, redo, reopen) as {
    moveLibrarySelection(paths: string[], destination: string): Promise<string | null>;
    routeHistory(command: "undo_notebook" | "redo_notebook", label: string): Promise<void>;
    calls: { command: string; root?: string; page?: string }[];
    state(): { root: string; notebookUndoOrder: Action[]; notebookRedoOrder: Action[]; canUndo: boolean; canRedo: boolean };
  };
}

describe("history after notebook relocation", () => {
  it.each([true, false])("retains structural Undo even when reopening succeeds=%s", async reopen => {
    const notebook = movedNotebook([{ kind: "structure" }], [], reopen);
    await notebook.moveLibrarySelection(["Notes"], "Archive");
    await notebook.routeHistory("undo_notebook", "Undo");
    expect(notebook.calls).toEqual([{ command: "undo_page_structure" }]);
    expect(notebook.state().canUndo).toBe(true);
    expect(notebook.state().root).toBe("new");
  });

  it("keeps notebook-wide Undo and Redo routed to their original pages", async () => {
    const notebook = movedNotebook([{ kind: "page", pageId: "page-2" }], [{ kind: "page", pageId: "page-3" }]);
    await notebook.moveLibrarySelection(["Notes"], "Archive");
    await notebook.routeHistory("undo_notebook", "Undo");
    await notebook.routeHistory("redo_notebook", "Redo");
    expect(notebook.calls).toEqual([
      { command: "undo_notebook", root: "new", page: "page-2" },
      { command: "redo_notebook", root: "new", page: "page-3" },
    ]);
    expect(notebook.state().canRedo).toBe(true);
  });
});
