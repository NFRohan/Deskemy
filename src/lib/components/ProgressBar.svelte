<script lang="ts">
  let {
    value = 0,
    height = "h-1",
    complete = false,
  }: { value?: number; height?: string; complete?: boolean } = $props();

  const clamped = $derived(Math.max(0, Math.min(100, value)));
</script>

<div class="{height} w-full bg-border rounded-full overflow-hidden">
  <div
    class="h-full rounded-full {complete ? 'bg-secondary-container' : 'bg-accent-blue'}"
    style="width: {clamped}%"
  ></div>
</div>
