<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core';
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';

import { createPluginHostSession, type PluginHostSession } from './pluginHost';
import { validatePluginFrameFailure, type PluginFrameSession } from './pluginFrameSession';
import { createPluginProbeConfig } from './pluginProbe';
import type { PluginOperationSnapshot } from './types';

const props = defineProps<{
  operation: PluginOperationSnapshot;
}>();

const emit = defineEmits<{
  ready: [];
  failed: [];
}>();

const frame = ref<HTMLIFrameElement | null>(null);
const frameUrl = ref('');
const entryIndex = ref(0);
let session: PluginHostSession | null = null;
let frameResourceSession: (PluginFrameSession & { revision: number }) | null = null;
let timer: ReturnType<typeof setTimeout> | null = null;
let settled = false;
let loadRevision = 0;

const config = computed(() => createPluginProbeConfig(props.operation));

function clear() {
  session?.terminate();
  session = null;
  if (timer) clearTimeout(timer);
  timer = null;
}

async function revokeResourceSession() {
  const active = frameResourceSession;
  frameResourceSession = null;
  frameUrl.value = '';
  if (!active) return;
  try {
    await invoke('plugin_revoke_frame_session', { sessionId: active.sessionId });
  } catch {
    // Session revocation is idempotent and best-effort during teardown.
  }
}

async function revokeCreatedSession(created: PluginFrameSession) {
  try {
    await invoke('plugin_revoke_frame_session', { sessionId: created.sessionId });
  } catch {
    // The backend may already have dropped a failed session.
  }
}

function fail() {
  if (settled) return;
  settled = true;
  clear();
  void revokeResourceSession();
  emit('failed');
}

function handleLoad() {
  clear();
  const contentWindow = frame.value?.contentWindow;
  const resourceSession = frameResourceSession;
  if (!contentWindow || !resourceSession) {
    fail();
    return;
  }
  const channel = new MessageChannel();
  session = createPluginHostSession({
    app: config.value.app,
    port: channel.port1,
    correlationId: props.operation.correlationId,
    theme: () => 'dark',
    notify: async () => {},
    saveFile: async () => ({ cancelled: true }),
    onHandshake: () => {
      if (settled) return;
      clear();
      if (entryIndex.value + 1 < config.value.entries.length) {
        entryIndex.value += 1;
        void loadEntry();
        return;
      }
      settled = true;
      void revokeResourceSession();
      emit('ready');
    }
  });
  contentWindow.postMessage(
    {
      type: 'attool:connect',
      apiVersion: config.value.apiVersion,
      appId: props.operation.appId
    },
    new URL(resourceSession.url).origin,
    [channel.port2]
  );
  timer = setTimeout(fail, 8_000);
}

async function loadEntry() {
  const revision = ++loadRevision;
  await revokeResourceSession();
  if (settled || revision !== loadRevision) return;
  try {
    const entry = config.value.entries[entryIndex.value];
    const created = await invoke<PluginFrameSession>('plugin_create_probe_frame_session', {
      operationId: props.operation.operationId,
      entry
    });
    if (settled || revision !== loadRevision) {
      await revokeCreatedSession(created);
      return;
    }
    frameResourceSession = { ...created, revision };
    frameUrl.value = created.url;
  } catch {
    if (!settled && revision === loadRevision) fail();
  }
}

function handleFrameMessage(event: MessageEvent) {
  const active = frameResourceSession;
  const contentWindow = frame.value?.contentWindow;
  if (!active || !contentWindow) return;
  const category = validatePluginFrameFailure(event, {
    source: contentWindow,
    launchToken: active.launchToken,
    revision: active.revision,
    currentRevision: loadRevision,
    launchPending: !settled
  });
  if (category) fail();
}

onMounted(() => {
  window.addEventListener('message', handleFrameMessage);
  void loadEntry();
});

onBeforeUnmount(() => {
  loadRevision += 1;
  window.removeEventListener('message', handleFrameMessage);
  clear();
  void revokeResourceSession();
});
</script>

<template>
  <iframe
    v-if="frameUrl"
    ref="frame"
    class="probe-frame"
    :src="frameUrl"
    :title="`${operation.name} 安装检查`"
    sandbox="allow-scripts allow-same-origin"
    referrerpolicy="no-referrer"
    @load="handleLoad"
    @error="fail"
  />
</template>

<style scoped>
.probe-frame {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  border: 0;
  opacity: 0;
  pointer-events: none;
}
</style>
