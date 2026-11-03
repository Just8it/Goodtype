// Offline parser checks against Goodtype's already installed PDF.js. No viewer claims.
import assert from 'node:assert/strict';
import { readFile, writeFile, access } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const [pdfjsRoot, artifactRoot] = process.argv.slice(2);
assert(pdfjsRoot && artifactRoot, 'Usage: node pdf_probe.mjs <pdfjs-dist directory> <layout artifact directory>');
const { getDocument, version } = await import(pathToFileURL(resolve(pdfjsRoot, 'legacy/build/pdf.mjs')).href);
const names = ['short', 'thirty-cells', 'long-stream', 'table', 'a4-split',
  'height-5000', 'height-14400', 'height-20000', 'height-50000'];
try { await access(resolve(artifactRoot, 'plot.pdf')); names.push('plot'); } catch {}
const report = [];
for (const name of names) {
  const bytes = await readFile(resolve(artifactRoot, `${name}.pdf`));
  const task = getDocument({ data: new Uint8Array(bytes), useSystemFonts: false,
    isEvalSupported: false, disableFontFace: true });
  const document = await task.promise;
  const pages = [];
  let allText = '';
  try {
    for (let index = 1; index <= document.numPages; index++) {
      const page = await document.getPage(index);
      const text = await page.getTextContent();
      const pageText = text.items.map(item => item.str ?? '').join(' ');
      allText += pageText + ' ';
      if (name === 'a4-split') {
        const cells = (pageText.match(/Cell\s+\d+/g) ?? []).length;
        assert.equal((pageText.match(/value_0/g) ?? []).length, cells, 'Code cell split from its label');
        assert.equal((pageText.match(/value_3/g) ?? []).length, cells, 'Code cell split across sheets');
      }
      pages.push({ number: index, box: page.view, userUnit: page.userUnit,
        textItems: text.items.length });
      assert(Math.abs(page.view[2] - 595.2756) < 0.01, `${name}: wrong A4 width`);
      page.cleanup();
    }
    if (name === 'short') {
      assert.equal(document.numPages, 1);
      assert(pages[0].box[3] < 200);
      assert.match(allText, /value_0/);
    }
    if (name === 'thirty-cells' || name === 'a4-split') {
      for (let cell = 1; cell <= 30; cell++) {
        assert(new RegExp(`Cell\\s+${cell}(?!\\d)`).test(allText), `Missing cell ${cell} in ${name}`);
      }
      assert.equal(document.numPages, name === 'thirty-cells' ? 1 : 3);
    }
    if (name === 'long-stream') assert(/line\s+1999:\s+saved\s+output/.test(allText), 'Final saved output line is missing');
    report.push({ name, pages, characters: allText.length,
      hasExplicitUserUnit: /\/UserUnit\b/.test(bytes.toString('latin1')) });
  } finally {
    await document.destroy();
  }
}
const destination = resolve(artifactRoot, 'pdfjs-evidence.json');
await writeFile(destination, JSON.stringify({ pdfjsVersion: version, parserOnly: true, cases: report }, null, 2), { flag: 'wx' });
console.log(`PASS: ${names.length} PDF.js parse/text cases. Evidence: ${destination}`);
