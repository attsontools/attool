import { describe, expect, it } from 'vitest';

import { permissionDescription } from './permissionDescription';

describe('plugin permission descriptions', () => {
  it.each([
    ['host.app.activate', '打开其他已安装应用'],
    ['host.clipboard.write', '写入系统剪贴板'],
    ['host.external.open', '使用系统默认应用打开外部链接'],
    ['host.video.extract', '解析视频链接'],
  ])('describes %s during installation', (permission, description) => {
    expect(permissionDescription(permission)).toBe(description);
  });

  it('discloses every clipboard data class available to an installed plugin', () => {
    const description = permissionDescription('host.clipboard.history');

    expect(description).toContain('文本');
    expect(description).toContain('图片');
    expect(description).toContain('文件路径');
  });

  it('keeps unknown future permissions explicit instead of hiding them', () => {
    expect(permissionDescription('host.future.capability')).toBe('应用请求此宿主能力。');
  });
});
