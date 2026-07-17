// A single app-wide right-click menu. Any element calls openContextMenu(event,
// items); the ContextMenu component (mounted once in the layout) renders it.
export type MenuItem = { label: string; action: () => void };

export const ctxMenu = $state<{ open: boolean; x: number; y: number; items: MenuItem[] }>({
  open: false,
  x: 0,
  y: 0,
  items: [],
});

export function openContextMenu(e: MouseEvent, items: MenuItem[]): void {
  if (!items.length) return;
  e.preventDefault();
  // Keep the menu on-screen (rough estimate; the component is small).
  const width = 224;
  const height = 8 + items.length * 36;
  ctxMenu.x = Math.min(e.clientX, window.innerWidth - width - 8);
  ctxMenu.y = Math.min(e.clientY, window.innerHeight - height - 8);
  ctxMenu.items = items;
  ctxMenu.open = true;
}

export function closeContextMenu(): void {
  ctxMenu.open = false;
  ctxMenu.items = [];
}
