import { describe, it, expect, vi } from 'vitest';
import { useCommandPalette } from './useCommandPalette';
import type { Tool } from '../types/tool';

const fakeTools: Tool[] = [
  { id: 'plugin:com.attson.http', name: 'HTTP 请求', description: 'HTTP body/headers', status: 'ready', icon: 'send', group: 'network', source: 'plugin', pluginAppId: 'com.attson.http' },
  { id: 'plugin:com.attson.json', name: 'JSON 工具', description: '格式化 / JSONPath', status: 'ready', icon: 'code', group: 'edit', source: 'plugin', pluginAppId: 'com.attson.json' },
  { id: 'aria2', name: 'Aria2',    description: '多连接下载',           status: 'ready', icon: 'download', group: 'download' }
];

describe('useCommandPalette', () => {
  it('空 query 时按分区列出全部 tool', () => {
    const onOpenTool = vi.fn();
    const cp = useCommandPalette({ tools: () => fakeTools, onOpenTool });
    cp.show();
    cp.query.value = '';
    expect(cp.results.value.length).toBe(3);
    expect(cp.results.value.every((r) => r.kind === 'tool')).toBe(true);
  });

  it('substring 匹配 name', () => {
    const cp = useCommandPalette({ tools: () => fakeTools, onOpenTool: vi.fn() });
    cp.query.value = 'json';
    expect(cp.results.value.map((r) => r.id)).toEqual(['plugin:com.attson.json']);
  });

  it('substring 匹配 description', () => {
    const cp = useCommandPalette({ tools: () => fakeTools, onOpenTool: vi.fn() });
    cp.query.value = 'headers';
    expect(cp.results.value.map((r) => r.id)).toEqual(['plugin:com.attson.http']);
  });

  it('拼音首字母匹配（HTTP → h）', () => {
    const cp = useCommandPalette({ tools: () => fakeTools, onOpenTool: vi.fn() });
    cp.query.value = 'ht';
    expect(cp.results.value.map((r) => r.id)).toContain('plugin:com.attson.http');
  });

  it('onSelect 触发 onOpenTool 回调', () => {
    const onOpenTool = vi.fn();
    const cp = useCommandPalette({ tools: () => fakeTools, onOpenTool });
    cp.query.value = 'json';
    cp.results.value[0].onSelect();
    expect(onOpenTool).toHaveBeenCalledWith('plugin:com.attson.json');
  });

  it('每分区搜索命中最多 20 条', () => {
    const many = Array.from({ length: 50 }, (_, i) => ({
      id: `t${i}`, name: `Tool ${i}`, description: 'match',
      status: 'ready' as const, icon: 'send' as const, group: 'utility' as const
    }));
    const cp = useCommandPalette({ tools: () => many, onOpenTool: vi.fn() });
    cp.query.value = 'match';
    expect(cp.results.value.length).toBe(20);
  });

  it('show / hide / toggle 控制 open ref', () => {
    const cp = useCommandPalette({ tools: () => fakeTools, onOpenTool: vi.fn() });
    expect(cp.open.value).toBe(false);
    cp.show();
    expect(cp.open.value).toBe(true);
    cp.hide();
    expect(cp.open.value).toBe(false);
    cp.toggle();
    expect(cp.open.value).toBe(true);
  });

  it('hide 时清空 query', () => {
    const cp = useCommandPalette({ tools: () => fakeTools, onOpenTool: vi.fn() });
    cp.show();
    cp.query.value = 'foo';
    cp.hide();
    expect(cp.query.value).toBe('');
  });
});
