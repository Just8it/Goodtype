<script lang="ts">
  import PagePrototype from "./lib/prototype/PagePrototype.svelte";
</script>

<PagePrototype />

<svelte:head>
  <title>Goodtype</title>
</svelte:head>

<style>
  /* One scrollbar for the whole app. The platform's own is a 17px tray with stepper arrows at
     both ends, which is wider than some of the controls it sits beside and belongs to a different
     decade of Windows than the rest of this chrome. Overlay-thin, and it fades into the panel it
     scrolls rather than framing it. */
  :global(*) {
    scrollbar-width: thin;
    scrollbar-color: rgb(255 255 255 / 16%) transparent;
  }

  :global(::-webkit-scrollbar) {
    width: 8px;
    height: 8px;
  }

  :global(::-webkit-scrollbar-track),
  :global(::-webkit-scrollbar-corner) {
    background: transparent;
  }

  :global(::-webkit-scrollbar-thumb) {
    border: 2px solid transparent;
    border-radius: 999px;
    background: rgb(255 255 255 / 16%);
    background-clip: padding-box;
  }

  :global(::-webkit-scrollbar-thumb:hover) {
    background: rgb(255 255 255 / 30%);
    background-clip: padding-box;
  }

  :global(html),
  :global(body),
  :global(#app) {
    width: 100%;
    height: 100%;
    min-height: 100%;
    margin: 0;
    overflow: hidden;
  }
</style>
