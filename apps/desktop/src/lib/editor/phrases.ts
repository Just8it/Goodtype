import type { Translator } from "../i18n";

/** CodeMirror owns these exact phrase identifiers. It reads this facet without rebuilding its document. */
export function editorPhrases(t: Translator): Record<string, string> {
  return {
    "Find": t("codeMirror.find"), "Replace": t("codeMirror.replace"),
    "next": t("codeMirror.next"), "previous": t("codeMirror.previous"),
    "all": t("codeMirror.all"), "match case": t("codeMirror.matchCase"),
    "regexp": t("codeMirror.regexp"), "by word": t("codeMirror.byWord"),
    "replace all": t("codeMirror.replaceAll"), "close": t("codeMirror.close"),
    "Go to line": t("codeMirror.goToLine"), "go": t("codeMirror.go"),
    "Fold line": t("codeMirror.foldLine"), "Unfold line": t("codeMirror.unfoldLine"),
    "folded code": t("codeMirror.foldedCode"), "unfold": t("codeMirror.unfold"),
    "Control character": t("codeMirror.controlCharacter"),
    "No diagnostics": t("codeMirror.noDiagnostics"), "Completions": t("codeMirror.completions"),
  };
}
