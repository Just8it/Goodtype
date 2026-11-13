import { afterEach, expect, it, vi } from "vitest";
import { dismissable } from "./dismiss";

afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });

it("leaves the trigger to toggle its menu and closes on an outside press or Escape", () => {
  vi.useFakeTimers();
  const handlers = new Map<string, (event: unknown) => void>();
  vi.stubGlobal("window", {
    addEventListener: (type: string, listener: (event: unknown) => void) => handlers.set(type, listener),
    removeEventListener: (type: string) => handlers.delete(type),
  });
  const inside = {}, trigger = {}, outside = {};
  const close = vi.fn();
  const action = dismissable({ contains: target => target === inside } as HTMLElement, {
    onClose: close, trigger: { contains: target => target === trigger } as HTMLElement,
  });
  vi.runAllTimers();
  handlers.get("pointerdown")!({ target: trigger });
  handlers.get("pointerdown")!({ target: inside });
  expect(close).not.toHaveBeenCalled();
  handlers.get("pointerdown")!({ target: outside });
  expect(close).toHaveBeenCalledOnce();
  handlers.get("keydown")!({ key: "Escape" });
  expect(close).toHaveBeenCalledTimes(2);
  action.destroy();
});
