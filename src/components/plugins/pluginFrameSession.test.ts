import { describe, expect, it } from 'vitest';

import {
  pluginFrameFailureMessage,
  validatePluginFrameFailure
} from './pluginFrameSession';

describe('plugin frame failure messages', () => {
  const source = {};
  const context = {
    source,
    launchToken: 'current-token',
    revision: 4,
    currentRevision: 4,
    launchPending: true
  };

  it('accepts a fixed failure category from the current frame and launch', () => {
    expect(validatePluginFrameFailure({
      source,
      data: {
        type: 'attool:frame-error',
        launchToken: 'current-token',
        category: 'preload'
      }
    }, context)).toBe('preload');
  });

  it('rejects messages from another frame, launch, or loading revision', () => {
    expect(validatePluginFrameFailure({
      source: {},
      data: {
        type: 'attool:frame-error',
        launchToken: 'current-token',
        category: 'resource'
      }
    }, context)).toBeNull();
    expect(validatePluginFrameFailure({
      source,
      data: {
        type: 'attool:frame-error',
        launchToken: 'old-token',
        category: 'resource'
      }
    }, context)).toBeNull();
    expect(validatePluginFrameFailure({
      source,
      data: {
        type: 'attool:frame-error',
        launchToken: 'current-token',
        category: 'resource'
      }
    }, { ...context, currentRevision: 5 })).toBeNull();
  });

  it('ignores runtime errors after the launch has completed', () => {
    expect(validatePluginFrameFailure({
      source,
      data: {
        type: 'attool:frame-error',
        launchToken: 'current-token',
        category: 'runtime'
      }
    }, { ...context, launchPending: false })).toBeNull();
  });

  it('rejects plugin-controlled message text and unknown categories', () => {
    expect(validatePluginFrameFailure({
      source,
      data: {
        type: 'attool:frame-error',
        launchToken: 'current-token',
        category: '<img src=x onerror=alert(1)>',
        message: '/home/user/private/plugin.js'
      }
    }, context)).toBeNull();
  });

  it('maps categories to stable host-owned user messages', () => {
    expect(pluginFrameFailureMessage('resource')).toBe('应用资源加载失败');
    expect(pluginFrameFailureMessage('preload')).toBe('应用模块加载失败');
    expect(pluginFrameFailureMessage('runtime')).toBe('应用启动脚本执行失败');
  });
});
