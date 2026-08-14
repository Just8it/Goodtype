import { describe, expect, it } from "vitest";
import type { TypstCompileResult } from "./typst";
import { clearTypstCache, getCachedTypst, setCachedTypst } from "./typstCache";

function result(svg: string | null, generation = 1): TypstCompileResult {
  return { generation, svg, widthPt: 120, heightPt: 48, padPt: 16, diagnostics: [] };
}

describe("typstCache", () => {
  it("returns a stored compile for the same source and width", () => {
    const source = `= Cached ${Math.random()}`;
    expect(getCachedTypst(source, 200)).toBeUndefined();

    setCachedTypst(source, 200, result("<svg>ok</svg>"));
    const hit = getCachedTypst(source, 200);
    expect(hit?.svg).toBe("<svg>ok</svg>");
    // The cached artifact carries no generation; callers stamp their own.
    expect(hit).not.toHaveProperty("generation");
  });

  it("misses when the source or width changes", () => {
    const source = `= Vary ${Math.random()}`;
    setCachedTypst(source, 200, result("<svg>a</svg>"));

    expect(getCachedTypst(`${source} edited`, 200)).toBeUndefined();
    expect(getCachedTypst(source, 240)).toBeUndefined();
  });

  it("never caches a failed compile so the last valid preview survives", () => {
    const source = `= Failing ${Math.random()}`;
    setCachedTypst(source, 200, result(null));
    expect(getCachedTypst(source, 200)).toBeUndefined();
  });

  it("invalidates inherited preset previews without touching page overrides", () => {
    const inherited = '#import "/styles/default.typ": preset';
    const override = '#import "/styles/custom.typ": preset';
    setCachedTypst(inherited, 200, result("<svg>default</svg>"));
    setCachedTypst(override, 200, result("<svg>custom</svg>"));
    clearTypstCache("/styles/default.typ");
    expect(getCachedTypst(inherited, 200)).toBeUndefined();
    expect(getCachedTypst(override, 200)?.svg).toBe("<svg>custom</svg>");
  });
});
