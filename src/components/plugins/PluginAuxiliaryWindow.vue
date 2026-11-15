<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core';
import { onMounted, ref } from 'vue';
import { NSpin } from 'naive-ui';

import { useTheme } from '../../composables/useTheme';
import PluginFrame from './PluginFrame.vue';
import type { InstalledPlugin } from './types';

const props = defineProps<{
  appId: string;
  windowId: string;
}>();

const { theme } = useTheme();
const app = ref<InstalledPlugin | null>(null);
const error = ref('');

onMounted(async () => {
  try {
    const installed = await invoke<InstalledPlugin[]>('plugin_list_installed');
    const match = installed.find((candidate) => candidate.appId === props.appId && candidate.enabled);
    if (
      !match
      || !match.permissions.includes('host.ui.auxiliary')
      || !match.windows[props.windowId]
    ) {
      error.value = '应用窗口不可用';
      return;
    }
    app.value = match;
  } catch {
    error.value = '无法读取应用窗口';
  }
});
</script>

<template>
  <PluginFrame
    v-if="app"
    :app="app"
    :theme="theme"
    :entry="app.windows[windowId].entry"
    :window-id="windowId"
    auxiliary
  />
  <div v-else class="auxiliary-state">
    <n-spin v-if="!error" size="small" />
    <span>{{ error || '正在加载应用窗口' }}</span>
  </div>
</template>

<style scoped>
.auxiliary-state {
  width: 100vw;
  height: 100vh;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 9px;
  background: var(--bg-base);
  color: var(--text-muted);
  font-size: var(--fs-xs);
}
</style>
