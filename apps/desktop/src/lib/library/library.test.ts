import { describe, expect, it } from "vitest";
import {
  bands,
  breadcrumb,
  childPath,
  nameProblem,
  parentPath,
  type LibraryEntry,
} from "./library";

const folder = (name: string, modifiedMs: number | null = 0): LibraryEntry => ({
  kind: "folder",
  name,
  path: name,
  modifiedMs,
  childCount: 0,
});

const notebook = (name: string, modifiedMs: number | null = 0): LibraryEntry => ({
  kind: "notebook",
  name,
  path: name,
  modifiedMs,
  pageCount: 1,
  paper: null,
});

describe("library paths", () => {
  it("walks the trail from the root down to the folder in view", () => {
    expect(breadcrumb("Semester 3/Thermodynamik")).toEqual([
      { name: "Bibliothek", path: "" },
      { name: "Semester 3", path: "Semester 3" },
      { name: "Thermodynamik", path: "Semester 3/Thermodynamik" },
    ]);
    // The root is a crumb of its own, so there is always something to navigate back to.
    expect(breadcrumb("")).toEqual([{ name: "Bibliothek", path: "" }]);
  });

  it("climbs one level at a time and stops at the root", () => {
    expect(parentPath("Semester 3/Thermodynamik/Serie 07")).toBe("Semester 3/Thermodynamik");
    expect(parentPath("Semester 3")).toBe("");
    // Null rather than "" — the root has no parent, and a caller must be able to tell the
    // difference between "go to the root" and "there is nowhere to go".
    expect(parentPath("")).toBeNull();
    expect(childPath("", "Semester 1")).toBe("Semester 1");
    expect(childPath("Semester 1", "Mathe 3")).toBe("Semester 1/Mathe 3");
  });
});

describe("names Goodtype will create", () => {
  it("accepts what coursework is actually called", () => {
    for (const name of ["Semester 3", "Thermodynamik", "Serie_07", "Übung 2 – Kinematik"]) {
      expect(nameProblem(name)).toBeNull();
    }
  });

  // These become directory names on someone else's machine the moment a library is synced, so
  // the rule is the intersection of the three platforms, not whatever this one tolerates.
  it("refuses what another platform would mangle or reject", () => {
    expect(nameProblem("")).not.toBeNull();
    expect(nameProblem("Serie 7/8")).not.toBeNull();
    expect(nameProblem("Mathe: Teil 2")).not.toBeNull();
    expect(nameProblem(".versteckt")).not.toBeNull();
    // Windows strips a trailing dot, which would silently merge two distinct names.
    expect(nameProblem("Serie 7.")).not.toBeNull();
    expect(nameProblem("con")).not.toBeNull();
    expect(nameProblem(" Semester 1")).not.toBeNull();
  });

  // Both of these used to pass here and then be refused by Rust, which is the worst possible
  // split: the writer gets a green field and an error from the backend.
  it("agrees with the Rust rule on the cases they used to disagree about", () => {
    for (const reserved of ["com", "lpt", "com0", "LPT0"]) {
      expect(nameProblem(reserved)).not.toBeNull();
    }
    // 80 characters, not 80 UTF-8 bytes: Rust counts characters, so this must be accepted.
    expect(nameProblem("Ü".repeat(80))).toBeNull();
    expect(nameProblem("Ü".repeat(81))).not.toBeNull();
  });
});

describe("shelf order", () => {
  it("puts Serie 2 before Serie 10", () => {
    const { notebooks } = bands(
      [notebook("Serie 10"), notebook("Serie 2"), notebook("Serie 1")],
      "name",
    );
    expect(notebooks.map((entry) => entry.name)).toEqual(["Serie 1", "Serie 2", "Serie 10"]);
  });

  it("keeps folders and notebooks in separate bands", () => {
    const { folders, notebooks } = bands(
      [notebook("Alt"), folder("Zettel"), notebook("Beta"), folder("Aufgaben")],
      "name",
    );
    expect(folders.map((entry) => entry.name)).toEqual(["Aufgaben", "Zettel"]);
    expect(notebooks.map((entry) => entry.name)).toEqual(["Alt", "Beta"]);
  });

  it("sorts by date newest first, since the point is finding what you just had open", () => {
    const { notebooks } = bands(
      [notebook("alt", 100), notebook("neu", 900), notebook("mittel", 500)],
      "modified",
    );
    expect(notebooks.map((entry) => entry.name)).toEqual(["neu", "mittel", "alt"]);
  });

  it("falls back to the name when a date is missing rather than dropping the entry", () => {
    const { notebooks } = bands([notebook("b", null), notebook("a", null)], "modified");
    expect(notebooks.map((entry) => entry.name)).toEqual(["a", "b"]);
  });
});
