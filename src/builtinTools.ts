import type { Tool } from './types/tool';

export const builtinTools: Tool[] = [
  { id: 'app-management', name: '应用管理', description: '安装、启停和卸载独立应用', status: 'ready', icon: 'apps', group: 'platform', source: 'platform' },
  { id: 'aria2', name: 'Aria2 下载', description: 'HTTP / HTTPS / FTP / BT 多连接下载', status: 'ready', icon: 'download', group: 'download' },
  { id: 'template', name: '主图模板', description: 'PSD 导入、字段替换、批量生成主图', status: 'ready', icon: 'layout', group: 'edit' },
  { id: 'image', name: '图片工具', description: '截图 / 录屏 / 标注 / 压缩 / 转换 / OCR', status: 'ready', icon: 'image', group: 'edit' },
  { id: 'network', name: '网络工具', description: 'URL 分析 / Ping / 端口检查 / DNS', status: 'ready', icon: 'wifi', group: 'network' },
  { id: 'ai-chat', name: 'AI 对话', description: '多提供商多模型 AI 助手（OpenAI 兼容 / Claude / Ollama）', status: 'ready', icon: 'chat', group: 'utility' }
];
