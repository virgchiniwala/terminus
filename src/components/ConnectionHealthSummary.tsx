import type { EmailConnectionRecord } from "../types";
import { formatShortLocalTime, watcherStatusLine } from "../uiLogic";

type Props = {
  record: EmailConnectionRecord;
};

export function ConnectionHealthSummary({ record }: Props) {
  return (
    <>
      {record.lastError && <p>Connection issue: {record.lastError}</p>}
      <p>{watcherStatusLine(record)}</p>
      {(record.watcherConsecutiveFailures ?? 0) > 0 && (
        <p>Recent failures: {record.watcherConsecutiveFailures}</p>
      )}
      {record.watcherLastError && (
        <p>
          Last watcher issue: {record.watcherLastError}
          {record.watcherUpdatedAtMs ? ` (${formatShortLocalTime(record.watcherUpdatedAtMs)})` : ""}
        </p>
      )}
    </>
  );
}

