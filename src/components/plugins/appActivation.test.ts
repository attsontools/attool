import { describe, expect, it, vi } from 'vitest';

import { AppActivationError, createAppActivationCoordinator } from './appActivation';

describe('app activation coordinator', () => {
  it('accepts before navigation and delivers queued activations in FIFO order', () => {
    const navigate = vi.fn();
    let nextId = 1;
    const coordinator = createAppActivationCoordinator({
      resolveTarget: () => ({ installed: true, enabled: true, available: true }),
      navigate,
      createId: () => `activation-${nextId++}`
    });

    const first = coordinator.accept({
      sourceAppId: 'com.attson.video',
      targetAppId: 'com.attson.aria2',
      payload: { version: 1, kind: 'download.urls', urls: ['https://example.com/a.mp4'] }
    });
    const second = coordinator.accept({
      sourceAppId: 'com.attson.video',
      targetAppId: 'com.attson.aria2',
      payload: { version: 1, kind: 'download.urls', urls: ['https://example.com/b.mp4'] }
    });

    expect(first.result).toEqual({ accepted: true, activationId: 'activation-1' });
    expect(second.result).toEqual({ accepted: true, activationId: 'activation-2' });
    expect(coordinator.pendingCount('com.attson.aria2')).toBe(2);
    expect(navigate).not.toHaveBeenCalled();

    const delivered: unknown[] = [];
    coordinator.registerTarget('com.attson.aria2', (activation) => delivered.push(activation));

    first.commit();
    expect(delivered).toEqual([
      {
        activationId: 'activation-1',
        sourceAppId: 'com.attson.video',
        payload: { version: 1, kind: 'download.urls', urls: ['https://example.com/a.mp4'] }
      }
    ]);
    expect(coordinator.pendingCount('com.attson.aria2')).toBe(1);

    second.commit();
    expect(navigate).toHaveBeenNthCalledWith(1, 'plugin:com.attson.aria2');
    expect(navigate).toHaveBeenNthCalledWith(2, 'plugin:com.attson.aria2');

    expect(delivered).toEqual([
      {
        activationId: 'activation-1',
        sourceAppId: 'com.attson.video',
        payload: { version: 1, kind: 'download.urls', urls: ['https://example.com/a.mp4'] }
      },
      {
        activationId: 'activation-2',
        sourceAppId: 'com.attson.video',
        payload: { version: 1, kind: 'download.urls', urls: ['https://example.com/b.mp4'] }
      }
    ]);
    expect(coordinator.pendingCount('com.attson.aria2')).toBe(0);
  });

  it('discards an activation that could not be acknowledged to the source session', () => {
    const navigate = vi.fn();
    const coordinator = createAppActivationCoordinator({
      resolveTarget: () => ({ installed: true, enabled: true, available: true }),
      navigate,
      createId: () => 'activation-1'
    });
    const accepted = coordinator.accept({
      sourceAppId: 'com.attson.video',
      targetAppId: 'com.attson.aria2',
      payload: { version: 1 }
    });

    accepted.discard();
    accepted.commit();

    expect(coordinator.pendingCount('com.attson.aria2')).toBe(0);
    expect(navigate).not.toHaveBeenCalled();
  });

  it('rejects unavailable targets, self activation, oversized payloads and a full queue', () => {
    let target = { installed: true, enabled: true, available: true };
    const navigate = vi.fn();
    const coordinator = createAppActivationCoordinator({
      resolveTarget: () => target,
      navigate,
      createId: () => crypto.randomUUID()
    });
    const request = {
      sourceAppId: 'com.attson.video',
      targetAppId: 'com.attson.aria2',
      payload: { version: 1 }
    };
    const expectCode = (run: () => unknown, code: string) => {
      try {
        run();
        throw new Error('expected activation to fail');
      } catch (error) {
        expect(error).toBeInstanceOf(AppActivationError);
        expect((error as AppActivationError).stableCode).toBe(code);
      }
    };

    target = { installed: false, enabled: false, available: false };
    expectCode(() => coordinator.accept(request), 'HOST_APP_NOT_INSTALLED');
    target = { installed: true, enabled: false, available: false };
    expectCode(() => coordinator.accept(request), 'HOST_APP_DISABLED');
    target = { installed: true, enabled: true, available: false };
    expectCode(() => coordinator.accept(request), 'HOST_APP_UNAVAILABLE');
    target = { installed: true, enabled: true, available: true };
    expectCode(
      () => coordinator.accept({ ...request, targetAppId: request.sourceAppId }),
      'HOST_APP_ACTIVATION_SELF_DENIED'
    );
    expectCode(
      () => coordinator.accept({ ...request, payload: { data: 'x'.repeat(64 * 1024) } }),
      'HOST_APP_ACTIVATION_PAYLOAD_TOO_LARGE'
    );

    for (let index = 0; index < 32; index += 1) coordinator.accept(request);
    expectCode(() => coordinator.accept(request), 'HOST_APP_ACTIVATION_QUEUE_FULL');
    expect(coordinator.pendingCount(request.targetAppId)).toBe(32);
    expect(navigate).not.toHaveBeenCalled();
  });

  it('retains accepted activations when delivery fails and retries after registration', () => {
    let nextId = 1;
    const coordinator = createAppActivationCoordinator({
      resolveTarget: () => ({ installed: true, enabled: true, available: true }),
      navigate: vi.fn(),
      createId: () => `activation-${nextId++}`
    });
    const request = {
      sourceAppId: 'com.attson.video',
      targetAppId: 'com.attson.aria2',
      payload: { version: 1, kind: 'download.urls', urls: ['https://example.com/a.mp4'] }
    };
    const first = coordinator.accept(request);
    const second = coordinator.accept({
      ...request,
      payload: { version: 1, kind: 'download.urls', urls: ['https://example.com/b.mp4'] }
    });
    first.commit();
    second.commit();

    expect(() => coordinator.registerTarget(request.targetAppId, () => {
      throw new Error('target session closed');
    })).not.toThrow();
    expect(coordinator.pendingCount(request.targetAppId)).toBe(2);

    const delivered: string[] = [];
    coordinator.registerTarget(request.targetAppId, (activation) => {
      delivered.push(activation.activationId);
    });

    expect(delivered).toEqual(['activation-1', 'activation-2']);
    expect(coordinator.pendingCount(request.targetAppId)).toBe(0);
  });

  it('does not deliver to an already registered target before acceptance is committed', () => {
    const delivered: string[] = [];
    const coordinator = createAppActivationCoordinator({
      resolveTarget: () => ({ installed: true, enabled: true, available: true }),
      navigate: vi.fn(),
      createId: () => 'activation-1'
    });
    coordinator.registerTarget('com.attson.aria2', (activation) => {
      delivered.push(activation.activationId);
    });

    const accepted = coordinator.accept({
      sourceAppId: 'com.attson.video',
      targetAppId: 'com.attson.aria2',
      payload: { version: 1, kind: 'download.urls', urls: ['https://example.com/a.mp4'] }
    });

    expect(delivered).toEqual([]);
    expect(coordinator.pendingCount('com.attson.aria2')).toBe(1);
    accepted.commit();
    expect(delivered).toEqual(['activation-1']);
  });
});
