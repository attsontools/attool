import type { InstalledPlugin } from './types';

type ApiVersion = '1.0' | '2.0';
const LEGACY_MAX_MESSAGE_BYTES = 1024 * 1024;
const HTTP_MAX_MESSAGE_BYTES = 8 * 1024 * 1024;
const AUXILIARY_PAYLOAD_MAX_BYTES = 64 * 1024;
const CLIPBOARD_PREVIEW_MAX_BYTES = 2 * 1024 * 1024;
const CLIPBOARD_LIST_MAX_ITEMS = 100;
const DOWNLOAD_TASK_METHODS = new Set([
  'download.defaultDirectory',
  'download.tasks.list',
  'download.tasks.start',
  'download.tasks.cancel',
  'download.tasks.openFolder',
  'download.tasks.clearCompleted',
  'download.tasks.delete'
]);
const TEMPLATE_PROJECT_METHODS = new Set([
  'template.templates.list',
  'template.templates.load',
  'template.templates.save',
  'template.templates.rename',
  'template.templates.delete',
  'template.psd.import',
  'template.assets.import',
  'template.assets.save',
  'template.assets.list',
  'template.assets.delete',
  'template.batch.sources.register',
  'template.batch.run',
  'template.batch.save'
]);
const CLIPBOARD_HISTORY_METHODS = new Set([
  'clipboard.history.list',
  'clipboard.history.get',
  'clipboard.history.pin',
  'clipboard.history.delete',
  'clipboard.history.clearUnpinned',
  'clipboard.history.restore',
  'clipboard.settings.get',
  'clipboard.settings.update'
]);

type RpcBase = {
  jsonrpc: '2.0';
  apiVersion: ApiVersion;
  appId: string;
};

type RpcRequest = RpcBase & {
  id: string;
  method: string;
  params: unknown;
};

type RpcNotification = RpcBase & {
  method: string;
  params: unknown;
};

type RpcSuccess = RpcBase & {
  id: string;
  result: unknown;
};

type RpcFailure = RpcBase & {
  id: string;
  error: {
    code: number;
    message: string;
    data: { code: string };
  };
};

export type HostRpcMessage = RpcRequest | RpcNotification | RpcSuccess | RpcFailure;

export interface HostMessagePort {
  onmessage: ((event: MessageEvent<HostRpcMessage>) => void) | null;
  postMessage(message: HostRpcMessage): void;
  start(): void;
  close(): void;
}

export interface HostFileSaveRequest {
  suggestedName: string;
  content: string;
  encoding: 'utf8' | 'base64';
}

export interface HostTemplateFilePickRequest {
  kind: 'psd' | 'image';
  multiple: boolean;
}

export interface HostSelectedPath {
  path: string;
  name: string;
}

export interface HostUiOpenRequest {
  windowId: string;
  payload: unknown;
}

export interface PluginHostSession {
  readonly sessionId: string;
  readonly correlationId: string;
  notifyTheme(theme: 'dark' | 'light'): void;
  notifyNativeServiceEvent(event: unknown): void;
  notifyActivated(payload: unknown): void;
  terminate(): void;
}

type SessionOptions = {
  app: InstalledPlugin;
  port: HostMessagePort;
  windowId?: string;
  sessionId?: string;
  correlationId?: string;
  theme(): 'dark' | 'light';
  notify(input: { title?: string; message: string; kind: 'info' | 'success' | 'warning' | 'error' }): Promise<void>;
  saveFile(input: HostFileSaveRequest): Promise<{ cancelled: boolean; path?: string }>;
  pickFile?(): Promise<{ path: string; name: string } | null>;
  pickDirectory?(defaultPath?: string): Promise<string | null>;
  pickTemplateFiles?(input: HostTemplateFilePickRequest): Promise<HostSelectedPath[] | null>;
  pickTemplateDirectory?(): Promise<HostSelectedPath | null>;
  copyText?(text: string): Promise<void>;
  requestNativeService?(method: string, params: unknown): Promise<unknown>;
  openAuxiliaryWindow?(input: HostUiOpenRequest): Promise<unknown>;
  closeAuxiliaryWindow?(): Promise<void>;
  onHandshake?(): void;
  maxMessageBytes?: number;
};

class RpcDispatchError extends Error {
  constructor(
    readonly stableCode: string,
    message: string,
    readonly rpcCode = -32000
  ) {
    super(message);
  }
}

export function createPluginHostSession(options: SessionOptions): PluginHostSession {
  const apiVersion: ApiVersion = options.app.nativeService ? '2.0' : '1.0';
  const maxMessageBytes = options.maxMessageBytes
    ?? (apiVersion === '2.0' ? HTTP_MAX_MESSAGE_BYTES : LEGACY_MAX_MESSAGE_BYTES);
  const sessionId = options.sessionId ?? crypto.randomUUID();
  const correlationId = options.correlationId ?? crypto.randomUUID();
  const cancelled = new Set<string>();
  const selectedFiles = new Map<string, string>();
  const templateSelections = new Map<
    string,
    HostSelectedPath & { kind: HostTemplateFilePickRequest['kind'] | 'directory' }
  >();
  const consumedTemplateTokens = new Set<string>();
  let handshaken = false;
  let terminated = false;

  function post(message: HostRpcMessage) {
    if (terminated) return;
    const bytes = messageBytes(message);
    if (bytes > maxMessageBytes) return;
    options.port.postMessage(message);
  }

  function success(id: string, result: unknown) {
    const message: RpcSuccess = {
      jsonrpc: '2.0',
      id,
      apiVersion,
      appId: options.app.appId,
      result
    };
    if (messageBytes(message) > maxMessageBytes) {
      failure(id, new RpcDispatchError('HOST_MESSAGE_TOO_LARGE', 'Host response is too large'));
      return;
    }
    post(message);
  }

  function failure(id: string, error: RpcDispatchError) {
    post({
      jsonrpc: '2.0',
      id,
      apiVersion,
      appId: options.app.appId,
      error: {
        code: error.rpcCode,
        message: error.message,
        data: { code: error.stableCode }
      }
    });
  }

  function requirePermission(permission: string) {
    if (!options.app.permissions.includes(permission)) {
      throw new RpcDispatchError(
        'HOST_PERMISSION_DENIED',
        `Permission is not granted: ${permission}`,
        -32003
      );
    }
  }

  async function dispatch(request: RpcRequest): Promise<unknown> {
    if (request.method === 'host.handshake') {
      handshaken = true;
      return {
        apiVersion,
        sessionId,
        correlationId,
        capabilities: [...options.app.permissions],
        metadata: {
          appId: options.app.appId,
          name: options.app.name,
          version: options.app.version
        },
        theme: options.theme()
      };
    }
    if (!handshaken) {
      throw new RpcDispatchError(
        'HOST_HANDSHAKE_REQUIRED',
        'Host handshake is required',
        -32001
      );
    }
    switch (request.method) {
      case 'host.theme.get':
        requirePermission('host.theme.read');
        return { theme: options.theme() };
      case 'host.app.getMetadata':
        return {
          appId: options.app.appId,
          name: options.app.name,
          version: options.app.version,
          publisherId: options.app.publisherId
        };
      case 'host.notification.show': {
        requirePermission('host.notification.show');
        const params = asRecord(request.params);
        const message = requiredString(params.message, 'message', 2000);
        const title = optionalString(params.title, 'title', 200);
        const kind = params.kind ?? 'info';
        if (!['info', 'success', 'warning', 'error'].includes(String(kind))) {
          throw invalidParams('kind');
        }
        await options.notify({
          title,
          message,
          kind: kind as 'info' | 'success' | 'warning' | 'error'
        });
        return { shown: true };
      }
      case 'host.file.save': {
        requirePermission('host.file.save');
        const params = asRecord(request.params);
        const suggestedName = requiredString(params.suggestedName, 'suggestedName', 255);
        if (suggestedName.includes('/') || suggestedName.includes('\\')) {
          throw invalidParams('suggestedName');
        }
        const content = requiredString(params.content, 'content', maxMessageBytes);
        const encoding = params.encoding ?? 'utf8';
        if (encoding !== 'utf8' && encoding !== 'base64') throw invalidParams('encoding');
        return options.saveFile({ suggestedName, content, encoding });
      }
      case 'host.http.pickFile': {
        requirePermission('host.http.request');
        if (!options.pickFile) {
          throw new RpcDispatchError('HOST_FILE_PICK_UNAVAILABLE', 'File selection is unavailable');
        }
        const selected = await options.pickFile();
        if (!selected) return { cancelled: true };
        const token = `host-file:${crypto.randomUUID()}`;
        selectedFiles.set(token, selected.path);
        return { cancelled: false, token, name: selected.name };
      }
      case 'host.http.copyText': {
        requirePermission('host.http.request');
        if (!options.copyText) {
          throw new RpcDispatchError('HOST_CLIPBOARD_WRITE_UNAVAILABLE', 'Clipboard write is unavailable');
        }
        const params = asRecord(request.params);
        const text = requiredString(params.text, 'text', maxMessageBytes);
        await options.copyText(text);
        return { copied: true };
      }
      case 'host.download.pickDirectory': {
        requirePermission('host.download.tasks');
        if (!options.pickDirectory) {
          throw new RpcDispatchError(
            'HOST_DIRECTORY_PICK_UNAVAILABLE',
            'Directory selection is unavailable'
          );
        }
        const params = asRecord(request.params);
        const defaultPath = optionalString(params.defaultPath, 'defaultPath', 4096);
        const path = await options.pickDirectory(defaultPath);
        return path ? { cancelled: false, path } : { cancelled: true };
      }
      case 'host.http.request':
        return dispatchHttpRequest(request, 'host.http.request', (method) =>
          method === 'http.send' || method === 'http.cancel'
        );
      case 'host.http.stream':
        return dispatchHttpRequest(request, 'host.http.stream', (method) =>
          method === 'stream.open'
          || method === 'stream.close'
          || method === 'stream.send'
          || method === 'stream.snapshot'
        );
      case 'host.http.data':
        return dispatchHttpRequest(request, 'host.http.data', (method) => method.startsWith('data.'));
      case 'host.download.tasks':
        return dispatchDownloadRequest(request);
      case 'host.ui.open': {
        requirePermission('host.ui.auxiliary');
        const params = asRecord(request.params);
        const windowId = requiredString(params.windowId, 'windowId', 64);
        if (!Object.prototype.hasOwnProperty.call(options.app.windows, windowId)) {
          throw new RpcDispatchError(
            'HOST_UI_WINDOW_NOT_DECLARED',
            'Auxiliary window is not declared by this application',
            -32003
          );
        }
        const payload = Object.prototype.hasOwnProperty.call(params, 'payload')
          ? params.payload
          : {};
        if (messageBytes(payload) > AUXILIARY_PAYLOAD_MAX_BYTES) {
          throw new RpcDispatchError(
            'HOST_UI_PAYLOAD_TOO_LARGE',
            'Auxiliary window payload is too large',
            -32002
          );
        }
        if (!options.openAuxiliaryWindow) {
          throw new RpcDispatchError('HOST_UI_UNAVAILABLE', 'Auxiliary windows are unavailable');
        }
        return options.openAuxiliaryWindow({ windowId, payload });
      }
      case 'host.ui.close': {
        requirePermission('host.ui.auxiliary');
        if (!options.windowId || !options.closeAuxiliaryWindow) {
          throw new RpcDispatchError(
            'HOST_UI_UNAVAILABLE',
            'The current Host session is not an auxiliary window'
          );
        }
        await options.closeAuxiliaryWindow();
        return { closed: true };
      }
      case 'host.clipboard.history':
        return dispatchClipboardRequest(request);
      case 'host.template.pickFiles': {
        requirePermission('host.template.project');
        if (!options.pickTemplateFiles) {
          throw new RpcDispatchError(
            'HOST_TEMPLATE_FILE_PICK_UNAVAILABLE',
            'Template file selection is unavailable'
          );
        }
        const params = asRecord(request.params);
        const kind = params.kind;
        if (kind !== 'psd' && kind !== 'image') throw invalidParams('kind');
        const multiple = requiredBoolean(params.multiple, 'multiple');
        if (kind === 'psd' && multiple) throw invalidParams('multiple');
        const selected = await options.pickTemplateFiles({ kind, multiple });
        if (!selected?.length) return { cancelled: true, files: [] };
        if (
          (kind === 'psd' && selected.length !== 1)
          || selected.some((file) => !templateFileMatchesKind(file.path, kind))
        ) {
          throw new RpcDispatchError(
            'HOST_TEMPLATE_FILE_TYPE_INVALID',
            'Selected template file type is not allowed',
            -32003
          );
        }
        return {
          cancelled: false,
          files: selected.map((file) => {
            const token = `host-template-file:${crypto.randomUUID()}`;
            templateSelections.set(token, { ...file, kind });
            return { token, name: file.name };
          })
        };
      }
      case 'host.template.pickDirectory': {
        requirePermission('host.template.project');
        if (!options.pickTemplateDirectory) {
          throw new RpcDispatchError(
            'HOST_TEMPLATE_DIRECTORY_PICK_UNAVAILABLE',
            'Template directory selection is unavailable'
          );
        }
        asRecord(request.params);
        const selected = await options.pickTemplateDirectory();
        if (!selected) return { cancelled: true };
        const token = `host-template-directory:${crypto.randomUUID()}`;
        templateSelections.set(token, { ...selected, kind: 'directory' });
        return { cancelled: false, token, name: selected.name };
      }
      case 'host.template.project':
        return dispatchTemplateRequest(request);
      default:
        throw new RpcDispatchError('HOST_METHOD_NOT_FOUND', 'Host API method was not found', -32601);
    }
  }

  async function dispatchHttpRequest(
    request: RpcRequest,
    permission: string,
    allows: (method: string) => boolean
  ) {
    requirePermission(permission);
    if (!options.requestNativeService) {
      throw new RpcDispatchError('NATIVE_SERVICE_NOT_CONNECTED', 'Native service is unavailable');
    }
    const params = asRecord(request.params);
    const method = requiredString(params.method, 'method', 128);
    if (!allows(method)) {
      throw new RpcDispatchError(
        'HOST_HTTP_METHOD_DENIED',
        `HTTP service method is not allowed for ${permission}`,
        -32003
      );
    }
    if (!Object.prototype.hasOwnProperty.call(params, 'params')) throw invalidParams('params');
    try {
      return await options.requestNativeService(
        method,
        method === 'http.send' ? resolveSelectedFiles(params.params, selectedFiles) : params.params
      );
    } catch (reason) {
      throw serviceError(reason);
    }
  }

  async function dispatchDownloadRequest(request: RpcRequest) {
    requirePermission('host.download.tasks');
    if (!options.requestNativeService) {
      throw new RpcDispatchError('NATIVE_SERVICE_NOT_CONNECTED', 'Native service is unavailable');
    }
    const params = asRecord(request.params);
    const method = requiredString(params.method, 'method', 128);
    if (!DOWNLOAD_TASK_METHODS.has(method)) {
      throw new RpcDispatchError(
        'HOST_DOWNLOAD_METHOD_DENIED',
        'Download service method is not allowed for host.download.tasks',
        -32003
      );
    }
    if (!Object.prototype.hasOwnProperty.call(params, 'params')) throw invalidParams('params');
    try {
      return await options.requestNativeService(method, params.params);
    } catch (reason) {
      throw serviceError(reason);
    }
  }

  async function dispatchClipboardRequest(request: RpcRequest) {
    requirePermission('host.clipboard.history');
    if (!options.requestNativeService) {
      throw new RpcDispatchError('NATIVE_SERVICE_NOT_CONNECTED', 'Native service is unavailable');
    }
    const params = asRecord(request.params);
    const method = requiredString(params.method, 'method', 128);
    if (!CLIPBOARD_HISTORY_METHODS.has(method)) {
      throw new RpcDispatchError(
        'HOST_CLIPBOARD_METHOD_DENIED',
        'Clipboard service method is not allowed for host.clipboard.history',
        -32003
      );
    }
    if (!Object.prototype.hasOwnProperty.call(params, 'params')) throw invalidParams('params');
    try {
      const serviceParams = validateClipboardParams(method, params.params);
      const result = await options.requestNativeService(method, serviceParams);
      assertClipboardResult(method, result);
      return result;
    } catch (reason) {
      throw reason instanceof RpcDispatchError ? reason : serviceError(reason);
    }
  }

  async function dispatchTemplateRequest(request: RpcRequest) {
    requirePermission('host.template.project');
    if (!options.requestNativeService) {
      throw new RpcDispatchError('NATIVE_SERVICE_NOT_CONNECTED', 'Native service is unavailable');
    }
    const params = asRecord(request.params);
    const method = requiredString(params.method, 'method', 128);
    if (!TEMPLATE_PROJECT_METHODS.has(method)) {
      throw new RpcDispatchError(
        'HOST_TEMPLATE_METHOD_DENIED',
        'Template service method is not allowed for host.template.project',
        -32003
      );
    }
    if (!Object.prototype.hasOwnProperty.call(params, 'params')) throw invalidParams('params');
    try {
      const serviceParams = resolveTemplateSelections(
        method,
        params.params,
        templateSelections,
        consumedTemplateTokens
      );
      const result = await options.requestNativeService(method, serviceParams);
      if (method.startsWith('template.batch.')) {
        assertTemplateResultHasNoPaths(result);
      }
      return result;
    } catch (reason) {
      throw reason instanceof RpcDispatchError ? reason : templateServiceError(reason);
    }
  }

  function notifyNativeServiceEvent(event: unknown) {
    if (!handshaken || terminated || apiVersion !== '2.0') return;
    const method = nativeServiceEventMethod(event, options.app.permissions);
    if (!method) return;
    const params = method === 'host.clipboard.historyChanged'
      ? {
          type: 'clipboard.history.changed',
          revision: (event as Record<string, unknown>).revision
        }
      : event;
    post({
      jsonrpc: '2.0',
      apiVersion,
      appId: options.app.appId,
      method,
      params
    });
  }

  options.port.onmessage = (event) => {
    if (terminated) return;
    const message = event.data;
    if (messageBytes(message) > maxMessageBytes) {
      if (isRequest(message)) {
        failure(
          message.id,
          new RpcDispatchError('HOST_MESSAGE_TOO_LARGE', 'Host API message is too large', -32002)
        );
      }
      return;
    }
    if (
      !isRequestLike(message) ||
      message.jsonrpc !== '2.0' ||
      message.apiVersion !== apiVersion ||
      message.appId !== options.app.appId
    ) {
      if (isRequest(message)) {
        failure(
          message.id,
          new RpcDispatchError('HOST_REQUEST_INVALID', 'Host API request is invalid', -32600)
        );
      }
      return;
    }
    if (!('id' in message)) {
      if (message.method === '$/cancelRequest') {
        const id = asRecord(message.params).id;
        if (typeof id === 'string') cancelled.add(id);
      }
      return;
    }
    void dispatch(message)
      .then((result) => {
        if (cancelled.delete(message.id) || terminated) return;
        success(message.id, result);
        if (message.method === 'host.handshake') options.onHandshake?.();
      })
      .catch((reason) => {
        if (cancelled.delete(message.id) || terminated) return;
        failure(
          message.id,
          reason instanceof RpcDispatchError
            ? reason
            : new RpcDispatchError('HOST_INTERNAL_ERROR', 'Host API request failed')
        );
      });
  };
  options.port.start();

  return {
    sessionId,
    correlationId,
    notifyTheme(theme) {
      if (!handshaken || terminated || !options.app.permissions.includes('host.theme.read')) return;
      post({
        jsonrpc: '2.0',
        apiVersion,
        appId: options.app.appId,
        method: 'host.theme.changed',
        params: { theme }
      });
    },
    notifyNativeServiceEvent,
    notifyActivated(payload) {
      if (!handshaken || terminated || !options.windowId) return;
      if (messageBytes(payload) > AUXILIARY_PAYLOAD_MAX_BYTES) return;
      post({
        jsonrpc: '2.0',
        apiVersion,
        appId: options.app.appId,
        method: 'host.ui.activated',
        params: { windowId: options.windowId, payload }
      });
    },
    terminate() {
      if (terminated) return;
      terminated = true;
      cancelled.clear();
      selectedFiles.clear();
      templateSelections.clear();
      consumedTemplateTokens.clear();
      options.port.onmessage = null;
      options.port.close();
    }
  };
}

function validateClipboardParams(method: string, value: unknown): unknown {
  if (method !== 'clipboard.history.list') return value;
  const params = asRecord(value);
  if (
    params.limit !== undefined
    && (!Number.isSafeInteger(params.limit)
      || (params.limit as number) < 1
      || (params.limit as number) > CLIPBOARD_LIST_MAX_ITEMS)
  ) {
    throw invalidParams('params.limit');
  }
  return params;
}

function assertClipboardResult(method: string, value: unknown) {
  if (method === 'clipboard.history.list') {
    const items = Array.isArray(value)
      ? value
      : (typeof value === 'object' && value !== null && Array.isArray((value as { items?: unknown }).items)
          ? (value as { items: unknown[] }).items
          : null);
    if (!items || items.length > CLIPBOARD_LIST_MAX_ITEMS) {
      throw invalidClipboardResult();
    }
  }

  const pending = [value];
  while (pending.length) {
    const current = pending.pop();
    if (Array.isArray(current)) {
      pending.push(...current);
      continue;
    }
    if (typeof current !== 'object' || current === null) continue;
    for (const [key, nested] of Object.entries(current)) {
      const normalized = key.toLowerCase();
      if (
        normalized === 'assetpath'
        || normalized === 'asseturl'
        || normalized === 'databasepath'
        || normalized === 'dbpath'
        || normalized === 'cachepath'
      ) {
        throw invalidClipboardResult();
      }
      if (
        normalized === 'previewdataurl'
        && nested !== null
        && (typeof nested !== 'string'
          || !/^data:image\/(?:png|jpeg|webp);base64,[A-Za-z0-9+/]*={0,2}$/.test(nested)
          || new TextEncoder().encode(nested).byteLength > CLIPBOARD_PREVIEW_MAX_BYTES)
      ) {
        throw invalidClipboardResult();
      }
      pending.push(nested);
    }
  }
}

function invalidClipboardResult() {
  return new RpcDispatchError(
    'HOST_CLIPBOARD_RESPONSE_INVALID',
    'Clipboard service response violates the public data contract',
    -32003
  );
}

function assertTemplateResultHasNoPaths(value: unknown) {
  const pending = [value];
  while (pending.length) {
    const current = pending.pop();
    if (Array.isArray(current)) {
      pending.push(...current);
      continue;
    }
    if (typeof current === 'string') {
      if (containsAbsolutePath(current)) {
        throw new RpcDispatchError(
          'HOST_TEMPLATE_PATH_EXPOSED',
          'Template service responses must not expose local paths',
          -32003
        );
      }
      continue;
    }
    if (typeof current !== 'object' || current === null) continue;
    for (const [key, nested] of Object.entries(current)) {
      const normalized = key.toLowerCase();
      if (normalized === 'path' || normalized.endsWith('path') || normalized === 'targetdirectory') {
        throw new RpcDispatchError(
          'HOST_TEMPLATE_PATH_EXPOSED',
          'Template service responses must not expose local paths',
          -32003
        );
      }
      pending.push(nested);
    }
  }
}

function containsAbsolutePath(value: string): boolean {
  return /file:\/\//i.test(value)
    || /(?:^|[\s"'(=])\/(?!\/)/.test(value)
    || /(?:^|[\s"'(=])[A-Za-z]:[\\/]/.test(value)
    || /(?:^|[\s"'(=])\\\\[^\\]/.test(value);
}

function templateFileMatchesKind(
  path: string,
  kind: HostTemplateFilePickRequest['kind']
): boolean {
  const fileName = path.split(/[\\/]/).pop()?.toLowerCase() ?? '';
  return kind === 'psd'
    ? fileName.endsWith('.psd')
    : ['.png', '.jpg', '.jpeg', '.webp'].some((extension) => fileName.endsWith(extension));
}

function resolveTemplateSelections(
  method: string,
  value: unknown,
  selections: Map<
    string,
    HostSelectedPath & { kind: HostTemplateFilePickRequest['kind'] | 'directory' }
  >,
  consumed: Set<string>
): Record<string, unknown> {
  const params = asRecord(value);
  if (method === 'template.psd.import' || method === 'template.assets.import') {
    const token = requiredString(params.fileToken, 'params.fileToken', 256);
    const expected = method === 'template.psd.import' ? 'psd' : 'image';
    const selected = consumeTemplateSelection(token, expected, selections, consumed);
    const { fileToken: _, ...rest } = params;
    return { ...rest, sourcePath: selected.path };
  }
  if (method === 'template.batch.sources.register') {
    if (!Array.isArray(params.fileTokens) || params.fileTokens.length === 0) {
      throw invalidParams('params.fileTokens');
    }
    const tokens = params.fileTokens.map((token, index) =>
      requiredString(token, `params.fileTokens[${index}]`, 256)
    );
    if (new Set(tokens).size !== tokens.length) throw invalidParams('params.fileTokens');
    const selected = tokens.map((token) =>
      inspectTemplateSelection(token, 'image', selections, consumed)
    );
    for (const token of tokens) {
      selections.delete(token);
      consumed.add(token);
    }
    const { fileTokens: _, ...rest } = params;
    return { ...rest, sourcePaths: selected.map((file) => file.path) };
  }
  if (method === 'template.batch.save') {
    const token = requiredString(params.directoryToken, 'params.directoryToken', 256);
    const selected = consumeTemplateSelection(token, 'directory', selections, consumed);
    const { directoryToken: _, ...rest } = params;
    return { ...rest, targetDirectory: selected.path };
  }
  return params;
}

function inspectTemplateSelection(
  token: string,
  expected: HostTemplateFilePickRequest['kind'] | 'directory',
  selections: Map<
    string,
    HostSelectedPath & { kind: HostTemplateFilePickRequest['kind'] | 'directory' }
  >,
  consumed: Set<string>
) {
  if (consumed.has(token)) {
    throw new RpcDispatchError(
      'HOST_TEMPLATE_TOKEN_CONSUMED',
      'Template selection token has already been consumed',
      -32003
    );
  }
  const selected = selections.get(token);
  if (!selected) {
    throw new RpcDispatchError(
      'HOST_TEMPLATE_TOKEN_REQUIRED',
      'Template files and directories must be selected through the host',
      -32003
    );
  }
  if (selected.kind !== expected) {
    throw new RpcDispatchError(
      'HOST_TEMPLATE_TOKEN_TYPE_MISMATCH',
      'Template selection token has the wrong type',
      -32003
    );
  }
  return selected;
}

function consumeTemplateSelection(
  token: string,
  expected: HostTemplateFilePickRequest['kind'] | 'directory',
  selections: Map<
    string,
    HostSelectedPath & { kind: HostTemplateFilePickRequest['kind'] | 'directory' }
  >,
  consumed: Set<string>
) {
  const selected = inspectTemplateSelection(token, expected, selections, consumed);
  selections.delete(token);
  consumed.add(token);
  return selected;
}

function nativeServiceEventMethod(event: unknown, permissions: string[]): string | null {
  if (typeof event !== 'object' || event === null || Array.isArray(event)) return null;
  const payload = event as Record<string, unknown>;
  if (
    permissions.includes('host.http.stream')
    && typeof payload.sessionId === 'string'
    && Number.isSafeInteger(payload.generation)
    && Object.prototype.hasOwnProperty.call(payload, 'message')
  ) {
    return 'host.http.streamEvent';
  }
  if (
    permissions.includes('host.download.tasks')
    && typeof payload.id === 'string'
    && ['queued', 'running', 'completed', 'failed', 'cancelled'].includes(String(payload.status))
    && typeof payload.progress === 'number'
    && Number.isFinite(payload.progress)
  ) {
    return 'host.download.taskEvent';
  }
  if (
    permissions.includes('host.clipboard.history')
    && payload.type === 'clipboard.history.changed'
    && Number.isSafeInteger(payload.revision)
    && Number(payload.revision) >= 0
  ) {
    return 'host.clipboard.historyChanged';
  }
  return null;
}

function resolveSelectedFiles(value: unknown, selectedFiles: Map<string, string>): unknown {
  const params = asRecord(value);
  const request = asRecord(params.request);
  if (request.bodyType !== 'multipart') return value;
  if (!Array.isArray(request.multipartFields)) throw invalidParams('params.request.multipartFields');
  const multipartFields = request.multipartFields.map((value) => {
    const field = asRecord(value);
    if (field.kind !== 'file' || field.enabled === false) return field;
    const token = requiredString(field.value, 'params.request.multipartFields.value', 256);
    const path = selectedFiles.get(token);
    if (!path) {
      throw new RpcDispatchError(
        'HOST_HTTP_FILE_TOKEN_REQUIRED',
        'Multipart files must be selected through the host',
        -32003
      );
    }
    return { ...field, value: path };
  });
  return { ...params, request: { ...request, multipartFields } };
}

function messageBytes(value: unknown): number {
  try {
    return new TextEncoder().encode(JSON.stringify(value)).byteLength;
  } catch {
    return Number.POSITIVE_INFINITY;
  }
}

function isRequestLike(value: unknown): value is RpcRequest | RpcNotification {
  if (typeof value !== 'object' || value === null) return false;
  const request = value as Partial<RpcRequest>;
  return typeof request.method === 'string' && 'params' in request;
}

function isRequest(value: unknown): value is RpcRequest {
  return isRequestLike(value) && typeof (value as Partial<RpcRequest>).id === 'string';
}

function asRecord(value: unknown): Record<string, unknown> {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    throw invalidParams('params');
  }
  return value as Record<string, unknown>;
}

function requiredString(value: unknown, field: string, maxLength: number): string {
  if (typeof value !== 'string' || value.length === 0 || value.length > maxLength) {
    throw invalidParams(field);
  }
  return value;
}

function optionalString(value: unknown, field: string, maxLength: number): string | undefined {
  if (value === undefined) return undefined;
  if (typeof value !== 'string' || value.length > maxLength) throw invalidParams(field);
  return value;
}

function requiredBoolean(value: unknown, field: string): boolean {
  if (typeof value !== 'boolean') throw invalidParams(field);
  return value;
}

function invalidParams(field: string) {
  return new RpcDispatchError('HOST_INVALID_PARAMS', `Invalid Host API parameter: ${field}`, -32602);
}

function serviceError(reason: unknown) {
  if (reason instanceof RpcDispatchError) return reason;
  if (typeof reason === 'object' && reason !== null) {
    const error = reason as { code?: unknown; message?: unknown };
    if (typeof error.code === 'string' && typeof error.message === 'string') {
      return new RpcDispatchError(error.code, error.message);
    }
  }
  return new RpcDispatchError('NATIVE_SERVICE_REQUEST_FAILED', 'Native service request failed');
}

function templateServiceError(reason: unknown) {
  if (typeof reason === 'object' && reason !== null) {
    const error = reason as { code?: unknown };
    if (typeof error.code === 'string' && /^[A-Z][A-Z0-9_]{0,127}$/.test(error.code)) {
      return new RpcDispatchError(error.code, 'Template service request failed');
    }
  }
  return new RpcDispatchError('NATIVE_SERVICE_REQUEST_FAILED', 'Template service request failed');
}
