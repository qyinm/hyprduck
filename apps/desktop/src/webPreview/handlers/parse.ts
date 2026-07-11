import type { ActiveJobSnapshot, FileSelection, UiSnapshot } from "@/appTypes";

import { WEB_MOCK_MARKDOWN, WEB_MOCK_SAMPLE_FILE } from "../fixtures";
import {
  emitWebSnapshot,
  setWebMockParseTimer,
  webMockParseTimer,
  webMockSnapshot,
} from "../state";

export const parseHandlers = {
  pick_import_file: () => ({ ...WEB_MOCK_SAMPLE_FILE }),
  start_parse: (args: { request: FileSelection }) => {
    const filePath = args.request.path;
    const format = args.request.format;
    const started: ActiveJobSnapshot = {
      jobId: `preview-${Date.now()}`,
      filePath,
      format,
      status: "parsing",
      progressPercent: 0,
      lastMessage: "Preview parse started.",
    };
    if (webMockParseTimer) {
      clearTimeout(webMockParseTimer);
    }
    emitWebSnapshot({
      ...webMockSnapshot,
      activeJob: started,
      lastResult: webMockSnapshot.lastResult,
      progressLog: [
        ...webMockSnapshot.progressLog,
        {
          phase: "parse",
          message: "Using mocked web preview parser.",
          timestamp: new Date().toISOString(),
        },
      ],
    });
    const timer = setTimeout(() => {
      const completedSnapshot: UiSnapshot = {
        ...webMockSnapshot,
        activeJob: null,
        lastProjectId: "preview:sample",
        workspaceRevision: (webMockSnapshot.workspaceRevision ?? 0) + 1,
        lastResult: {
          savedOutputPath: `~/Library/Application Support/Etyma/web-preview/${new Date()
            .toISOString()
            .slice(0, 10)}.md`,
          successCount: 2,
          failedCount: 0,
          markdown: WEB_MOCK_MARKDOWN,
        },
        progressLog: [
          ...webMockSnapshot.progressLog,
          {
            phase: "parse",
            message: "Preview parse completed.",
            timestamp: new Date().toISOString(),
          },
        ],
      };
      emitWebSnapshot(completedSnapshot);
      setWebMockParseTimer(null);
    }, 700);
    setWebMockParseTimer(timer);
  },
  retry_failed_pages: () => {
    emitWebSnapshot({
      ...webMockSnapshot,
      progressLog: [
        ...webMockSnapshot.progressLog,
        {
          phase: "retry",
          message: "Preview failed-page retry completed.",
          timestamp: new Date().toISOString(),
        },
      ],
    });
  },
  cancel_parse: () => {
    if (webMockParseTimer) {
      clearTimeout(webMockParseTimer);
      setWebMockParseTimer(null);
    }
    const current = webMockSnapshot;
    if (current.activeJob) {
      emitWebSnapshot({
        ...current,
        activeJob: null,
        progressLog: [
          ...current.progressLog,
          {
            phase: "parse",
            message: "Preview parse canceled.",
            timestamp: new Date().toISOString(),
          },
        ],
      });
    }
  },
};
