import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  build: {
    rolldownOptions: {
      output: {
        assetFileNames: (asset) =>
          asset.names.some((name) => /^(jbig2|openjpeg|qcms_bg|LICENSE_)/.test(name))
            ? "assets/pdfjs/[name][extname]"
            : "assets/[name]-[hash][extname]",
      },
    },
  },
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
});

