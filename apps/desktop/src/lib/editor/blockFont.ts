const header = /^\/\/ Goodtype block font\r?\n#set text\(font: ("(?:[^"\\\r\n]|\\.)*")\)\r?\n\r?\n/;

export function blockFont(source: string): string {
  const name = header.exec(source)?.[1];
  try { return name ? JSON.parse(name) : ""; } catch { return ""; }
}

/** Replace only our leading setting; literal examples and the user's source stay intact. */
export function withBlockFont(source: string, font: string): string {
  const body = source.replace(header, "");
  return font ? `// Goodtype block font\n#set text(font: ${JSON.stringify(font)})\n\n${body}` : body;
}
