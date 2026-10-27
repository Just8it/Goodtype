/** Touch menus belong to our one-finger hold, not the browser's multi-touch/right-tap gesture. */
export function nativeContextMenuAllowed(event: MouseEvent & {
  pointerType?: string;
  sourceCapabilities?: { firesTouchEvents: boolean } | null;
}): boolean {
  return !event.sourceCapabilities?.firesTouchEvents &&
    (!event.pointerType || event.pointerType === "mouse");
}
