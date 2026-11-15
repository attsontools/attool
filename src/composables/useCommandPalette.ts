import { computed, ref } from 'vue';
import type { ComputedRef, Ref } from 'vue';
import type { Tool } from '../types/tool';

export interface CommandItem {
  kind: 'tool';
  id: string;
  title: string;
  subtitle?: string;
  groupLabel: string;
  onSelect: () => void;
}

export interface UseCommandPaletteInput {
  tools: () => Tool[];
  onOpenTool: (id: string) => void;
}

export interface UseCommandPaletteReturn {
  open: Ref<boolean>;
  query: Ref<string>;
  results: ComputedRef<CommandItem[]>;
  show: () => void;
  hide: () => void;
  toggle: () => void;
}

const PER_SECTION_LIMIT = 20;

// 拼音首字母映射（简版，覆盖常见工具名首字）。命中失败落回 substring。
const PINYIN_INITIAL: Record<string, string> = {
  下: 'x', 载: 'z',
  主: 'z', 图: 't', 模: 'm', 板: 'b',
  剪: 'j', 贴: 't',
  文: 'w', 本: 'b',
  网: 'w', 络: 'l',
  编: 'b', 码: 'm',
  生: 's', 成: 'c', 器: 'q',
  时: 's', 间: 'j',
  视: 's', 频: 'p', 链: 'l', 接: 'j', 抽: 'c', 取: 'q',
  片: 'p',
  请: 'q', 求: 'q',
  工: 'g', 具: 'j'
};

function toInitials(text: string): string {
  let out = '';
  for (const ch of text) {
    if (/[a-zA-Z0-9]/.test(ch)) out += ch.toLowerCase();
    else if (PINYIN_INITIAL[ch]) out += PINYIN_INITIAL[ch];
  }
  return out;
}

function matches(haystack: string, needle: string): boolean {
  if (!needle) return true;
  const q = needle.toLowerCase().trim();
  if (!q) return true;
  if (haystack.toLowerCase().includes(q)) return true;
  return toInitials(haystack).includes(q);
}

export function useCommandPalette(input: UseCommandPaletteInput): UseCommandPaletteReturn {
  const open = ref(false);
  const query = ref('');

  function show()  { open.value = true; }
  function hide()  { open.value = false; query.value = ''; }
  function toggle() { open.value ? hide() : show(); }

  const results = computed<CommandItem[]>(() => {
    const q = query.value.trim();
    return input.tools()
      .filter((t) =>
        matches(t.name, q) ||
        matches(t.description, q) ||
        matches(t.id, q)
      )
      .slice(0, PER_SECTION_LIMIT)
      .map((t) => ({
        kind: 'tool' as const,
        id: t.id,
        title: t.name,
        subtitle: t.description,
        groupLabel: '工具',
        onSelect: () => input.onOpenTool(t.id)
      }));
  });

  return { open, query, results, show, hide, toggle };
}
