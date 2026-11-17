import { describe, expect, it } from 'vitest';

import { builtinTools } from './builtinTools';

describe('builtinTools', () => {
  it('does not expose the retired built-in video extractor', () => {
    expect(builtinTools.map((tool) => tool.id)).not.toContain('video-link');
  });
});
