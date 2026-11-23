import { existsSync } from 'node:fs';
import path from 'node:path';

type PluginDevConfig = Record<string, unknown>;

const APP_ID_PATTERN = /^[a-z0-9]+(?:[.-][a-z0-9]+)+$/;

export function resolvePluginDevOverrides(
  config: PluginDevConfig,
  repositoryRoot: string,
  entryExists: (entry: string) => boolean = existsSync,
): Record<string, string> {
  if (!config || Array.isArray(config) || typeof config !== 'object') {
    throw new Error('Plugin development config must be a JSON object');
  }

  const overrides: Record<string, string> = {};
  for (const [appId, configuredPath] of Object.entries(config)) {
    if (!APP_ID_PATTERN.test(appId)) {
      throw new Error(`Invalid plugin app ID: ${appId}`);
    }
    if (typeof configuredPath !== 'string' || !configuredPath.trim()) {
      throw new Error(`${appId} development UI root must be a non-empty path`);
    }
    const uiRoot = path.resolve(repositoryRoot, configuredPath);
    const entry = path.join(uiRoot, 'index.html');
    if (!entryExists(entry)) {
      throw new Error(`${appId} UI entry does not exist: ${entry}`);
    }
    overrides[appId] = uiRoot;
  }
  return overrides;
}
