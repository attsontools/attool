import type {
  AcceptedAppActivation,
  AppActivationRequest
} from './appActivation';
import { AppActivationError } from './appActivation';

export type BuiltinDownloadActivationResult =
  | { ok: true; activationId: string }
  | { ok: false; code: string; message: string };

type AcceptActivation = (request: AppActivationRequest) => AcceptedAppActivation;

export function createBuiltinDownloadActivator(accept: AcceptActivation) {
  return (urls: string[]): BuiltinDownloadActivationResult => {
    const normalized = normalizeUrls(urls);
    if (!normalized) {
      return {
        ok: false,
        code: 'HOST_INVALID_PARAMS',
        message: '下载链接格式无效'
      };
    }
    try {
      const activation = accept({
        sourceAppId: 'com.attson.video',
        targetAppId: 'com.attson.aria2',
        payload: {
          version: 1,
          kind: 'download.urls',
          urls: normalized
        }
      });
      activation.commit();
      return {
        ok: true,
        activationId: activation.result.activationId
      };
    } catch (error) {
      const code = error instanceof AppActivationError
        ? error.stableCode
        : 'HOST_APP_ACTIVATION_FAILED';
      return {
        ok: false,
        code,
        message: activationErrorMessage(code)
      };
    }
  };
}

function activationErrorMessage(code: string): string {
  const messages: Record<string, string> = {
    HOST_APP_NOT_INSTALLED: 'Aria2 尚未安装，请先在应用管理中安装',
    HOST_APP_DISABLED: 'Aria2 已停用，请先在应用管理中启用',
    HOST_APP_UNAVAILABLE: 'Aria2 后台服务不可用，请稍后重试',
    HOST_APP_ACTIVATION_QUEUE_FULL: 'Aria2 正在处理过多请求，请稍后重试',
    HOST_APP_ACTIVATION_PAYLOAD_TOO_LARGE: '发送到 Aria2 的下载链接过多',
    HOST_INVALID_PARAMS: '下载链接格式无效'
  };
  return messages[code] ?? '无法打开 Aria2，请稍后重试';
}

function normalizeUrls(values: string[]): string[] | null {
  if (values.length < 1 || values.length > 100) return null;
  const urls: string[] = [];
  const seen = new Set<string>();
  for (const value of values) {
    if (!value || value !== value.trim() || value.length > 8192) return null;
    let parsed: URL;
    try {
      parsed = new URL(value);
    } catch {
      return null;
    }
    if (
      !['http:', 'https:'].includes(parsed.protocol)
      || parsed.username.length > 0
      || parsed.password.length > 0
    ) {
      return null;
    }
    if (seen.has(parsed.href)) continue;
    seen.add(parsed.href);
    urls.push(value);
  }
  return urls.length > 0 ? urls : null;
}
