import { expect, test } from "@playwright/test";

import { installMockBridge } from "../helpers/bridge";

/**
 * The Dashboard's "Relay machine" card: CPU, memory and disk as the relay
 * samples them (`GET /health/system`). Stewards see the numbers, a member
 * sees no card, and a relay without the endpoint is named as such.
 */

test("an owner sees the relay's machine with every figure named", async ({
  page,
}) => {
  await installMockBridge(page);
  await page.goto("/");

  const card = page.getByTestId("dashboard-card-relay-health");
  await expect(card).toBeVisible();
  await expect(page.getByTestId("dashboard-card-relay-health-cpu")).toHaveText(
    "12% of 4 cores · relay process 3% · load 0.52 · 0.40 · 0.31",
  );
  await expect(
    page.getByTestId("dashboard-card-relay-health-memory"),
  ).toHaveText("5.5 GiB of 16 GiB in use (34%) · relay process 120 MiB");
  await expect(
    page.getByTestId("dashboard-card-relay-health-disk-0"),
  ).toHaveText("git data, root · 40 GiB free of 100 GiB (60% used)");
  await expect(page.getByTestId("dashboard-card-relay-health-age")).toHaveText(
    "mock-relay · sampled just now",
  );
});

test("a member of a rostered relay gets no relay card at all", async ({
  page,
}) => {
  await installMockBridge(page, {
    relayRequiresMembership: true,
    relayRole: "member",
  });
  await page.goto("/");

  await expect(page.getByTestId("dashboard-overview")).toBeVisible();
  await expect(page.getByTestId("dashboard-card-inbox")).toBeVisible();
  await expect(page.getByTestId("dashboard-card-relay-health")).toHaveCount(0);
});

test("a relay that refuses the read hides the card; one without the endpoint says so", async ({
  page,
}) => {
  await installMockBridge(page, {
    relaySystemHealth: null,
    relaySystemHealthError:
      "relay returned 403 Forbidden: restricted: machine health is for this community's owners and admins",
  });
  await page.goto("/");
  await expect(page.getByTestId("dashboard-card-inbox")).toBeVisible();
  await expect(page.getByTestId("dashboard-card-relay-health")).toHaveCount(0);

  await installMockBridge(page, { relaySystemHealth: null });
  await page.goto("/");
  const note = page.getByTestId("dashboard-card-relay-health-note");
  await expect(note).toHaveText(
    "This relay does not report its machine yet — it predates the health endpoint.",
  );
});

test("a container limit is a second memory figure", async ({ page }) => {
  const gib = 1024 ** 3;
  await installMockBridge(page, {
    relaySystemHealth: {
      sampled_at: "2026-09-14T20:00:00Z",
      age_seconds: 45,
      interval_seconds: 10,
      host: {
        name: null,
        os: null,
        uptime_seconds: 1,
        relay_uptime_seconds: 1,
      },
      cpu: {
        cores: 2,
        machine_percent: 50,
        process_percent: 110.4,
        load_average: null,
      },
      memory: {
        machine_total_bytes: 8 * gib,
        machine_used_bytes: 2 * gib,
        machine_available_bytes: 6 * gib,
        swap_total_bytes: 0,
        swap_used_bytes: 0,
        process_rss_bytes: 1.2 * gib,
        container: { limit_bytes: 2 * gib, used_bytes: 1.2 * gib },
      },
      disks: [],
    },
  });
  await page.goto("/");

  await expect(page.getByTestId("dashboard-card-relay-health-cpu")).toHaveText(
    "50% of 2 cores · relay process 110%",
  );
  await expect(
    page.getByTestId("dashboard-card-relay-health-memory"),
  ).toHaveText(
    "2.0 GiB of 8.0 GiB in use (25%) · relay process 1.2 GiB · container 1.2 GiB of 2.0 GiB (60%)",
  );
  await expect(
    page.getByTestId("dashboard-card-relay-health-disk-none"),
  ).toHaveText("The relay could not measure a filesystem.");
  await expect(page.getByTestId("dashboard-card-relay-health-age")).toHaveText(
    "sampled 45 s ago (stale)",
  );
});
