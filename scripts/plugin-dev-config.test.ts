import { describe, expect, it } from 'vitest';
import path from 'node:path';

import { resolvePluginDevOverrides } from './plugin-dev-config';

describe('resolvePluginDevOverrides', () => {
  it('resolves configured UI roots relative to the repository', () => {
    expect(resolvePluginDevOverrides(
      { 'com.attson.json': '../atjson/dist-plugin' },
      '/workspace/attool',
      () => true,
    )).toEqual({
      'com.attson.json': path.resolve('/workspace/attool', '../atjson/dist-plugin'),
    });
  });

  it('rejects a build directory without index.html', () => {
    expect(() => resolvePluginDevOverrides(
      { 'com.attson.json': '../atjson/dist-plugin' },
      '/workspace/attool',
      () => false,
    )).toThrow('com.attson.json UI entry does not exist');
  });

  it('rejects invalid application identifiers and paths', () => {
    expect(() => resolvePluginDevOverrides(
      { '../json': '../atjson/dist-plugin' },
      '/workspace/attool',
      () => true,
    )).toThrow('Invalid plugin app ID');
    expect(() => resolvePluginDevOverrides(
      { 'com.attson.json': '' },
      '/workspace/attool',
      () => true,
    )).toThrow('must be a non-empty path');
  });
});
