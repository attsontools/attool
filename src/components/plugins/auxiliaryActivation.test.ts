import { describe, expect, it, vi } from 'vitest';

import { createLatestActivationDispatcher } from './auxiliaryActivation';

type Activation = { activationId: string };

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, resolve, reject };
}

async function flush() {
  await Promise.resolve();
  await Promise.resolve();
}

describe('latest auxiliary activation dispatcher', () => {
  it('keeps an event received while the initial activation is being fetched', async () => {
    const fetched = deferred<Activation | null>();
    const deliver = vi.fn();
    const acknowledge = vi.fn().mockResolvedValue(undefined);
    const dispatcher = createLatestActivationDispatcher({
      fetchLatest: () => fetched.promise,
      deliver,
      acknowledge,
      onError: vi.fn()
    });

    const ready = dispatcher.setReady();
    dispatcher.enqueue({ activationId: 'newest' });
    fetched.resolve({ activationId: 'older' });
    await ready;

    expect(deliver).toHaveBeenCalledTimes(1);
    expect(deliver).toHaveBeenCalledWith({ activationId: 'newest' });
    expect(acknowledge).toHaveBeenCalledWith({ activationId: 'newest' });
  });

  it('coalesces activations received during acknowledgement to the latest one', async () => {
    const firstAcknowledgement = deferred<void>();
    const deliver = vi.fn();
    const acknowledge = vi.fn()
      .mockImplementationOnce(() => firstAcknowledgement.promise)
      .mockResolvedValue(undefined);
    const dispatcher = createLatestActivationDispatcher<Activation>({
      fetchLatest: async () => null,
      deliver,
      acknowledge,
      onError: vi.fn()
    });
    await dispatcher.setReady();

    dispatcher.enqueue({ activationId: 'first' });
    await flush();
    dispatcher.enqueue({ activationId: 'second' });
    dispatcher.enqueue({ activationId: 'third' });
    firstAcknowledgement.resolve();
    await flush();
    await flush();

    expect(deliver.mock.calls.map(([activation]) => activation.activationId)).toEqual([
      'first',
      'third'
    ]);
  });

  it('reports acknowledgement failures and continues with the next activation', async () => {
    const onError = vi.fn();
    const deliver = vi.fn();
    const acknowledge = vi.fn()
      .mockRejectedValueOnce(new Error('ack unavailable'))
      .mockResolvedValue(undefined);
    const dispatcher = createLatestActivationDispatcher<Activation>({
      fetchLatest: async () => null,
      deliver,
      acknowledge,
      onError
    });
    await dispatcher.setReady();

    dispatcher.enqueue({ activationId: 'first' });
    await flush();
    dispatcher.enqueue({ activationId: 'second' });
    await flush();
    await flush();

    expect(onError).toHaveBeenCalledOnce();
    expect(deliver.mock.calls.map(([activation]) => activation.activationId)).toEqual([
      'first',
      'second'
    ]);
  });
});
