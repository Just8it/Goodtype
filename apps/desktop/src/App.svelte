<script lang="ts">
  import PagePrototype from "./lib/prototype/PagePrototype.svelte";
</script>

<PagePrototype />

<svelte:head>
  <title>Goodtype</title>
</svelte:head>

<style>
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
