import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import App from "../App";
import { setLanguage } from "../lib/i18n";

afterEach(() => {
  setLanguage("zh-CN");
  cleanup();
});

describe("language switching", () => {
  it("renders Chinese by default", () => {
    render(<App />);
    expect(screen.getByText("SRL 需要在桌面应用中运行")).toBeInTheDocument();
  });

  it("renders the English shell when English is selected", () => {
    setLanguage("en");
    render(<App />);
    expect(screen.getByText("SRL must be run inside the desktop app")).toBeInTheDocument();
  });
});
