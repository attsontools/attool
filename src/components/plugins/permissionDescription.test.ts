import { describe, expect, it } from 'vitest';

import { permissionDescription } from './permissionDescription';

describe('plugin permission descriptions', () => {
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
