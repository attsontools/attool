import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { spawn } from 'node:child_process';

import { resolvePluginDevOverrides } from './plugin-dev-config';

const CONFIG_NAME = '.attool-plugins.local.json';
const OVERRIDES_ENV = 'ATTOOL_PLUGIN_DEV_OVERRIDES';

async function readConfig(repositoryRoot: string): Promise<Record<string, unknown>> {
  try {
    return JSON.parse(await readFile(path.join(repositoryRoot, CONFIG_NAME), 'utf8'));
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') return {};
    throw error;
  }
}

export async function runTauriDev(args = process.argv.slice(2)): Promise<number> {
  const repositoryRoot = path.resolve(import.meta.dirname, '..');
  const overrides = resolvePluginDevOverrides(await readConfig(repositoryRoot), repositoryRoot);
  const env = { ...process.env };
  if (Object.keys(overrides).length > 0) {
    env[OVERRIDES_ENV] = JSON.stringify(overrides);
    for (const [appId, uiRoot] of Object.entries(overrides)) {
      console.log(`[plugin-dev] ${appId} -> ${uiRoot}`);
    }
  } else {
    delete env[OVERRIDES_ENV];
  }

  const pnpmCli = process.env.npm_execpath;
  if (!pnpmCli) throw new Error('pnpm executable path is unavailable');
  const child = spawn(process.execPath, [pnpmCli, 'exec', 'tauri', 'dev', ...args], {
    cwd: repositoryRoot,
    env,
    stdio: 'inherit',
  });
  return await new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('exit', (code, signal) => {
      if (signal) reject(new Error(`tauri dev stopped by ${signal}`));
      else resolve(code ?? 1);
    });
  });
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  process.exitCode = await runTauriDev();
}
