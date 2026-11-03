import "@testing-library/jest-dom/vitest";
import { vi } from "vitest";

// jsdom has no canvas implementation; skin previews are visual-only in tests.
Object.defineProperty(HTMLCanvasElement.prototype, "getContext", {
  configurable: true,
  value: vi.fn(() => null),
});
