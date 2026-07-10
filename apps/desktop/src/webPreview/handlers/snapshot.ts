import type { UiSnapshot } from "@/appTypes";

import { webMockSnapshot } from "../state";

export const snapshotHandlers = {
  app_snapshot: (): UiSnapshot => ({ ...webMockSnapshot }),
};
