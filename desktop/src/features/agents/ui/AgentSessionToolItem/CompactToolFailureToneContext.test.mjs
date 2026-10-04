import assert from "node:assert/strict";
import test from "node:test";

import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  CompactToolFailureToneContext,
  CompactToolSummaryRow,
} from "./CompactToolSummaryRow.tsx";

const failedRun = {
  action: { verb: "Ran", object: "python3 demo/does_not_exist.py" },
  duration: "1.2s",
  failed: true,
  fileEditSummary: null,
  kind: "shell",
  label: "Ran command failed",
  preview: "python3 demo/does_not_exist.py",
  thumbnailSrc: null,
};

function render(props, tone) {
  const row = React.createElement(CompactToolSummaryRow, props);
  return renderToStaticMarkup(
    tone
      ? React.createElement(
          CompactToolFailureToneContext.Provider,
          { value: tone },
          row,
        )
      : row,
  );
}

test("failureTone_withoutAProvider_staysAlarm", () => {
  // A row that does not know it sits in a fold keeps the loud row.
  const markup = render(failedRun);
  assert.match(markup, /Tool call failed/);
  assert.doesNotMatch(markup, /data-failure-tone="quiet"/);
});

test("failureTone_insideTheFoldProvider_readsQuietAndStillSaysFailed", () => {
  const markup = render({ ...failedRun, failureDetail: "exit 2" }, "quiet");
  assert.doesNotMatch(markup, /Tool call failed/);
  assert.match(markup, /data-failure-tone="quiet"/);
  assert.match(markup, /aria-label="Failed"/);
  assert.match(markup, /exit 2/);
  // The quiet label sets its own muted colour, so a <summary> toned
  // destructive by its caller cannot repaint it red.
  assert.match(
    markup,
    /inline-flex min-w-0 items-center gap-1\.5 text-muted-foreground\/60/,
  );
});

test("failureTone_explicitProp_winsOverTheProvider", () => {
  const markup = render({ ...failedRun, failureTone: "alarm" }, "quiet");
  assert.match(markup, /Tool call failed/);
});

test("failureTone_quietProvider_leavesSuccessfulRowsAlone", () => {
  const markup = render({ ...failedRun, failed: false }, "quiet");
  assert.doesNotMatch(markup, /data-failure-tone/);
  assert.doesNotMatch(markup, /Failed/);
});
