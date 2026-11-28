import { describe, expect, it } from 'vitest';

import {
  AppActivationError,
  type AcceptedAppActivation,
  type AppActivationRequest
} from './appActivation';
import { createBuiltinDownloadActivator } from './builtinDownloadActivation';

describe('built-in video download activation', () => {
  it('submits one activation with the published video-to-Aria2 contract', () => {
    const requests: AppActivationRequest[] = [];
    let commits = 0;
    const accept = (request: AppActivationRequest): AcceptedAppActivation => {
      requests.push(request);
      return {
        result: { accepted: true, activationId: 'activation-1' },
        commit: () => { commits += 1; },
        discard: () => undefined
      };
    };

    const activate = createBuiltinDownloadActivator(accept);
    const result = activate([
      'https://cdn.example.com/one.jpg',
      'https://cdn.example.com/two.jpg'
    ]);

    expect(result).toEqual({ ok: true, activationId: 'activation-1' });
    expect(requests).toEqual([{
      sourceAppId: 'com.attson.video',
      targetAppId: 'com.attson.aria2',
      payload: {
        version: 1,
        kind: 'download.urls',
        urls: [
          'https://cdn.example.com/one.jpg',
          'https://cdn.example.com/two.jpg'
        ]
      }
    }]);
    expect(commits).toBe(1);
  });

  it('rejects URL input outside the target contract before queueing activation', () => {
    let accepted = false;
    const activate = createBuiltinDownloadActivator(() => {
      accepted = true;
      throw new Error('must not be called');
    });

    expect(activate(['ftp://example.com/file.zip'])).toEqual({
      ok: false,
      code: 'HOST_INVALID_PARAMS',
      message: '下载链接格式无效'
    });
    expect(accepted).toBe(false);
  });

  it('returns stable target availability errors without committing navigation', () => {
    const activate = createBuiltinDownloadActivator(() => {
      throw new AppActivationError('HOST_APP_DISABLED', 'Target application is disabled');
    });

    expect(activate(['https://example.com/video.mp4'])).toEqual({
      ok: false,
      code: 'HOST_APP_DISABLED',
      message: 'Aria2 已停用，请先在应用管理中启用'
    });
  });
});
