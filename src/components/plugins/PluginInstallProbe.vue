<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core';
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';

import { createPluginHostSession, type PluginHostSession } from './pluginHost';
import { buildPluginFrameDocument, pluginAssetBaseUrl } from './pluginFrameDocument';
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
const frameHtml = ref('');
let session: PluginHostSession | null = null;
let timer: ReturnType<typeof setTimeout> | null = null;
let settled = false;

const config = computed(() => createPluginProbeConfig(props.operation));

function clear() {
  session?.terminate();
  session = null;
  if (timer) clearTimeout(timer);
  timer = null;
}

function fail() {
  if (settled) return;
  settled = true;
  clear();
  emit('failed');
}

function handleLoad() {
  clear();
  const contentWindow = frame.value?.contentWindow;
  if (!contentWindow) {
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
      settled = true;
      clear();
      emit('ready');
    }
  });
  contentWindow.postMessage(
    {
      type: 'attool:connect',
      apiVersion: config.value.apiVersion,
      appId: props.operation.appId
    },
    '*',
    [channel.port2]
  );
  timer = setTimeout(fail, 8_000);
}

onMounted(async () => {
  try {
    const html = await invoke<string>('plugin_read_probe_entry_html', {
      operationId: props.operation.operationId
    });
    frameHtml.value = buildPluginFrameDocument(
      html,
      pluginAssetBaseUrl(
        'attool-plugin-probe',
        props.operation.operationId,
        props.operation.entry ?? ''
      )
    );
  } catch {
    fail();
  }
});

onBeforeUnmount(clear);
</script>

<template>
  <iframe
    v-if="frameHtml"
    ref="frame"
    class="probe-frame"
    :srcdoc="frameHtml"
    :title="`${operation.name} 安装检查`"
    sandbox="allow-scripts"
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
