export interface AppActivationTargetState {
  installed: boolean;
  enabled: boolean;
  available: boolean;
}

export interface AppActivationRequest {
  sourceAppId: string;
  targetAppId: string;
  payload: unknown;
}

export interface AppActivationNotification {
  activationId: string;
  sourceAppId: string;
  payload: unknown;
}

export interface AcceptedAppActivation {
  result: {
    accepted: true;
    activationId: string;
  };
  commit(): void;
  discard(): void;
}

export class AppActivationError extends Error {
  constructor(readonly stableCode: string, message: string) {
    super(message);
    this.name = 'AppActivationError';
  }
}

export interface AppActivationCoordinator {
  accept(request: AppActivationRequest): AcceptedAppActivation;
  registerTarget(
    appId: string,
    deliver: (activation: AppActivationNotification) => void
  ): () => void;
  pendingCount(appId: string): number;
}

export function createAppActivationCoordinator(options: {
  resolveTarget(appId: string): AppActivationTargetState;
  navigate(toolId: string): void;
  createId?: () => string;
}): AppActivationCoordinator {
  const maxPayloadBytes = 64 * 1024;
  const maxPendingPerTarget = 32;
  type QueuedActivation = {
    notification: AppActivationNotification;
    committed: boolean;
  };
  const queues = new Map<string, QueuedActivation[]>();
  const targets = new Map<string, (activation: AppActivationNotification) => void>();
  const createId = options.createId ?? (() => crypto.randomUUID());

  function drain(appId: string) {
    const deliver = targets.get(appId);
    const queue = queues.get(appId);
    if (!deliver || !queue) return;
    while (queue[0]?.committed) {
      try {
        deliver(queue[0].notification);
        queue.shift();
      } catch {
        return;
      }
    }
    if (queue.length === 0) queues.delete(appId);
  }

  return {
    accept(request) {
      if (request.targetAppId === request.sourceAppId) {
        throw new AppActivationError(
          'HOST_APP_ACTIVATION_SELF_DENIED',
          'An application cannot activate itself'
        );
      }
      const target = options.resolveTarget(request.targetAppId);
      if (!target.installed) {
        throw new AppActivationError('HOST_APP_NOT_INSTALLED', 'Target application is not installed');
      }
      if (!target.enabled) {
        throw new AppActivationError('HOST_APP_DISABLED', 'Target application is disabled');
      }
      if (!target.available) {
        throw new AppActivationError('HOST_APP_UNAVAILABLE', 'Target application is unavailable');
      }
      let payloadBytes: number;
      try {
        payloadBytes = new TextEncoder().encode(JSON.stringify(request.payload) ?? 'null').byteLength;
      } catch {
        throw new AppActivationError('HOST_INVALID_PARAMS', 'Activation payload must be JSON serializable');
      }
      if (payloadBytes > maxPayloadBytes) {
        throw new AppActivationError(
          'HOST_APP_ACTIVATION_PAYLOAD_TOO_LARGE',
          'Activation payload is too large'
        );
      }
      const activationId = createId();
      const queue = queues.get(request.targetAppId) ?? [];
      if (queue.length >= maxPendingPerTarget) {
        throw new AppActivationError(
          'HOST_APP_ACTIVATION_QUEUE_FULL',
          'Target application activation queue is full'
        );
      }
      const queued: QueuedActivation = {
        notification: {
          activationId,
          sourceAppId: request.sourceAppId,
          payload: request.payload
        },
        committed: false
      };
      queue.push(queued);
      queues.set(request.targetAppId, queue);
      let settled = false;
      return {
        result: { accepted: true, activationId },
        commit: () => {
          if (settled) return;
          settled = true;
          queued.committed = true;
          options.navigate(`plugin:${request.targetAppId}`);
          drain(request.targetAppId);
        },
        discard: () => {
          if (settled) return;
          settled = true;
          const currentQueue = queues.get(request.targetAppId);
          if (!currentQueue) return;
          const index = currentQueue.indexOf(queued);
          if (index >= 0) currentQueue.splice(index, 1);
          if (currentQueue.length === 0) queues.delete(request.targetAppId);
          else if (index === 0) drain(request.targetAppId);
        }
      };
    },
    registerTarget(appId, deliver) {
      targets.set(appId, deliver);
      drain(appId);
      return () => {
        if (targets.get(appId) === deliver) targets.delete(appId);
      };
    },
    pendingCount(appId) {
      return queues.get(appId)?.length ?? 0;
    }
  };
}
