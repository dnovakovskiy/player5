import { expect, test } from "@playwright/test";

test("loads, starts audio and the playhead advances", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto("/");
  await expect(page.getByRole("button", { name: "Play" })).toBeVisible();
  await expect(page.locator("#app")).toHaveAttribute("data-playing-step", "-1");

  await page.getByRole("button", { name: "Play" }).click();
  await expect(page.locator("#status")).toHaveAttribute("data-state", "running");
  await expect(page.getByRole("button", { name: "Stop" })).toBeVisible();

  // 124 BPM: a step is ~121 ms; step 2 should light within a second.
  await expect
    .poll(async () => Number(await page.locator("#app").getAttribute("data-playing-step")), {
      timeout: 5_000,
    })
    .toBeGreaterThanOrEqual(2);

  await page.getByRole("button", { name: "Stop" }).click();
  await expect(page.locator("#app")).toHaveAttribute("data-playing-step", "-1");
  expect(errors).toEqual([]);
});

test("steps cycle and the pattern round-trips through the URL", async ({ page }) => {
  await page.goto("/");
  const step = page.getByRole("button", { name: "step 2" });
  await expect(step).toHaveAttribute("data-state", "off");
  await step.click();
  await expect(step).toHaveAttribute("data-state", "on");
  await step.click();
  await expect(step).toHaveAttribute("data-state", "accent");

  const url = page.url();
  expect(url).toContain("#p=");
  await page.goto(url);
  await expect(page.getByRole("button", { name: "step 2" })).toHaveAttribute("data-state", "accent");
});
