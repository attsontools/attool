const DESCRIPTIONS: Record<string, string> = {
  'host.theme.read': '读取 AT Tool 当前的深浅主题。',
  'host.file.save': '通过系统保存对话框写入你选择的文件。',
  'host.notification.show': '在 AT Tool 中显示通知。',
  'host.http.request': '发送一次性 HTTP 请求。',
  'host.http.stream': '建立并调试 SSE 或 WebSocket 长连接。',
  'host.http.data': '读写该应用的请求、环境和历史数据。',
  'host.download.tasks': '创建、管理并查看下载任务。',
  'host.template.project': '读写主图模板、素材和批处理项目。',
  'host.app.activate': '打开其他已安装应用',
  'host.clipboard.write': '写入系统剪贴板',
  'host.external.open': '使用系统默认应用打开外部链接',
  'host.video.extract': '解析视频链接',
  'host.ui.auxiliary': '打开该应用已签名声明的辅助窗口。',
  'host.clipboard.history': '读取剪贴板文本、图片和文件路径，并管理历史与采集设置。',
};

export function permissionDescription(permission: string) {
  return DESCRIPTIONS[permission] ?? '应用请求此宿主能力。';
}
