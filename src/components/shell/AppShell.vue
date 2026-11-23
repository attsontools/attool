<script setup lang="ts">
import Sidebar from './Sidebar.vue';
import type { Tool } from '../../types/tool';

defineProps<{
  tools: Tool[];
  activeId: string | null;
  collapsed: boolean;
  fullBleed?: boolean;
  theme: 'dark' | 'light';
}>();

const emit = defineEmits<{
  select: [id: string];
  toggle: [];
  brand: [];
  search: [];
  themeToggle: [];
  settingsToggle: [];
}>();
</script>

<template>
  <div class="app-shell">
    <Sidebar
      :tools="tools"
      :active-id="activeId"
      :collapsed="collapsed"
      :theme="theme"
      @select="(id) => emit('select', id)"
      @toggle="emit('toggle')"
      @brand="emit('brand')"
      @search="emit('search')"
      @theme-toggle="emit('themeToggle')"
      @settings-toggle="emit('settingsToggle')"
    />
    <main class="main">
      <slot name="banner" />
      <div class="content" :class="{ 'content--full-bleed': fullBleed }">
        <slot />
      </div>
    </main>
  </div>
</template>

<style scoped>
.app-shell {
  display: flex;
  height: 100vh;
  background: var(--bg-base);
}

.main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}

.content {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: 18px 22px;
}

.content--full-bleed {
  overflow: hidden;
  padding: 0;
}
</style>
