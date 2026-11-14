import { describe, expect, it, vi } from 'vitest';

import { createPluginHostSession, type HostMessagePort, type HostRpcMessage } from './pluginHost';
import type { InstalledPlugin } from './types';

class FakePort implements HostMessagePort {
  onmessage: ((event: MessageEvent<HostRpcMessage>) => void) | null = null;
  onPost: (() => void) | null = null;
  sent: HostRpcMessage[] = [];
  start() {}
  close() {}
  postMessage(message: HostRpcMessage) {
    this.sent.push(message);
    this.onPost?.();
  }
  receive(message: HostRpcMessage) {
    this.onmessage?.({ data: message } as MessageEvent<HostRpcMessage>);
  }
}

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

const http: InstalledPlugin = {
  ...codec,
  appId: 'com.attson.http',
  name: 'HTTP request',
  permissions: ['host.file.save', 'host.http.request', 'host.http.stream', 'host.http.data'],
  nativeService: {
    entry: 'native/athttp-service',
    version: '0.1.0-alpha.0',
    sha256: 'a'.repeat(64),
    protocolVersion: 1,
    lifecycle: 'onDemand'
  }
};

const aria2: InstalledPlugin = {
  ...codec,
  appId: 'com.attson.aria2',
  name: 'Aria2 下载',
  permissions: ['host.download.tasks'],
  nativeService: {
    entry: 'native/ataria2-service',
    version: '0.1.0-alpha.0',
    sha256: 'b'.repeat(64),
    protocolVersion: 1,
    lifecycle: 'onDemand'
  }
};

const template: InstalledPlugin = {
  ...codec,
  appId: 'com.attson.template',
  name: '主图模板',
  permissions: ['host.template.project'],
  nativeService: {
    entry: 'native/attemplate-service',
    version: '0.1.0-alpha.0',
    sha256: 'c'.repeat(64),
    protocolVersion: 1,
    lifecycle: 'onDemand'
  }
};

const clipboard: InstalledPlugin = {
  ...codec,
  appId: 'com.attson.clipboard',
  name: 'Clipboard',
  windows: {
    history: { entry: 'ui/history.html', presentation: 'bottomPanel' },
    preview: { entry: 'ui/preview.html', presentation: 'centeredDialog' }
  },
  permissions: ['host.ui.auxiliary', 'host.clipboard.history'],
  nativeService: {
    entry: 'native/atclipboard-service',
    version: '0.1.0-alpha.0',
    sha256: 'd'.repeat(64),
    protocolVersion: 1,
    lifecycle: 'background'
  }
};

function request(id: string, method: string, params: unknown = {}): HostRpcMessage {
  return {
    jsonrpc: '2.0',
    id,
    apiVersion: '1.0',
    appId: codec.appId,
    method,
    params
  };
}

async function flush() {
  await new Promise((resolve) => setTimeout(resolve, 0));
}

describe('plugin host session', () => {
  it('opens only signed auxiliary windows and delivers activation to the target session', async () => {
    const port = new FakePort();
    const openAuxiliaryWindow = vi.fn(async () => ({ opened: true }));
    const closeAuxiliaryWindow = vi.fn(async () => {});
    const session = createPluginHostSession({
      app: clipboard,
      port,
      windowId: 'history',
      theme: () => 'dark',
      notify: vi.fn(),
      saveFile: vi.fn(),
      openAuxiliaryWindow,
      closeAuxiliaryWindow,
      requestNativeService: vi.fn()
    });
    const clipboardRequest = (id: string, method: string, params: unknown = {}): HostRpcMessage => ({
      jsonrpc: '2.0', id, apiVersion: '2.0', appId: clipboard.appId, method, params
    });

    port.receive(clipboardRequest('1', 'host.handshake'));
    await flush();
    port.receive(clipboardRequest('2', 'host.ui.open', {
      windowId: 'preview', payload: { itemId: 'clip-1' }
    }));
    await flush();

    expect(openAuxiliaryWindow).toHaveBeenCalledWith({
      windowId: 'preview', payload: { itemId: 'clip-1' }
    });
    expect(port.sent.at(-1)).toMatchObject({ id: '2', result: { opened: true } });

    port.receive(clipboardRequest('3', 'host.ui.close'));
    await flush();
    expect(closeAuxiliaryWindow).toHaveBeenCalledOnce();
    expect(port.sent.at(-1)).toMatchObject({ id: '3', result: { closed: true } });

    session.notifyActivated({ source: 'shortcut' });
    expect(port.sent.at(-1)).toMatchObject({
      method: 'host.ui.activated',
      params: { windowId: 'history', payload: { source: 'shortcut' } }
    });
  });

  it('rejects unauthorized, unknown, and oversized auxiliary window requests', async () => {
    const openAuxiliaryWindow = vi.fn();
    const invoke = async (app: InstalledPlugin, id: string, params: unknown) => {
      const port = new FakePort();
      createPluginHostSession({
        app,
        port,
        theme: () => 'dark',
        notify: vi.fn(),
        saveFile: vi.fn(),
        openAuxiliaryWindow,
        requestNativeService: vi.fn()
      });
      const send = (requestId: string, method: string, requestParams: unknown): HostRpcMessage => ({
        jsonrpc: '2.0', id: requestId, apiVersion: '2.0', appId: app.appId,
        method, params: requestParams
      });
      port.receive(send('handshake', 'host.handshake', {}));
      await flush();
      port.receive(send(id, 'host.ui.open', params));
      await flush();
      return port.sent.at(-1);
    };

    await expect(invoke(
      { ...clipboard, permissions: ['host.clipboard.history'] },
      'permission',
      { windowId: 'history' }
    )).resolves.toMatchObject({ error: { data: { code: 'HOST_PERMISSION_DENIED' } } });
    await expect(invoke(clipboard, 'unknown', { windowId: 'settings' }))
      .resolves.toMatchObject({ error: { data: { code: 'HOST_UI_WINDOW_NOT_DECLARED' } } });
    await expect(invoke(clipboard, 'large', {
      windowId: 'history', payload: { value: 'x'.repeat(65 * 1024) }
    })).resolves.toMatchObject({ error: { data: { code: 'HOST_UI_PAYLOAD_TOO_LARGE' } } });
    expect(openAuxiliaryWindow).not.toHaveBeenCalled();
  });

  it('routes only the eight clipboard service methods', async () => {
    const port = new FakePort();
    const requestNativeService = vi.fn(async (method: string) => (
      method === 'clipboard.history.list' ? [] : { ok: true }
    ));
    createPluginHostSession({
      app: clipboard,
      port,
      theme: () => 'dark',
      notify: vi.fn(),
      saveFile: vi.fn(),
      requestNativeService
    });
    const send = (id: string, method: string, params: unknown = {}): HostRpcMessage => ({
      jsonrpc: '2.0', id, apiVersion: '2.0', appId: clipboard.appId, method, params
    });
    port.receive(send('1', 'host.handshake'));
    await flush();

    const allowed = [
      'clipboard.history.list', 'clipboard.history.get', 'clipboard.history.pin',
      'clipboard.history.delete', 'clipboard.history.clearUnpinned',
      'clipboard.history.restore', 'clipboard.settings.get', 'clipboard.settings.update'
    ];
    for (const [index, method] of allowed.entries()) {
      port.receive(send(String(index + 2), 'host.clipboard.history', { method, params: {} }));
      await flush();
    }
    expect(requestNativeService).toHaveBeenCalledTimes(8);

    port.receive(send('denied', 'host.clipboard.history', {
      method: 'clipboard.history.exportDatabase', params: {}
    }));
    await flush();
    expect(port.sent.at(-1)).toMatchObject({
      id: 'denied', error: { data: { code: 'HOST_CLIPBOARD_METHOD_DENIED' } }
    });
    expect(requestNativeService).toHaveBeenCalledTimes(8);
  });

  it('enforces the public clipboard DTO and list page boundary', async () => {
    const port = new FakePort();
    const requestNativeService = vi.fn(async (method: string): Promise<unknown> => {
      if (method === 'clipboard.history.list') {
        return Array.from({ length: 101 }, (_, index) => ({ id: `clip-${index}` }));
      }
      return {
        id: 'clip-image',
        kind: 'image',
        assetPath: '/home/user/.local/share/attool/clipboard/private.png'
      };
    });
    createPluginHostSession({
      app: clipboard,
      port,
      theme: () => 'dark',
      notify: vi.fn(),
      saveFile: vi.fn(),
      requestNativeService
    });
    const send = (id: string, method: string, params: unknown = {}): HostRpcMessage => ({
      jsonrpc: '2.0', id, apiVersion: '2.0', appId: clipboard.appId, method, params
    });
    port.receive(send('handshake', 'host.handshake'));
    await flush();

    port.receive(send('request-limit', 'host.clipboard.history', {
      method: 'clipboard.history.list', params: { limit: 101 }
    }));
    await flush();
    expect(port.sent.at(-1)).toMatchObject({
      id: 'request-limit', error: { data: { code: 'HOST_INVALID_PARAMS' } }
    });
    expect(requestNativeService).not.toHaveBeenCalled();

    port.receive(send('response-limit', 'host.clipboard.history', {
      method: 'clipboard.history.list', params: { limit: 100 }
    }));
    await flush();
    expect(port.sent.at(-1)).toMatchObject({
      id: 'response-limit', error: { data: { code: 'HOST_CLIPBOARD_RESPONSE_INVALID' } }
    });

    port.receive(send('path', 'host.clipboard.history', {
      method: 'clipboard.history.get', params: { id: 'clip-image' }
    }));
    await flush();
    expect(port.sent.at(-1)).toMatchObject({
      id: 'path', error: { data: { code: 'HOST_CLIPBOARD_RESPONSE_INVALID' } }
    });
    expect(JSON.stringify(port.sent.at(-1))).not.toContain('/home/user');

    requestNativeService.mockResolvedValueOnce({
      id: 'clip-image',
      kind: 'image',
      previewDataUrl: `data:image/png;base64,${'a'.repeat(2 * 1024 * 1024)}`
    });
    port.receive(send('preview-limit', 'host.clipboard.history', {
      method: 'clipboard.history.get', params: { id: 'clip-image' }
    }));
    await flush();
    expect(port.sent.at(-1)).toMatchObject({
      id: 'preview-limit', error: { data: { code: 'HOST_CLIPBOARD_RESPONSE_INVALID' } }
    });
  });

  it('exposes PSD selections as one-use template tokens', async () => {
    const port = new FakePort();
    const requestService = vi.fn(async () => ({ id: 'template-1' }));
    const pickTemplateFiles = vi.fn(async () => [
      { path: '/home/user/private/source.psd', name: 'source.psd' }
    ]);
    createPluginHostSession({
      app: template,
      port,
      theme: () => 'dark',
      notify: vi.fn(),
      saveFile: vi.fn(),
      pickTemplateFiles,
      requestNativeService: requestService
    });
    const templateRequest = (id: string, method: string, params: unknown = {}): HostRpcMessage => ({
      jsonrpc: '2.0', id, apiVersion: '2.0', appId: template.appId, method, params
    });

    port.receive(templateRequest('1', 'host.handshake'));
    await flush();
    port.receive(templateRequest('2', 'host.template.pickFiles', {
      kind: 'psd', multiple: false
    }));
    await flush();

    expect(pickTemplateFiles).toHaveBeenCalledWith({ kind: 'psd', multiple: false });
    const selection = (port.sent[1] as {
      result: { cancelled: false; files: Array<{ token: string; name: string }> }
    }).result.files[0]!;
    expect(selection.name).toBe('source.psd');
    expect(selection.token).not.toContain('/home/user');
    expect(JSON.stringify(port.sent[1])).not.toContain('/home/user');

    port.receive(templateRequest('3', 'host.template.project', {
      method: 'template.psd.import', params: { fileToken: selection.token }
    }));
    await flush();
    expect(requestService).toHaveBeenCalledWith('template.psd.import', {
      sourcePath: '/home/user/private/source.psd'
    });

    port.receive(templateRequest('4', 'host.template.project', {
      method: 'template.psd.import', params: { fileToken: selection.token }
    }));
    await flush();
    expect(port.sent.at(-1)).toMatchObject({
      id: '4', error: { data: { code: 'HOST_TEMPLATE_TOKEN_CONSUMED' } }
    });
    expect(requestService).toHaveBeenCalledTimes(1);
  });

  it('resolves image and directory tokens only for their declared template fields', async () => {
    const port = new FakePort();
    const requestService = vi.fn(async () => ({ ok: true }));
    createPluginHostSession({
      app: template,
      port,
      theme: () => 'dark',
      notify: vi.fn(),
      saveFile: vi.fn(),
      pickTemplateFiles: vi.fn(async () => [
        { path: '/home/user/a.png', name: 'a.png' },
        { path: '/home/user/b.jpg', name: 'b.jpg' }
      ]),
      pickTemplateDirectory: vi.fn(async () => ({
        path: '/home/user/Exports', name: 'Exports'
      })),
      requestNativeService: requestService
    });
    const templateRequest = (id: string, method: string, params: unknown = {}): HostRpcMessage => ({
      jsonrpc: '2.0', id, apiVersion: '2.0', appId: template.appId, method, params
    });

    port.receive(templateRequest('1', 'host.handshake'));
    await flush();
    port.receive(templateRequest('2', 'host.template.pickFiles', {
      kind: 'image', multiple: true
    }));
    await flush();
    const files = (port.sent[1] as {
      result: { files: Array<{ token: string; name: string }> }
    }).result.files;

    port.receive(templateRequest('3', 'host.template.project', {
      method: 'template.psd.import', params: { fileToken: files[0]!.token }
    }));
    await flush();
    expect(port.sent.at(-1)).toMatchObject({
      id: '3', error: { data: { code: 'HOST_TEMPLATE_TOKEN_TYPE_MISMATCH' } }
    });

    port.receive(templateRequest('4', 'host.template.project', {
      method: 'template.batch.sources.register',
      params: { fileTokens: files.map((file) => file.token) }
    }));
    await flush();
    expect(requestService).toHaveBeenLastCalledWith('template.batch.sources.register', {
      sourcePaths: ['/home/user/a.png', '/home/user/b.jpg']
    });

    port.receive(templateRequest('5', 'host.template.pickDirectory'));
    await flush();
    const directory = (port.sent.at(-1) as {
      result: { token: string; name: string }
    }).result;
    expect(directory).toMatchObject({ name: 'Exports' });
    expect(JSON.stringify(directory)).not.toContain('/home/user');

    port.receive(templateRequest('6', 'host.template.project', {
      method: 'template.batch.save',
      params: { outputIds: ['output-1'], directoryToken: directory.token }
    }));
    await flush();
    expect(requestService).toHaveBeenLastCalledWith('template.batch.save', {
      outputIds: ['output-1'], targetDirectory: '/home/user/Exports'
    });
  });

  it.each([
    { result: { outputs: [{ id: 'output-1', filePath: '/tmp/private/output.png' }] } },
    { result: { outputs: [{ id: 'output-1', url: '/tmp/private/output.png' }] } },
    { result: { outputs: [{ id: 'output-1', url: 'C:\\Users\\private\\output.png' }] } },
    { result: { outputs: [{ id: 'output-1', url: '\\\\server\\private\\output.png' }] } },
    { result: { outputs: [{ id: 'output-1', url: 'file:///tmp/private/output.png' }] } }
  ])('rejects local paths returned by template batch methods: $result', async ({ result }) => {
    const port = new FakePort();
    createPluginHostSession({
      app: template,
      port,
      theme: () => 'dark',
      notify: vi.fn(),
      saveFile: vi.fn(),
      requestNativeService: vi.fn(async () => result)
    });
    const templateRequest = (id: string, method: string, params: unknown = {}): HostRpcMessage => ({
      jsonrpc: '2.0', id, apiVersion: '2.0', appId: template.appId, method, params
    });

    port.receive(templateRequest('1', 'host.handshake'));
    await flush();
    port.receive(templateRequest('2', 'host.template.project', {
      method: 'template.batch.run', params: { templateId: 'template-1', tasks: [] }
    }));
    await flush();

    expect(port.sent.at(-1)).toMatchObject({
      id: '2', error: { data: { code: 'HOST_TEMPLATE_PATH_EXPOSED' } }
    });
    expect(JSON.stringify(port.sent.at(-1))).not.toContain('private');
  });

  it('does not expose Native Service error paths to template plugins', async () => {
    const port = new FakePort();
    createPluginHostSession({
      app: template,
      port,
      theme: () => 'dark',
      notify: vi.fn(),
      saveFile: vi.fn(),
      requestNativeService: vi.fn(async () => {
        throw {
          code: 'TEMPLATE_PSD_PARSE_FAILED',
          message: 'Cannot parse /home/user/private/source.psd'
        };
      })
    });
    const templateRequest = (id: string, method: string, params: unknown = {}): HostRpcMessage => ({
      jsonrpc: '2.0', id, apiVersion: '2.0', appId: template.appId, method, params
    });

    port.receive(templateRequest('1', 'host.handshake'));
    await flush();
    port.receive(templateRequest('2', 'host.template.project', {
      method: 'template.templates.list', params: {}
    }));
    await flush();

    expect(port.sent.at(-1)).toMatchObject({
      id: '2',
      error: {
        message: 'Template service request failed',
        data: { code: 'TEMPLATE_PSD_PARSE_FAILED' }
      }
    });
    expect(JSON.stringify(port.sent.at(-1))).not.toContain('/home/user/private');
  });

  it('does not expose paths disguised as template service error codes', async () => {
    const port = new FakePort();
    createPluginHostSession({
      app: template,
      port,
      theme: () => 'dark',
      notify: vi.fn(),
      saveFile: vi.fn(),
      requestNativeService: vi.fn(async () => {
        throw { code: '/home/user/private.psd', message: 'Import failed' };
      })
    });
    const templateRequest = (id: string, method: string, params: unknown = {}): HostRpcMessage => ({
      jsonrpc: '2.0', id, apiVersion: '2.0', appId: template.appId, method, params
    });

    port.receive(templateRequest('1', 'host.handshake'));
    await flush();
    port.receive(templateRequest('2', 'host.template.project', {
      method: 'template.templates.list', params: {}
    }));
    await flush();

    expect(port.sent.at(-1)).toMatchObject({
      id: '2', error: { data: { code: 'NATIVE_SERVICE_REQUEST_FAILED' } }
    });
    expect(JSON.stringify(port.sent.at(-1))).not.toContain('/home/user');
  });

  it('rejects template picker results outside the declared file kind', async () => {
    const port = new FakePort();
    createPluginHostSession({
      app: template,
      port,
      theme: () => 'dark',
      notify: vi.fn(),
      saveFile: vi.fn(),
      pickTemplateFiles: vi.fn(async () => [
        { path: '/home/user/private.txt', name: 'private.txt' }
      ]),
      requestNativeService: vi.fn()
    });
    const templateRequest = (id: string, method: string, params: unknown = {}): HostRpcMessage => ({
      jsonrpc: '2.0', id, apiVersion: '2.0', appId: template.appId, method, params
    });

    port.receive(templateRequest('1', 'host.handshake'));
    await flush();
    port.receive(templateRequest('2', 'host.template.pickFiles', {
      kind: 'image', multiple: false
    }));
    await flush();

    expect(port.sent.at(-1)).toMatchObject({
      id: '2', error: { data: { code: 'HOST_TEMPLATE_FILE_TYPE_INVALID' } }
    });
    expect(JSON.stringify(port.sent.at(-1))).not.toContain('/home/user');
  });

  it('cancels template selections and rejects unscoped or unauthorized requests', async () => {
    const port = new FakePort();
    const requestService = vi.fn();
    createPluginHostSession({
      app: template,
      port,
      theme: () => 'dark',
      notify: vi.fn(),
      saveFile: vi.fn(),
      pickTemplateFiles: vi.fn(async () => null),
      pickTemplateDirectory: vi.fn(async () => null),
      requestNativeService: requestService
    });
    const templateRequest = (id: string, method: string, params: unknown = {}): HostRpcMessage => ({
      jsonrpc: '2.0', id, apiVersion: '2.0', appId: template.appId, method, params
    });

    port.receive(templateRequest('1', 'host.handshake'));
    await flush();
    port.receive(templateRequest('2', 'host.template.pickFiles', {
      kind: 'image', multiple: true
    }));
    await flush();
    expect(port.sent.at(-1)).toMatchObject({
      id: '2', result: { cancelled: true, files: [] }
    });
    port.receive(templateRequest('3', 'host.template.pickDirectory'));
    await flush();
    expect(port.sent.at(-1)).toMatchObject({ id: '3', result: { cancelled: true } });

    port.receive(templateRequest('4', 'host.template.project', {
      method: 'template.assets.import', params: { fileToken: '/etc/passwd' }
    }));
    await flush();
    expect(port.sent.at(-1)).toMatchObject({
      id: '4', error: { data: { code: 'HOST_TEMPLATE_TOKEN_REQUIRED' } }
    });
    port.receive(templateRequest('5', 'host.template.project', {
      method: 'template.process.spawn', params: {}
    }));
    await flush();
    expect(port.sent.at(-1)).toMatchObject({
      id: '5', error: { data: { code: 'HOST_TEMPLATE_METHOD_DENIED' } }
    });
    expect(requestService).not.toHaveBeenCalled();

    const deniedPort = new FakePort();
    createPluginHostSession({
      app: { ...template, permissions: [] },
      port: deniedPort,
      theme: () => 'dark',
      notify: vi.fn(),
      saveFile: vi.fn(),
      pickTemplateFiles: vi.fn(async () => null),
      requestNativeService: requestService
    });
    deniedPort.receive({
      jsonrpc: '2.0', id: '1', apiVersion: '2.0', appId: template.appId,
      method: 'host.handshake', params: {}
    });
    await flush();
    deniedPort.receive({
      jsonrpc: '2.0', id: '2', apiVersion: '2.0', appId: template.appId,
      method: 'host.template.pickFiles', params: { kind: 'psd', multiple: false }
    });
    await flush();
    expect(deniedPort.sent.at(-1)).toMatchObject({
      id: '2', error: { data: { code: 'HOST_PERMISSION_DENIED' } }
    });
  });

  it('routes only declared download task methods and pushes task events', async () => {
    const port = new FakePort();
    const requestService = vi.fn(async (_method: string, _params: unknown) => ({ id: 'dl-1' }));
    const pickDirectory = vi.fn(async () => '/home/user/Downloads');
    const session = createPluginHostSession({
      app: aria2,
      port,
      theme: () => 'dark',
      notify: vi.fn(),
      saveFile: vi.fn(),
      pickDirectory,
      requestNativeService: requestService
    });
    const downloadRequest = (id: string, method: string, params: unknown = {}): HostRpcMessage => ({
      jsonrpc: '2.0', id, apiVersion: '2.0', appId: aria2.appId, method, params
    });

    port.receive(downloadRequest('1', 'host.handshake'));
    await flush();
    port.receive(downloadRequest('2', 'host.download.pickDirectory', {
      defaultPath: '/home/user'
    }));
    await flush();
    expect(pickDirectory).toHaveBeenCalledWith('/home/user');
    expect(port.sent[1]).toMatchObject({
      id: '2', result: { cancelled: false, path: '/home/user/Downloads' }
    });

    port.receive(downloadRequest('3', 'host.download.tasks', {
      method: 'download.tasks.start',
      params: { request: { url: 'https://example.com/file.zip' } }
    }));
    await flush();
    expect(requestService).toHaveBeenCalledWith('download.tasks.start', {
      request: { url: 'https://example.com/file.zip' }
    });

    port.receive(downloadRequest('4', 'host.download.tasks', {
      method: 'download.process.spawn', params: {}
    }));
    await flush();
    expect(port.sent.at(-1)).toMatchObject({
      id: '4', error: { data: { code: 'HOST_DOWNLOAD_METHOD_DENIED' } }
    });
    expect(requestService).toHaveBeenCalledTimes(1);

    session.notifyNativeServiceEvent({ id: 'dl-1', status: 'running', progress: 25 });
    expect(port.sent.at(-1)).toMatchObject({
      apiVersion: '2.0',
      method: 'host.download.taskEvent',
      params: { id: 'dl-1', status: 'running', progress: 25 }
    });
    session.terminate();
  });

  it('routes native service events by payload when a plugin declares multiple domains', async () => {
    const port = new FakePort();
    const session = createPluginHostSession({
      app: {
        ...aria2,
        permissions: ['host.http.stream', 'host.download.tasks']
      },
      port,
      theme: () => 'dark',
      notify: vi.fn(),
      saveFile: vi.fn(),
      requestNativeService: vi.fn()
    });
    const hybridRequest = (id: string, method: string): HostRpcMessage => ({
      jsonrpc: '2.0', id, apiVersion: '2.0', appId: aria2.appId, method, params: {}
    });

    port.receive(hybridRequest('1', 'host.handshake'));
    await flush();
    session.notifyNativeServiceEvent({
      sessionId: 'sse-1', generation: 1, message: { kind: 'open' }
    });
    expect(port.sent.at(-1)).toMatchObject({ method: 'host.http.streamEvent' });

    session.notifyNativeServiceEvent({ id: 'dl-1', status: 'running', progress: 25 });
    expect(port.sent.at(-1)).toMatchObject({ method: 'host.download.taskEvent' });

    const sentCount = port.sent.length;
    session.notifyNativeServiceEvent({});
    expect(port.sent).toHaveLength(sentCount);
    session.terminate();
  });

  it('routes HTTP Host API 2.0 methods by declared capability and pushes service events', async () => {
    const port = new FakePort();
    const requestService = vi.fn(async (_method: string, _params: unknown) => ({ ok: true }));
    const session = createPluginHostSession({
      app: http,
      port,
      theme: () => 'dark',
      notify: vi.fn(),
      saveFile: vi.fn(),
      requestNativeService: requestService
    });
    const httpRequest = (id: string, method: string, params: unknown = {}): HostRpcMessage => ({
      jsonrpc: '2.0',
      id,
      apiVersion: '2.0',
      appId: http.appId,
      method,
      params
    });

    port.receive(httpRequest('1', 'host.handshake'));
    await flush();
    port.receive(httpRequest('2', 'host.http.request', {
      method: 'http.send',
      params: { request: { url: 'https://example.com' } }
    }));
    await flush();

    expect(requestService).toHaveBeenCalledWith('http.send', {
      request: { url: 'https://example.com' }
    });
    expect(port.sent[1]).toMatchObject({ id: '2', result: { ok: true } });

    port.receive(httpRequest('3', 'host.http.request', {
      method: 'data.tabs.list',
      params: {}
    }));
    await flush();
    expect(port.sent[2]).toMatchObject({
      id: '3',
      error: { data: { code: 'HOST_HTTP_METHOD_DENIED' } }
    });

    session.notifyNativeServiceEvent({ sessionId: 'sse-1', generation: 1, message: { kind: 'open' } });
    expect(port.sent.at(-1)).toMatchObject({
      apiVersion: '2.0',
      method: 'host.http.streamEvent',
      params: { sessionId: 'sse-1', generation: 1 }
    });
    session.terminate();
  });

  it('exposes multipart files as opaque tokens and rejects unscoped paths', async () => {
    const port = new FakePort();
    const requestService = vi.fn(async (_method: string, _params: unknown) => ({ ok: true }));
    const copyText = vi.fn(async () => {});
    createPluginHostSession({
      app: http,
      port,
      theme: () => 'dark',
      notify: vi.fn(),
      saveFile: vi.fn(),
      pickFile: vi.fn(async () => ({ path: '/home/user/private.txt', name: 'private.txt' })),
      copyText,
      requestNativeService: requestService
    });
    const httpRequest = (id: string, method: string, params: unknown = {}): HostRpcMessage => ({
      jsonrpc: '2.0',
      id,
      apiVersion: '2.0',
      appId: http.appId,
      method,
      params
    });

    port.receive(httpRequest('1', 'host.handshake'));
    await flush();
    port.receive(httpRequest('2', 'host.http.pickFile'));
    await flush();
    const selection = (port.sent[1] as { result: { token: string; name: string } }).result;
    expect(selection.name).toBe('private.txt');
    expect(selection.token).not.toContain('/home/user');

    port.receive(httpRequest('copy', 'host.http.copyText', { text: 'curl example.com' }));
    await flush();
    expect(copyText).toHaveBeenCalledWith('curl example.com');

    port.receive(httpRequest('3', 'host.http.request', {
      method: 'http.send',
      params: {
        request: {
          bodyType: 'multipart',
          multipartFields: [{ key: 'file', kind: 'file', value: selection.token, enabled: true }]
        }
      }
    }));
    await flush();
    expect(requestService).toHaveBeenLastCalledWith('http.send', {
      request: {
        bodyType: 'multipart',
        multipartFields: [{ key: 'file', kind: 'file', value: '/home/user/private.txt', enabled: true }]
      }
    });

    port.receive(httpRequest('4', 'host.http.request', {
      method: 'http.send',
      params: {
        request: {
          bodyType: 'multipart',
          multipartFields: [{ key: 'file', kind: 'file', value: '/etc/passwd', enabled: true }]
        }
      }
    }));
    await flush();
    expect(port.sent.at(-1)).toMatchObject({
      id: '4',
      error: { data: { code: 'HOST_HTTP_FILE_TOKEN_REQUIRED' } }
    });
    expect(requestService).toHaveBeenCalledTimes(1);
  });

  it('delivers a legal five MiB stream snapshot over Host API 2', async () => {
    const port = new FakePort();
    const session = createPluginHostSession({
      app: http,
      port,
      theme: () => 'dark',
      notify: vi.fn(),
      saveFile: vi.fn(),
      requestNativeService: vi.fn(async () => ({ body: 'x'.repeat(5 * 1024 * 1024) }))
    });
    const httpRequest = (id: string, method: string, params: unknown = {}): HostRpcMessage => ({
      jsonrpc: '2.0', id, apiVersion: '2.0', appId: http.appId, method, params
    });

    port.receive(httpRequest('1', 'host.handshake'));
    await flush();
    port.receive(httpRequest('2', 'host.http.stream', {
      method: 'stream.snapshot', params: { sessionId: 'stream-1' }
    }));
    await flush();

    expect(port.sent.at(-1)).toMatchObject({ id: '2', result: { body: expect.any(String) } });
    session.terminate();
  });

  it('requires a handshake and exposes only declared capabilities', async () => {
    const port = new FakePort();
    const session = createPluginHostSession({
      app: codec,
      port,
      sessionId: 'session-1',
      correlationId: 'correlation-1',
      theme: () => 'dark',
      notify: async () => {},
      saveFile: async () => ({ cancelled: false })
    });

    port.receive(request('1', 'host.theme.get'));
    await flush();
    expect(port.sent[0]).toMatchObject({
      id: '1',
      error: { data: { code: 'HOST_HANDSHAKE_REQUIRED' } }
    });

    port.receive(request('2', 'host.handshake'));
    await flush();
    expect(port.sent[1]).toMatchObject({
      id: '2',
      result: {
        apiVersion: '1.0',
        sessionId: 'session-1',
        correlationId: 'correlation-1',
        capabilities: ['host.theme.read'],
        metadata: { appId: codec.appId, name: codec.name, version: codec.version },
        theme: 'dark'
      }
    });

    port.receive(request('3', 'host.file.save', { suggestedName: 'codec.txt', content: 'x' }));
    await flush();
    expect(port.sent[2]).toMatchObject({
      id: '3',
      error: { data: { code: 'HOST_PERMISSION_DENIED' } }
    });
    session.terminate();
  });

  it('reports a completed handshake only after posting the successful response', async () => {
    const port = new FakePort();
    const sequence: string[] = [];
    port.onPost = () => sequence.push('response');
    createPluginHostSession({
      app: codec,
      port,
      theme: () => 'dark',
      notify: vi.fn(),
      saveFile: vi.fn(),
      onHandshake: () => sequence.push('callback')
    });

    port.receive(request('1', 'host.handshake'));
    await vi.waitFor(() => expect(sequence).toHaveLength(2));

    expect(sequence).toEqual(['response', 'callback']);
  });

  it('pushes theme changes after the handshake', async () => {
    const port = new FakePort();
    const session = createPluginHostSession({
      app: codec,
      port,
      theme: () => 'light',
      notify: async () => {},
      saveFile: async () => ({ cancelled: false })
    });
    port.receive(request('1', 'host.handshake'));
    await flush();

    session.notifyTheme('dark');

    expect(port.sent.at(-1)).toEqual({
      jsonrpc: '2.0',
      apiVersion: '1.0',
      appId: codec.appId,
      method: 'host.theme.changed',
      params: { theme: 'dark' }
    });
    session.terminate();
  });
});
