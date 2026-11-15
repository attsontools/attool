<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';
import { open, save } from '@tauri-apps/plugin-dialog';
import { NButton, NSpin, useMessage } from 'naive-ui';

import type { ThemeName } from '../../composables/useTheme';
import {
  createPluginHostSession,
  type HostFileSaveRequest,
  type HostTemplateFilePickRequest,
  type HostUiOpenRequest,
  type PluginHostSession
} from './pluginHost';
import { createLatestActivationDispatcher } from './auxiliaryActivation';
import { buildPluginFrameDocument, pluginAssetBaseUrl } from './pluginFrameDocument';
import type { InstalledPlugin } from './types';

const props = defineProps<{
  app: InstalledPlugin;
  theme: ThemeName;
  entry?: string;
  windowId?: string;
  auxiliary?: boolean;
}>();

const emit = defineEmits<{
  versionState: [app: InstalledPlugin];
}>();

type FrameState = 'loading' | 'connecting' | 'ready' | 'error';

const message = useMessage();
const frame = ref<HTMLIFrameElement | null>(null);
const state = ref<FrameState>('loading');
const errorMessage = ref('');
const reloadRevision = ref(0);
const serviceReady = ref(false);
const frameHtml = ref('');
const frameEntry = computed(() => props.entry ?? props.app.entry);
let session: PluginHostSession | null = null;
let handshakeTimer: ReturnType<typeof setTimeout> | null = null;
let handlingFailure = false;
let nativeServiceEventUnlisten: UnlistenFn | null = null;
let auxiliaryActivationUnlisten: UnlistenFn | null = null;
let disposed = false;

type AuxiliaryActivation = {
  activationId: string;
  appId: string;
  windowId: string;
  payload: unknown;
};

const activationDispatcher = createLatestActivationDispatcher<AuxiliaryActivation>({
  fetchLatest: () => invoke<AuxiliaryActivation | null>(
    'plugin_get_auxiliary_activation',
    { appId: props.app.appId, windowId: props.windowId }
  ),
  deliver: (activation) => session?.notifyActivated(activation.payload),
  acknowledge: async (activation) => {
    await invoke<boolean>('plugin_ack_auxiliary_activation', {
      appId: props.app.appId,
      windowId: props.windowId,
      activationId: activation.activationId
    });
  },
  onError: () => message.error('无法同步应用窗口激活状态')
});

function clearSession() {
  session?.terminate();
  session = null;
  activationDispatcher.reset();
  if (handshakeTimer) clearTimeout(handshakeTimer);
  handshakeTimer = null;
}

async function savePluginFile(input: HostFileSaveRequest) {
  const path = await save({ defaultPath: input.suggestedName });
  if (!path) return { cancelled: true };
  const base64 = input.encoding === 'base64' ? input.content : utf8ToBase64(input.content);
  await invoke('write_binary_file', { path, base64 });
  return { cancelled: false };
}

async function pickPluginFile() {
  const path = await open({ multiple: false });
  if (!path || typeof path !== 'string') return null;
  return { path, name: path.split(/[\\/]/).pop() || 'file' };
}

async function pickPluginDirectory(defaultPath?: string) {
  const path = await open({ directory: true, multiple: false, defaultPath });
  return typeof path === 'string' ? path : null;
}

async function pickTemplateFiles(input: HostTemplateFilePickRequest) {
  const selected = await open({
    multiple: input.multiple,
    filters: input.kind === 'psd'
      ? [{ name: 'PSD', extensions: ['psd'] }]
      : [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp'] }]
  });
  const paths = typeof selected === 'string' ? [selected] : selected;
  if (!paths?.length) return null;
  return paths.map((path) => ({ path, name: pathName(path) }));
}

async function pickTemplateDirectory() {
  const path = await open({ directory: true, multiple: false });
  return typeof path === 'string' ? { path, name: pathName(path) } : null;
}

function pathName(path: string) {
  return path.split(/[\\/]/).filter(Boolean).pop() || 'file';
}

async function prepareFrame() {
  serviceReady.value = false;
  state.value = props.app.nativeService ? 'connecting' : 'loading';
  errorMessage.value = '';
  try {
    if (props.app.nativeService && !nativeServiceEventUnlisten) {
      const unlisten = await listen<{ appId: string; event: unknown }>(
        'plugin://native-service-event',
        ({ payload }) => {
          if (payload.appId === props.app.appId) session?.notifyNativeServiceEvent(payload.event);
        }
      );
      if (disposed) {
        unlisten();
        return;
      }
      nativeServiceEventUnlisten = unlisten;
    }
    if (props.auxiliary && props.windowId && !auxiliaryActivationUnlisten) {
      const unlisten = await listen<AuxiliaryActivation>(
        'plugin://auxiliary-activation',
        ({ payload }) => {
          if (payload.appId !== props.app.appId || payload.windowId !== props.windowId) return;
          activationDispatcher.enqueue(payload);
        }
      );
      if (disposed) {
        unlisten();
        return;
      }
      auxiliaryActivationUnlisten = unlisten;
    }
    if (props.app.nativeService) {
      await invoke('plugin_connect_native_service', { appId: props.app.appId });
    }
    const html = await invoke<string>('plugin_read_entry_html', {
      appId: props.app.appId,
      entry: frameEntry.value
    });
    if (disposed) return;
    frameHtml.value = `${buildPluginFrameDocument(
      html,
      pluginAssetBaseUrl('attool-plugin', props.app.appId, frameEntry.value)
    )}\n<!-- reload:${reloadRevision.value} -->`;
    state.value = 'loading';
    serviceReady.value = true;
  } catch (reason) {
    await handleLaunchFailure(serviceErrorMessage(reason));
  }
}

async function openAuxiliaryWindow(input: HostUiOpenRequest) {
  return invoke('plugin_open_auxiliary_window', {
    appId: props.app.appId,
    windowId: input.windowId,
    payload: input.payload
  });
}

function handleLoad() {
  clearSession();
  handlingFailure = false;
  const contentWindow = frame.value?.contentWindow;
  if (!contentWindow) {
    void handleLaunchFailure('无法建立应用窗口连接');
    return;
  }
  state.value = 'connecting';
  errorMessage.value = '';
  const channel = new MessageChannel();
  session = createPluginHostSession({
    app: props.app,
    port: channel.port1,
    windowId: props.windowId,
    theme: () => props.theme,
    notify: async ({ title, message: body, kind }) => {
      message[kind](title ? `${title}: ${body}` : body);
    },
    saveFile: savePluginFile,
    pickFile: pickPluginFile,
    pickDirectory: pickPluginDirectory,
    pickTemplateFiles,
    pickTemplateDirectory,
    copyText: writeText,
    openAuxiliaryWindow,
    closeAuxiliaryWindow: props.auxiliary
      ? () => getCurrentWindow().close()
      : undefined,
    requestNativeService: props.app.nativeService
      ? (method, params) => invoke('plugin_native_service_request', {
          appId: props.app.appId,
          method,
          params
        })
      : undefined,
    onHandshake: () => {
      if (handshakeTimer) clearTimeout(handshakeTimer);
      handshakeTimer = null;
      if (props.auxiliary) {
        state.value = 'ready';
        void activationDispatcher.setReady();
      } else {
        void confirmLaunch();
      }
    }
  });
  contentWindow.postMessage(
    {
      type: 'attool:connect',
      apiVersion: props.app.nativeService ? '2.0' : '1.0',
      appId: props.app.appId
    },
    '*',
    [channel.port2]
  );
  handshakeTimer = setTimeout(() => {
    clearSession();
    void handleLaunchFailure('应用未在规定时间内完成 Host API 握手');
  }, 8_000);
}

async function confirmLaunch() {
  try {
    await invoke<boolean>('plugin_confirm_plugin_launch', {
      appId: props.app.appId,
      version: props.app.version
    });
    state.value = 'ready';
  } catch {
    clearSession();
    await handleLaunchFailure('无法确认当前应用版本');
  }
}

async function handleLaunchFailure(reason: string) {
  if (handlingFailure) return;
  handlingFailure = true;
  if (props.auxiliary) {
    fail(reason);
    return;
  }
  try {
    const rolledBack = await invoke<InstalledPlugin | null>('plugin_rollback_plugin_launch', {
      appId: props.app.appId,
      version: props.app.version
    });
    if (rolledBack) {
      message.warning(`启动失败，已恢复 ${rolledBack.version}`);
      emit('versionState', rolledBack);
      return;
    }
  } catch {
    fail('自动回滚失败，请在应用管理中查看操作记录');
    return;
  }
  fail(reason);
}

function fail(reason: string) {
  state.value = 'error';
  errorMessage.value = reason;
}

function reload() {
  clearSession();
  state.value = 'loading';
  errorMessage.value = '';
  handlingFailure = false;
  serviceReady.value = false;
  frameHtml.value = '';
  reloadRevision.value += 1;
  void prepareFrame();
}

function serviceErrorMessage(reason: unknown) {
  if (typeof reason === 'object' && reason !== null && 'message' in reason) {
    const message = (reason as { message?: unknown }).message;
    if (typeof message === 'string' && message.trim()) return message;
  }
  return '无法连接应用服务';
}

function utf8ToBase64(value: string) {
  const bytes = new TextEncoder().encode(value);
  let binary = '';
  for (let offset = 0; offset < bytes.length; offset += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000));
  }
  return btoa(binary);
}

watch(
  () => props.theme,
  (theme) => session?.notifyTheme(theme)
);

onMounted(() => void prepareFrame());

onBeforeUnmount(() => {
  disposed = true;
  clearSession();
  nativeServiceEventUnlisten?.();
  nativeServiceEventUnlisten = null;
  auxiliaryActivationUnlisten?.();
  auxiliaryActivationUnlisten = null;
  activationDispatcher.dispose();
});
</script>

<template>
  <section class="plugin-frame" :class="{ auxiliary }">
    <iframe
      v-if="serviceReady"
      ref="frame"
      :srcdoc="frameHtml"
      :title="windowId ? `${app.name} ${windowId}` : app.name"
      sandbox="allow-scripts"
      referrerpolicy="no-referrer"
      @load="handleLoad"
      @error="handleLaunchFailure('应用资源加载失败')"
    />

    <div v-if="state === 'loading' || state === 'connecting'" class="frame-state">
      <n-spin size="small" />
      <span>{{ state === 'loading' ? '正在加载应用' : '正在建立安全连接' }}</span>
    </div>

    <div v-else-if="state === 'error'" class="frame-state error-state">
      <strong>应用无法启动</strong>
      <span>{{ errorMessage }}</span>
      <n-button size="small" secondary @click="reload">重新加载</n-button>
    </div>
  </section>
</template>

<style scoped>
.plugin-frame {
  position: relative;
  width: 100%;
  height: 100%;
  min-height: 480px;
  overflow: hidden;
  border: 1px solid var(--line);
  border-radius: var(--radius-md);
  background: var(--bg-elevated);
}

.plugin-frame.auxiliary,
.plugin-frame.auxiliary iframe {
  min-height: 100vh;
  border: 0;
  border-radius: 0;
}

iframe {
  display: block;
  width: 100%;
  height: 100%;
  min-height: 480px;
  border: 0;
  background: var(--bg-base);
}

.frame-state {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 9px;
  background: var(--bg-elevated);
  color: var(--text-muted);
  font-size: var(--fs-xs);
}

.error-state {
  flex-direction: column;
}

.error-state strong {
  color: var(--text);
  font-size: var(--fs-md);
}
</style>
