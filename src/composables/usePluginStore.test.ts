import { describe, expect, it, vi } from 'vitest';

import { catalogToolGroup, usePluginStore } from './usePluginStore';
import type {
  CatalogSnapshot,
  InstalledPlugin,
  PluginApi,
  PluginOperationSnapshot
} from '../components/plugins/types';

const codec: InstalledPlugin = {
  appId: 'com.attson.codec',
  name: '编码转换',
  version: '0.1.0-alpha.0',
  publisherId: 'com.attson',
  publisherName: 'Attson',
  publisherKeyId: 'attool-p0-test',
  entry: 'ui/index.html',
  windows: {},
  permissions: ['host.theme.read'],
  enabled: true,
  installedAt: '2026-11-10T00:00:00Z'
};

const catalog: CatalogSnapshot = {
  rootSequence: 3,
  catalogSequence: 18,
  generatedAt: '2026-11-10T08:00:00Z',
  expiresAt: '2026-11-17T08:00:00Z',
  expired: false,
  apps: [
    {
      appId: codec.appId,
      name: '编码转换',
      description: 'Base64、URL、Unicode、Hex、Hash 与 JWT',
      publisherId: 'com.attson',
      publisherName: 'Attson',
      icon: 'codec',
      latestVersion: '1.1.0',
      publishedAt: '2026-11-10T07:00:00Z',
      notes: '新增一键复制转换结果',
      publisherKeyId: 'codec-2026-01',
      hostApiMin: '1.0.0',
      hostApiMax: '1.x',
      permissions: ['host.theme.read'],
      target: {
        os: 'linux',
        arch: 'x86_64',
        url: 'https://github.com/attsontools/atcodec/releases/download/v1.1.0/AT.Codec_1.1.0_linux_x86_64.atpkg',
        fileName: 'AT.Codec_1.1.0_linux_x86_64.atpkg',
        size: 4096,
        sha256: 'a'.repeat(64)
      }
    }
  ]
};

describe('catalogToolGroup', () => {
  it.each([
    'com.attson.codec',
    'com.attson.generator',
    'com.attson.time',
    'com.attson.json',
    'com.attson.clipboard'
  ])('keeps the official utility app %s under utility navigation', (appId) => {
    expect(catalogToolGroup(appId)).toBe('utility');
  });

  it('falls back to utility navigation for an unknown app', () => {
    expect(catalogToolGroup('com.example.unknown')).toBe('utility');
  });
});

function fakeApi(): PluginApi {
  return {
    inspectPackage: vi.fn(),
    installPackage: vi.fn(),
    listInstalled: vi.fn().mockResolvedValue([{ ...codec }]),
    listNativeServiceStatuses: vi.fn().mockResolvedValue([]),
    listenNativeServiceStatuses: vi.fn().mockResolvedValue(() => {}),
    listHistory: vi.fn().mockResolvedValue([]),
    listCatalog: vi.fn().mockResolvedValue({ ...catalog }),
    refreshCatalog: vi.fn().mockResolvedValue({ ...catalog }),
    installCatalogApp: vi.fn(),
    updateCatalogApp: vi.fn(),
    completeCatalogInstall: vi.fn(),
    failCatalogInstall: vi.fn(),
    getOperation: vi.fn(),
    getLatestOperation: vi.fn().mockResolvedValue(null),
    getLatestOperations: vi.fn().mockResolvedValue([]),
    listenOperations: vi.fn().mockResolvedValue(() => {}),
    confirmPluginLaunch: vi.fn(),
    rollbackPluginLaunch: vi.fn(),
    setEnabled: vi.fn().mockResolvedValue(undefined),
    uninstall: vi.fn().mockResolvedValue(undefined)
  };
}

describe('usePluginStore', () => {
  it('keeps the newest native service status from snapshots and events', async () => {
    const api = fakeApi();
    let statusListener: (status: {
      appId: string;
      state: 'connecting' | 'connected' | 'recovering' | 'unavailable' | 'disconnected';
      errorCode: string | null;
      message: string | null;
      correlationId: string | null;
      updatedAt: number;
    }) => void = () => {};
    vi.mocked(api.listNativeServiceStatuses).mockResolvedValue([{
      appId: codec.appId,
      state: 'unavailable',
      errorCode: 'NATIVE_SERVICE_START_FAILED',
      message: 'Native service failed to start',
      correlationId: 'correlation-old',
      updatedAt: 10
    }]);
    vi.mocked(api.listenNativeServiceStatuses).mockImplementation(async (listener) => {
      statusListener = listener;
      return () => {};
    });
    const store = usePluginStore(api);

    await store.load();
    statusListener({
      appId: codec.appId,
      state: 'connected',
      errorCode: null,
      message: null,
      correlationId: 'correlation-new',
      updatedAt: 20
    });
    statusListener({
      appId: codec.appId,
      state: 'unavailable',
      errorCode: 'STALE',
      message: 'stale',
      correlationId: 'correlation-stale',
      updatedAt: 15
    });

    expect(store.nativeServiceStatuses.value.get(codec.appId)).toMatchObject({
      state: 'connected',
      correlationId: 'correlation-new',
      updatedAt: 20
    });
  });

  it('keeps a disabled plugin in management but removes it from navigation', async () => {
    const api = fakeApi();
    const store = usePluginStore(api);
    await store.load();

    expect(store.navigationTools.value.map((tool) => tool.id)).toEqual([
      'plugin:com.attson.codec'
    ]);

    await store.setEnabled('com.attson.codec', false);

    expect(api.setEnabled).toHaveBeenCalledWith('com.attson.codec', false);
    expect(store.apps.value).toHaveLength(1);
    expect(store.apps.value[0].enabled).toBe(false);
    expect(store.navigationTools.value).toEqual([]);
  });

  it('groups enabled download plugins under download navigation', async () => {
    const api = fakeApi();
    vi.mocked(api.listInstalled).mockResolvedValue([
      { ...codec, appId: 'com.attson.aria2', name: 'Aria2 下载' },
      { ...codec, appId: 'com.attson.video', name: '视频链接抽取' }
    ]);
    const store = usePluginStore(api);

    await store.load();

    expect(store.navigationTools.value.map((tool) => tool.group)).toEqual([
      'download',
      'download'
    ]);
  });

  it('groups enabled editing plugins under edit navigation', async () => {
    const api = fakeApi();
    vi.mocked(api.listInstalled).mockResolvedValue([
      { ...codec, appId: 'com.attson.text', name: '文本工具' },
      { ...codec, appId: 'com.attson.template', name: '主图模板' }
    ]);
    const store = usePluginStore(api);

    await store.load();

    expect(store.navigationTools.value.map((tool) => tool.group)).toEqual([
      'edit',
      'edit'
    ]);
  });

  it('groups the enabled HTTP plugin under network navigation', async () => {
    const api = fakeApi();
    vi.mocked(api.listInstalled).mockResolvedValue([
      { ...codec, appId: 'com.attson.http', name: 'HTTP 请求' }
    ]);
    const store = usePluginStore(api);

    await store.load();

    expect(store.navigationTools.value[0].group).toBe('network');
  });

  it('does not report a completed install as failed when history refresh fails', async () => {
    const api = fakeApi();
    vi.mocked(api.installPackage).mockResolvedValue({ ...codec });
    vi.mocked(api.listHistory).mockRejectedValue(new Error('history unavailable'));
    const store = usePluginStore(api);

    await expect(store.install('/tmp/codec.atpkg')).resolves.toEqual(codec);
    expect(store.apps.value).toEqual([codec]);
  });

  it('does not report a completed state change as failed when history refresh fails', async () => {
    const api = fakeApi();
    const store = usePluginStore(api);
    await store.load();
    vi.mocked(api.listHistory).mockRejectedValue(new Error('history unavailable'));

    await expect(store.setEnabled(codec.appId, false)).resolves.toBeUndefined();
    expect(store.apps.value[0].enabled).toBe(false);
  });

  it('does not report a completed uninstall as failed when history refresh fails', async () => {
    const api = fakeApi();
    const store = usePluginStore(api);
    await store.load();
    vi.mocked(api.listHistory).mockRejectedValue(new Error('history unavailable'));

    await expect(store.uninstall(codec.appId, false)).resolves.toBeUndefined();
    expect(store.apps.value).toEqual([]);
  });

  it('loads the cached catalog and merges the installed version', async () => {
    const store = usePluginStore(fakeApi());

    await store.load();

    expect(store.catalogApps.value).toHaveLength(1);
    expect(store.catalogApps.value[0]).toMatchObject({
      name: '编码转换',
      latestVersion: '1.1.0',
      installedVersion: '0.1.0-alpha.0'
    });
  });

  it('uses the catalog business icon for installed app navigation', async () => {
    const store = usePluginStore(fakeApi());

    await store.load();

    expect(store.navigationTools.value[0].icon).toBe('hash');
  });

  it('updates fallback navigation icons after the catalog is refreshed', async () => {
    const api = fakeApi();
    vi.mocked(api.listCatalog).mockResolvedValue(null);
    const store = usePluginStore(api);

    await store.load();
    expect(store.navigationTools.value[0].icon).toBe('apps');

    await store.refreshCatalog();
    expect(store.navigationTools.value[0].icon).toBe('hash');
  });

  it('maps only supported catalog icons and falls back for unknown apps', async () => {
    const api = fakeApi();
    const icons = [
      ['codec', 'hash'],
      ['type', 'type'],
      ['dice', 'dice'],
      ['clock', 'clock'],
      ['code', 'code'],
      ['http', 'send'],
      ['clipboard', 'clipboard'],
      ['video', 'video']
    ] as const;
    vi.mocked(api.listInstalled).mockResolvedValue([
      ...icons.map(([icon], index) => ({
        ...codec,
        appId: `com.attson.${icon}`,
        name: icon,
        installedAt: `2026-11-10T00:00:0${index}Z`
      })),
      { ...codec, appId: 'com.example.unknown', name: 'Unknown' }
    ]);
    vi.mocked(api.listCatalog).mockResolvedValue({
      ...catalog,
      apps: icons.map(([icon], index) => ({
        ...catalog.apps[0],
        appId: `com.attson.${icon}`,
        name: icon,
        icon,
        publishedAt: `2026-11-10T07:00:0${index}Z`
      }))
    });
    const store = usePluginStore(api);

    await store.load();

    expect(store.navigationTools.value.map((tool) => tool.icon)).toEqual([
      ...icons.map(([, expected]) => expected),
      'apps'
    ]);
  });

  it('loads installed apps when operation event subscription is unavailable', async () => {
    const api = fakeApi();
    vi.mocked(api.listenOperations).mockRejectedValue(new Error('event bridge unavailable'));
    const store = usePluginStore(api);

    await store.load();

    expect(store.apps.value).toEqual([codec]);
    expect(store.catalog.value?.catalogSequence).toBe(18);
    expect(store.error.value).toMatchObject({
      code: 'PLUGIN_UNKNOWN_ERROR',
      message: 'Error: event bridge unavailable'
    });
  });

  it('restores the latest persisted operation when the management page reloads', async () => {
    const api = fakeApi();
    const persisted: PluginOperationSnapshot = {
      operationId: 'operation-refresh',
      correlationId: 'correlation-refresh',
      appId: codec.appId,
      name: codec.name,
      version: '1.1.0',
      publisherId: codec.publisherId,
      publisherName: codec.publisherName,
      publisherKeyId: codec.publisherKeyId,
      entry: codec.entry,
      windows: {},
      permissions: codec.permissions,
      nativeService: null,
      stage: 'health-check' as const,
      steps: ['download', 'verify', 'staging', 'health-check'],
      downloadedBytes: 4096,
      totalBytes: 4096,
      errorCode: null,
      message: null,
      finalAvailableVersion: codec.version
    };
    vi.mocked(api.getLatestOperations).mockResolvedValue([persisted]);
    const store = usePluginStore(api);

    await store.load();

    expect(store.operation.value).toEqual(persisted);
  });

  it('restores the latest persisted operation for each application', async () => {
    const codecOperation: PluginOperationSnapshot = {
      operationId: 'operation-codec',
      correlationId: 'correlation-codec',
      appId: codec.appId,
      name: codec.name,
      version: '1.1.0',
      publisherId: codec.publisherId,
      publisherName: codec.publisherName,
      publisherKeyId: codec.publisherKeyId,
      entry: codec.entry,
      windows: {},
      permissions: codec.permissions,
      nativeService: null,
      stage: 'download',
      steps: ['download'],
      downloadedBytes: 1024,
      totalBytes: 4096,
      errorCode: null,
      message: null,
      finalAvailableVersion: codec.version
    };
    const clipboardOperation: PluginOperationSnapshot = {
      ...codecOperation,
      operationId: 'operation-clipboard',
      correlationId: 'correlation-clipboard',
      appId: 'com.attson.clipboard',
      name: '剪贴板',
      version: '1.0.6',
      stage: 'verify',
      steps: ['download', 'verify'],
      downloadedBytes: 4096
    };
    const api = fakeApi();
    vi.mocked(api.getLatestOperations).mockResolvedValue([codecOperation, clipboardOperation]);
    const store = usePluginStore(api);

    await store.load();

    expect(store.operationForApp(codec.appId)).toEqual(codecOperation);
    expect(store.operationForApp('com.attson.clipboard')).toEqual(clipboardOperation);
    expect(store.operationIsActive(codec.appId)).toBe(false);
    expect(store.operationIsActive('com.attson.clipboard')).toBe(false);
  });

  it('keeps operation events independent for different applications', async () => {
    const api = fakeApi();
    let operationListener: (operation: PluginOperationSnapshot) => void = () => {};
    vi.mocked(api.listenOperations).mockImplementation(async (listener) => {
      operationListener = listener;
      return () => {};
    });
    const codecOperation: PluginOperationSnapshot = {
      operationId: 'operation-codec',
      correlationId: 'correlation-codec',
      appId: codec.appId,
      name: codec.name,
      version: '1.1.0',
      publisherId: codec.publisherId,
      publisherName: codec.publisherName,
      publisherKeyId: codec.publisherKeyId,
      entry: codec.entry,
      windows: {},
      permissions: codec.permissions,
      nativeService: null,
      stage: 'download',
      steps: ['download'],
      downloadedBytes: 1024,
      totalBytes: 4096,
      errorCode: null,
      message: null,
      finalAvailableVersion: codec.version
    };
    const clipboardOperation: PluginOperationSnapshot = {
      ...codecOperation,
      operationId: 'operation-clipboard',
      correlationId: 'correlation-clipboard',
      appId: 'com.attson.clipboard',
      name: '剪贴板',
      version: '1.0.6',
      stage: 'verify',
      steps: ['download', 'verify'],
      downloadedBytes: 4096
    };
    const store = usePluginStore(api);
    await store.load();

    operationListener(codecOperation);
    operationListener(clipboardOperation);

    expect(store.operationForApp(codec.appId)).toEqual(codecOperation);
    expect(store.operationForApp('com.attson.clipboard')).toEqual(clipboardOperation);
    expect(store.operationIsActive(codec.appId)).toBe(true);
    expect(store.operationIsActive('com.attson.clipboard')).toBe(true);

    operationListener({ ...codecOperation, stage: 'error', steps: ['download', 'error'] });

    expect(store.operationIsActive(codec.appId)).toBe(false);
    expect(store.operationIsActive('com.attson.clipboard')).toBe(true);
  });

  it('preserves the cached catalog when refresh fails', async () => {
    const api = fakeApi();
    const store = usePluginStore(api);
    await store.load();
    vi.mocked(api.refreshCatalog).mockRejectedValue({
      code: 'CATALOG_SIGNATURE_INVALID',
      message: 'Catalog signature verification failed',
      correlationId: 'catalog-correlation'
    });

    await expect(store.refreshCatalog()).rejects.toMatchObject({
      code: 'CATALOG_SIGNATURE_INVALID'
    });
    expect(store.catalog.value?.catalogSequence).toBe(18);
    expect(store.catalogError.value).toMatchObject({
      code: 'CATALOG_SIGNATURE_INVALID',
      correlationId: 'catalog-correlation'
    });
  });

  it('keeps a remote install out of navigation until health check activation completes', async () => {
    const api = fakeApi();
    const pending: PluginOperationSnapshot = {
      operationId: 'operation-1',
      correlationId: 'correlation-1',
      appId: codec.appId,
      name: codec.name,
      version: codec.version,
      publisherId: codec.publisherId,
      publisherName: codec.publisherName,
      publisherKeyId: codec.publisherKeyId,
      entry: codec.entry,
      windows: {},
      permissions: codec.permissions,
      nativeService: null,
      stage: 'health-check' as const,
      steps: ['download', 'verify', 'staging', 'health-check'],
      downloadedBytes: 4096,
      totalBytes: 4096,
      errorCode: null,
      message: null,
      finalAvailableVersion: null
    };
    vi.mocked(api.listInstalled).mockResolvedValue([]);
    vi.mocked(api.installCatalogApp).mockResolvedValue(pending);
    vi.mocked(api.completeCatalogInstall).mockResolvedValue({
      operation: { ...pending, stage: 'complete', steps: [...pending.steps, 'complete'] },
      installed: codec
    });
    const store = usePluginStore(api);
    await store.load();

    await store.installCatalog(codec.appId, codec.permissions);
    expect(store.navigationTools.value).toEqual([]);
    expect(store.operation.value?.stage).toBe('health-check');
    expect(store.operationForApp(codec.appId)?.stage).toBe('health-check');

    await store.completeCatalogInstall('operation-1');
    expect(store.operationForApp(codec.appId)?.stage).toBe('complete');
    expect(store.navigationTools.value.map((tool) => tool.id)).toEqual([
      'plugin:com.attson.codec'
    ]);
  });

  it('starts an upgrade with no added permissions without replacing the current version', async () => {
    const api = fakeApi();
    const pending: PluginOperationSnapshot = {
      operationId: 'operation-update',
      correlationId: 'correlation-update',
      appId: codec.appId,
      name: codec.name,
      version: '1.1.0',
      publisherId: codec.publisherId,
      publisherName: codec.publisherName,
      publisherKeyId: codec.publisherKeyId,
      entry: codec.entry,
      windows: {},
      permissions: codec.permissions,
      nativeService: null,
      stage: 'health-check' as const,
      steps: ['download', 'verify', 'staging', 'health-check'],
      downloadedBytes: 4096,
      totalBytes: 4096,
      errorCode: null,
      message: null,
      finalAvailableVersion: codec.version
    };
    vi.mocked(api.updateCatalogApp).mockResolvedValue(pending);
    const store = usePluginStore(api);
    await store.load();

    await store.updateCatalog(codec.appId, []);

    expect(api.updateCatalogApp).toHaveBeenCalledWith(codec.appId, []);
    expect(store.apps.value[0].version).toBe(codec.version);
    expect(store.operation.value).toEqual(pending);
    expect(store.operationForApp(codec.appId)).toEqual(pending);
  });

  it('replaces the current navigation version after automatic rollback', async () => {
    const api = fakeApi();
    const rolledBack = { ...codec, version: '0.1.0-alpha.0' };
    const store = usePluginStore(api);
    await store.load();

    store.replaceInstalled({ ...codec, version: '1.1.0' });
    store.replaceInstalled(rolledBack);

    expect(store.apps.value).toEqual([rolledBack]);
    expect(store.navigationTools.value[0].name).toBe('编码转换');
  });
});
