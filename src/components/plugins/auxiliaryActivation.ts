export interface LatestActivationDispatcherOptions<T> {
  fetchLatest(): Promise<T | null>;
  deliver(activation: T): void;
  acknowledge(activation: T): Promise<void>;
  onError(reason: unknown): void;
}

export interface LatestActivationDispatcher<T> {
  enqueue(activation: T): void;
  setReady(): Promise<void>;
  reset(): void;
  dispose(): void;
}

export function createLatestActivationDispatcher<T>(
  options: LatestActivationDispatcherOptions<T>
): LatestActivationDispatcher<T> {
  let pending: T | null = null;
  let running: Promise<void> | null = null;
  let ready = false;
  let disposed = false;

  function report(reason: unknown) {
    try {
      options.onError(reason);
    } catch {
      // Error reporting must not break activation delivery.
    }
  }

  async function drain(fetchWhenEmpty: boolean): Promise<void> {
    if (!ready || disposed) return;
    if (running) return running;

    const task = (async () => {
      if (!pending && fetchWhenEmpty) {
        try {
          const fetched = await options.fetchLatest();
          if (!pending) pending = fetched;
        } catch (reason) {
          report(reason);
        }
      }

      while (ready && !disposed && pending) {
        const activation = pending;
        pending = null;
        try {
          options.deliver(activation);
          await options.acknowledge(activation);
        } catch (reason) {
          report(reason);
        }
      }
    })();
    running = task;
    try {
      await task;
    } finally {
      running = null;
      if (ready && !disposed && pending) void drain(false);
    }
  }

  return {
    enqueue(activation) {
      if (disposed) return;
      pending = activation;
      void drain(false);
    },
    setReady() {
      ready = true;
      return drain(true);
    },
    reset() {
      ready = false;
    },
    dispose() {
      disposed = true;
      ready = false;
      pending = null;
    }
  };
}
