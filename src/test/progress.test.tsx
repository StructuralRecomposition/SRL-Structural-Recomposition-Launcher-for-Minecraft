import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ProgressBar } from "../components/ui";

describe("ProgressBar", () => {
  it("shows a real percentage when total is known", () => {
    render(<ProgressBar value={42} label="42%" />);
    const bar = screen.getByRole("progressbar");
    expect(bar).toHaveAttribute("aria-valuenow", "42");
    expect(screen.getByText("42%")).toBeInTheDocument();
  });

  it("does not fabricate a percentage when total is unknown", () => {
    render(<ProgressBar value={null} label="已下载 1.2 MB · 总量未知" />);
    const bar = screen.getByRole("progressbar");
    expect(bar).not.toHaveAttribute("aria-valuenow");
    expect(bar.className).toContain("progress--indeterminate");
    expect(screen.getByText("已下载 1.2 MB · 总量未知")).toBeInTheDocument();
  });
});
