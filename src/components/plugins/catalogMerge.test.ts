import { describe, expect, it } from 'vitest';

import {
  catalogRowStatus,
  mergeCatalogApps,
  permissionConfirmationComplete
} from './catalogMerge';
import type { CatalogSnapshot, InstalledPlugin, PluginOperationSnapshot } from './types';

const installedCodec: InstalledPlugin = {
  appId: 'com.attson.codec',
  name: '编码转换',
  version: '1.0.0',
  publisherId: 'com.attson',
  publisherName: 'Attson',
  publisherKeyId: 'codec-2026-01',
  entry: 'ui/index.html',
  windows: {},
  permissions: ['host.theme.read'],
  enabled: true,
  installedAt: '2026-11-10T00:00:00Z'
};

const snapshot: CatalogSnapshot = {
  rootSequence: 3,
  catalogSequence: 18,
  generatedAt: '2026-11-10T08:00:00Z',
  expiresAt: '2026-11-17T08:00:00Z',
  expired: false,
  apps: [
    {
      appId: 'com.attson.codec',
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

const downloadOperation: PluginOperationSnapshot = {
  operationId: 'operation-1',
  correlationId: 'correlation-1',
  appId: installedCodec.appId,
  name: installedCodec.name,
  version: '1.1.0',
  publisherId: installedCodec.publisherId,
  publisherName: installedCodec.publisherName,
  publisherKeyId: installedCodec.publisherKeyId,
  entry: installedCodec.entry,
  windows: {},
  permissions: installedCodec.permissions,
  nativeService: null,
  stage: 'download',
  steps: ['download'],
  downloadedBytes: 29,
  totalBytes: 100,
  errorCode: null,
  message: null,
  finalAvailableVersion: null
};

describe('mergeCatalogApps', () => {
  it('joins the trusted catalog with the installed version by appId', () => {
    const [codec] = mergeCatalogApps(snapshot, [installedCodec]);

    expect(codec.name).toBe('编码转换');
    expect(codec.latestVersion).toBe('1.1.0');
    expect(codec.installedVersion).toBe('1.0.0');
    expect(codec.enabled).toBe(true);
    expect(codec.remoteActionsAllowed).toBe(true);
    expect(codec.updateAvailable).toBe(true);
    expect(codec.addedPermissions).toEqual([]);
  });

  it('does not offer an update when the catalog version is not strictly newer', () => {
    const sameVersion = {
      ...snapshot,
      apps: [{ ...snapshot.apps[0], latestVersion: installedCodec.version }]
    };

    const [codec] = mergeCatalogApps(sameVersion, [installedCodec]);

    expect(codec.updateAvailable).toBe(false);
  });

  it('orders prerelease identifiers that contain hyphens', () => {
    const prereleaseCatalog = {
      ...snapshot,
      apps: [{ ...snapshot.apps[0], latestVersion: '1.0.0-alpha-2' }]
    };
    const prereleaseInstalled = [{ ...installedCodec, version: '1.0.0-alpha-1' }];

    const [codec] = mergeCatalogApps(prereleaseCatalog, prereleaseInstalled);

    expect(codec.updateAvailable).toBe(true);
  });

  it('keeps installed state visible but disables remote actions for expired metadata', () => {
    const [codec] = mergeCatalogApps({ ...snapshot, expired: true }, [installedCodec]);

    expect(codec.installedVersion).toBe('1.0.0');
    expect(codec.remoteActionsAllowed).toBe(false);
  });

  it('separates added and removed permissions for an upgrade', () => {
    const changedPermissions = {
      ...snapshot,
      apps: [{
        ...snapshot.apps[0],
        permissions: ['host.file.save']
      }]
    };

    const [codec] = mergeCatalogApps(changedPermissions, [installedCodec]);

    expect(codec.addedPermissions).toEqual(['host.file.save']);
    expect(codec.removedPermissions).toEqual(['host.theme.read']);
  });

  it('requires the confirmed permission set to match exactly', () => {
    const expected = ['host.file.save', 'host.notification.show'];

    expect(permissionConfirmationComplete(expected, ['host.file.save'])).toBe(false);
    expect(permissionConfirmationComplete(expected, [...expected, 'host.theme.read'])).toBe(false);
    expect(permissionConfirmationComplete(expected, [expected[1], expected[0]])).toBe(true);
    expect(permissionConfirmationComplete([], [])).toBe(true);
  });
});

describe('catalogRowStatus', () => {
  it('marks an installed app with a newer catalog version as upgradable', () => {
    const [codec] = mergeCatalogApps(snapshot, [installedCodec]);

    expect(catalogRowStatus(codec, null)).toEqual({
      label: '可升级',
      type: 'warning'
    });
  });

  it('shows download progress before the available update state', () => {
    const [codec] = mergeCatalogApps(snapshot, [installedCodec]);

    expect(catalogRowStatus(codec, downloadOperation)).toEqual({
      label: '下载 29%',
      type: 'info'
    });
  });

  it('clamps download progress to the zero-to-one-hundred range', () => {
    const [codec] = mergeCatalogApps(snapshot, [installedCodec]);

    expect(catalogRowStatus(codec, {
      ...downloadOperation,
      downloadedBytes: -1
    }).label).toBe('下载 0%');
    expect(catalogRowStatus(codec, {
      ...downloadOperation,
      downloadedBytes: 101
    }).label).toBe('下载 100%');
  });

  it('keeps an installed app without an update marked as installed', () => {
    const currentSnapshot = {
      ...snapshot,
      apps: [{ ...snapshot.apps[0], latestVersion: installedCodec.version }]
    };
    const [codec] = mergeCatalogApps(currentSnapshot, [installedCodec]);

    expect(catalogRowStatus(codec, null)).toEqual({
      label: '已安装',
      type: 'success'
    });
  });

  it('keeps an uninstalled app marked as available', () => {
    const [codec] = mergeCatalogApps(snapshot, []);

    expect(catalogRowStatus(codec, null)).toEqual({
      label: '可获取',
      type: 'default'
    });
  });
});
