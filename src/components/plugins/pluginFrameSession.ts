export interface PluginFrameSession {
  sessionId: string;
  launchToken: string;
  url: string;
}

export type PluginFrameFailureCategory = 'resource' | 'runtime' | 'preload';

interface FrameMessage {
  source: unknown;
  data: unknown;
}

interface FrameFailureContext {
  source: unknown;
  launchToken: string;
  revision: number;
  currentRevision: number;
  launchPending: boolean;
}

export function validatePluginFrameFailure(
  event: FrameMessage,
  context: FrameFailureContext
): PluginFrameFailureCategory | null {
  if (
    !context.launchPending
    || event.source !== context.source
    || context.revision !== context.currentRevision
  ) return null;
  if (!isRecord(event.data)) return null;
  if (event.data.type !== 'attool:frame-error') return null;
  if (event.data.launchToken !== context.launchToken) return null;
  return isFailureCategory(event.data.category) ? event.data.category : null;
}

export function pluginFrameFailureMessage(category: PluginFrameFailureCategory) {
  switch (category) {
    case 'resource':
      return '应用资源加载失败';
    case 'preload':
      return '应用模块加载失败';
    case 'runtime':
      return '应用启动脚本执行失败';
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function isFailureCategory(value: unknown): value is PluginFrameFailureCategory {
  return value === 'resource' || value === 'runtime' || value === 'preload';
}
