<script setup lang="ts">
import { computed, defineAsyncComponent, onMounted, onUnmounted, ref } from 'vue';
import {
  NConfigProvider,
  NMessageProvider,
  darkTheme
} from 'naive-ui';
import { darkOverrides, lightOverrides } from './theme';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import TemplateTool from './components/ecommerce/TemplateTool.vue';
import AppShell from './components/shell/AppShell.vue';
import ShortcutErrorNotifier from './components/shell/ShortcutErrorNotifier.vue';
import Dashboard from './components/shell/Dashboard.vue';
import CommandPalette from './components/shell/CommandPalette.vue';
import { useCommandPalette } from './composables/useCommandPalette';
import { useSidebarState } from './composables/useSidebarState';
import { useLastTool } from './composables/useLastTool';
import { useTheme } from './composables/useTheme';
import { usePluginStore } from './composables/usePluginStore';
import { builtinTools } from './builtinTools';
import UpdateBanner from './components/shell/UpdateBanner.vue';
import SettingsModal from './components/shell/SettingsModal.vue';
import { useUpdater } from './composables/useUpdater';
import { useUpdaterPrefs } from './composables/useUpdaterPrefs';
import type { Tool } from './types/tool';
import { createAppActivationCoordinator } from './components/plugins/appActivation';
import { createBuiltinDownloadActivator } from './components/plugins/builtinDownloadActivation';
import {
  publishRecordingResult,
  requestCaptureMode,
  requestImageTab,
  type RecordingResult
} from './components/image/imageBus';

const DouyinTool = defineAsyncComponent(() => import('./components/douyin/DouyinTool.vue'));
const ImageTool = defineAsyncComponent(() => import('./components/image/ImageTool.vue'));
const NetworkTool = defineAsyncComponent(() => import('./components/network/NetworkTool.vue'));
const AiChatTool = defineAsyncComponent(() => import('./components/ai/AiChatTool.vue'));
const ApplicationManager = defineAsyncComponent(
  () => import('./components/plugins/ApplicationManager.vue')
);
const PluginFrame = defineAsyncComponent(() => import('./components/plugins/PluginFrame.vue'));
const PluginAuxiliaryWindow = defineAsyncComponent(
  () => import('./components/plugins/PluginAuxiliaryWindow.vue')
);

const currentWindow = getCurrentWindow();
const pluginAuxiliaryIdentity = (() => {
  const query = new URLSearchParams(window.location.search);
  const appId = query.get('pluginAppId');
  const windowId = query.get('pluginWindowId');
  return appId && windowId ? { appId, windowId } : null;
})();
const isPluginAuxiliaryWindow = pluginAuxiliaryIdentity !== null;
const isCaptureOverlayWindow = currentWindow.label === 'capture-overlay';
const isCapturePinWindow = currentWindow.label.startsWith('capture-pin-');
const CaptureOverlay = defineAsyncComponent(() => import('./components/image/CaptureOverlay.vue'));
const CapturePinWindow = defineAsyncComponent(() => import('./components/image/CapturePinWindow.vue'));

const { collapsed: sidebarCollapsed, toggle: toggleSidebar } = useSidebarState();
const { lastToolId, remember: rememberLastTool } = useLastTool();
const { theme, toggle: toggleTheme } = useTheme();
const pluginStore = usePluginStore();
const appActivation = createAppActivationCoordinator({
  resolveTarget(appId) {
    const app = pluginStore.apps.value.find((candidate) => candidate.appId === appId);
    if (!app) return { installed: false, enabled: false, available: false };
    const serviceStatus = pluginStore.nativeServiceStatuses.value.get(appId);
    return {
      installed: true,
      enabled: app.enabled,
      available: app.enabled && Boolean(app.entry) && serviceStatus?.state !== 'unavailable'
    };
  },
  navigate: (toolId) => selectTool(toolId)
});
const activateVideoDownload = createBuiltinDownloadActivator(appActivation.accept);
const tools = computed<Tool[]>(() =>
  [...builtinTools, ...pluginStore.navigationTools.value]
);
const {
  open: paletteOpen,
  query: paletteQuery,
  results: paletteResults,
  show: showPalette,
  hide: hidePalette
} = useCommandPalette({
  tools: () => tools.value,
  onOpenTool: (id) => selectTool(id)
});
function onPaletteOpen(v: boolean) {
  v ? showPalette() : hidePalette();
}
function onPaletteQuery(v: string) {
  paletteQuery.value = v;
}
const { state: updaterState, check: updaterCheck, install: updaterInstall, relaunch: updaterRelaunch, dismiss: updaterDismiss } = useUpdater();
const { autoCheck: updaterAutoCheck, setAutoCheck: updaterSetAutoCheck, skipVersion: updaterSkipVersion, shouldSkip: updaterShouldSkip } = useUpdaterPrefs();
const settingsOpen = ref(false);
const naiveTheme = computed(() => (theme.value === 'dark' ? darkTheme : null));
const naiveOverrides = computed(() => (theme.value === 'dark' ? darkOverrides : lightOverrides));
const initialToolId = (() => {
  const id = lastToolId.value;
  if (!id) return null;
  const t = tools.value.find((x) => x.id === id);
  return t && t.status === 'ready' ? id : null;
})();
const selectedToolId = ref<string | null>(initialToolId);

let unlistenCaptureFail: Promise<UnlistenFn> | undefined;
let unlistenRecordingCompleted: Promise<UnlistenFn> | undefined;

onMounted(() => {
  if (
    isPluginAuxiliaryWindow
    || isCaptureOverlayWindow
    || isCapturePinWindow
  ) return;

  void pluginStore.load().then(() => {
    if (!selectedToolId.value && lastToolId.value?.startsWith('plugin:')) {
      selectTool(lastToolId.value);
    }
  });

  unlistenCaptureFail = listen<string>('capture-failed', (event) => {
    // 用户按 Esc 取消不算错误，就直接静默；其它错误在控制台留个痕迹
    const msg = event.payload;
    if (!msg || msg.includes('取消')) return;
    console.warn('[capture]', msg);
  });

  unlistenRecordingCompleted = listen<RecordingResult>('recording://completed', (event) => {
    publishRecordingResult(event.payload);
    selectTool('image');
    requestImageTab('capture');
    requestCaptureMode('recording');
  });

  window.addEventListener('keydown', handleHotkey);

  if (updaterAutoCheck.value) {
    setTimeout(() => runUpdateCheck('auto'), 5000);
  }
});

function handleHotkey(event: KeyboardEvent) {
  const meta = event.metaKey || event.ctrlKey;
  if (!meta) return;

  if (event.key === '\\') {
    event.preventDefault();
    toggleSidebar();
  } else if (event.key === 'k' || event.key === 'K') {
    event.preventDefault();
    openSearch();
  }
}

onUnmounted(() => {
  unlistenCaptureFail?.then((dispose) => dispose()).catch(() => undefined);
  unlistenRecordingCompleted?.then((dispose) => dispose()).catch(() => undefined);
  window.removeEventListener('keydown', handleHotkey);
});

const selectedTool = computed(() =>
  tools.value.find((tool) => tool.id === selectedToolId.value) ?? null
);
const selectedPlugin = computed(() => {
  const appId = selectedTool.value?.pluginAppId;
  return appId ? pluginStore.apps.value.find((app) => app.appId === appId) ?? null : null;
});

function selectTool(id: string) {
  const tool = tools.value.find((t) => t.id === id);
  if (!tool || tool.status !== 'ready') return;
  selectedToolId.value = id;
  rememberLastTool(id);
}

function goHome() {
  selectedToolId.value = null;
}

function openSearch() {
  showPalette();
}

function handleSkip() {
  if (updaterState.value.available) {
    updaterSkipVersion(updaterState.value.available.version);
  }
  updaterDismiss();
}
function openSettings() {
  settingsOpen.value = true;
}
async function runUpdateCheck(trigger: 'auto' | 'manual') {
  await updaterCheck(trigger);
  if (updaterState.value.status === 'available' &&
      updaterShouldSkip(updaterState.value.available?.version ?? '')) {
    updaterDismiss();
  }
}
function handleSettingsCheck() {
  runUpdateCheck('manual');
}
</script>

<template>
  <n-config-provider :theme="naiveTheme" :theme-overrides="naiveOverrides">
    <n-message-provider>
      <ShortcutErrorNotifier
        v-if="!isPluginAuxiliaryWindow && !isCaptureOverlayWindow && !isCapturePinWindow"
      />
      <PluginAuxiliaryWindow
        v-if="pluginAuxiliaryIdentity"
        :app-id="pluginAuxiliaryIdentity.appId"
        :window-id="pluginAuxiliaryIdentity.windowId"
      />
      <CaptureOverlay v-else-if="isCaptureOverlayWindow" />
      <CapturePinWindow v-else-if="isCapturePinWindow" />
      <AppShell
        v-else-if="!isCapturePinWindow"
        :tools="tools"
        :active-id="selectedToolId"
        :collapsed="sidebarCollapsed"
        :plugin-workspace="selectedTool?.source === 'plugin'"
        :theme="theme"
        @select="selectTool"
        @toggle="toggleSidebar"
        @brand="goHome"
        @search="openSearch"
        @theme-toggle="toggleTheme"
        @settings-toggle="openSettings"
      >
        <template #banner>
          <UpdateBanner
            :state="updaterState"
            @install="updaterInstall"
            @skip="handleSkip"
            @relaunch="updaterRelaunch"
            @dismiss="updaterDismiss"
          />
        </template>
        <Dashboard
          v-if="!selectedTool"
          :tools="tools"
          :last-tool-id="lastToolId"
          @open="selectTool"
        />

        <template v-else-if="selectedTool.id === 'template'">
          <TemplateTool />
        </template>

        <template v-else-if="selectedTool.id === 'video-link'">
          <DouyinTool :activate-download="activateVideoDownload" />
        </template>

        <template v-else-if="selectedTool.id === 'image'">
          <ImageTool />
        </template>

        <template v-else-if="selectedTool.id === 'network'">
          <NetworkTool />
        </template>

        <template v-else-if="selectedTool.id === 'ai-chat'">
          <AiChatTool />
        </template>

        <template v-else-if="selectedTool.id === 'app-management'">
          <ApplicationManager
            :builtin-tools="builtinTools"
            :store="pluginStore"
            @open="selectTool"
          />
        </template>

        <template v-else-if="selectedTool.source === 'plugin' && selectedPlugin">
          <PluginFrame
            :key="`${selectedPlugin.appId}:${selectedPlugin.version}`"
            :app="selectedPlugin"
            :theme="theme"
            :activate-app="appActivation.accept"
            :register-app-activation-target="appActivation.registerTarget"
            @version-state="pluginStore.replaceInstalled"
          />
        </template>
      </AppShell>

      <CommandPalette
        :open="paletteOpen"
        :query="paletteQuery"
        :results="paletteResults"
        @update:open="onPaletteOpen"
        @update:query="onPaletteQuery"
      />

      <SettingsModal
        v-model:show="settingsOpen"
        :state="updaterState"
        :auto-check="updaterAutoCheck"
        @check="handleSettingsCheck"
        @install="updaterInstall"
        @relaunch="updaterRelaunch"
        @update:auto-check="updaterSetAutoCheck"
      />
    </n-message-provider>
  </n-config-provider>
</template>
