import "@testing-library/jest-dom/vitest";
import { clearMocks } from "@tauri-apps/api/mocks";
import { afterEach } from "vitest";

afterEach(() => {
  // Tauri IPC mocks live on `window`; node-environment tests have none.
  if (typeof window !== "undefined") clearMocks();
});
