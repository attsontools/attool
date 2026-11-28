import { describe, expect, it } from 'vitest';

import { builtinTools } from './builtinTools';

describe('builtinTools', () => {
  it('does not expose the retired Aria2 implementation', () => {
    expect(builtinTools.map((tool) => tool.id)).not.toContain('aria2');
  });
});
