import { describe, expect, it } from 'vitest';

import { buildPluginFrameDocument, pluginAssetBaseUrl } from './pluginFrameDocument';

describe('plugin frame document', () => {
  it('loads signed assets from the plugin protocol while keeping the document origin opaque', () => {
    const document = buildPluginFrameDocument(
      '<!doctype html><html><head><title>Template</title></head><body><script type="module" src="./assets/app.js"></script></body></html>',
      'attool-plugin://com.attson.template/ui/'
    );

    expect(document).toContain('<base href="attool-plugin://com.attson.template/ui/">');
    expect(document).toContain("connect-src 'none'");
    expect(document).toContain('script-src attool-plugin: attool-plugin-probe:');
    expect(document.indexOf('<base ')).toBeLessThan(document.indexOf('<title>Template</title>'));
    expect(document).toContain('src="./assets/app.js"');
  });

  it('derives the signed asset directory from the manifest entry', () => {
    expect(pluginAssetBaseUrl('attool-plugin', 'com.attson.template', 'ui/index.html')).toBe(
      'attool-plugin://com.attson.template/ui/'
    );
  });

  it('places the security prefix before untrusted document markup', () => {
    const document = buildPluginFrameDocument(
      '<!doctype html><html><body><script>const marker = "<head>";</script></body></html>',
      'attool-plugin://com.attson.template/ui/'
    );

    expect(document.indexOf('Content-Security-Policy')).toBeLessThan(
      document.indexOf('const marker')
    );
    expect(document.indexOf('<base ')).toBeLessThan(document.indexOf('const marker'));
  });
});
