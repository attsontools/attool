const PLUGIN_FRAME_CSP = [
  "default-src 'none'",
  'script-src attool-plugin: attool-plugin-probe: http://attool-plugin.localhost https://attool-plugin.localhost http://attool-plugin-probe.localhost https://attool-plugin-probe.localhost',
  "style-src attool-plugin: attool-plugin-probe: http://attool-plugin.localhost https://attool-plugin.localhost http://attool-plugin-probe.localhost https://attool-plugin-probe.localhost 'unsafe-inline'",
  'img-src attool-plugin: attool-plugin-probe: http://attool-plugin.localhost https://attool-plugin.localhost http://attool-plugin-probe.localhost https://attool-plugin-probe.localhost data:',
  'font-src attool-plugin: attool-plugin-probe: http://attool-plugin.localhost https://attool-plugin.localhost http://attool-plugin-probe.localhost https://attool-plugin-probe.localhost data:',
  'media-src attool-plugin: attool-plugin-probe: http://attool-plugin.localhost https://attool-plugin.localhost http://attool-plugin-probe.localhost https://attool-plugin-probe.localhost',
  "connect-src 'none'",
  "object-src 'none'",
  'base-uri attool-plugin: attool-plugin-probe: http://attool-plugin.localhost https://attool-plugin.localhost http://attool-plugin-probe.localhost https://attool-plugin-probe.localhost',
  "form-action 'none'"
].join('; ');

export function buildPluginFrameDocument(html: string, baseUrl: string) {
  const base = new URL(baseUrl);
  if (base.protocol !== 'attool-plugin:' && base.protocol !== 'attool-plugin-probe:') {
    throw new Error('Plugin frame base URL must use an AT Tool plugin protocol');
  }
  if (base.username || base.password || base.search || base.hash) {
    throw new Error('Plugin frame base URL must not contain credentials, query, or fragment');
  }

  const prefix = `<meta http-equiv="Content-Security-Policy" content="${PLUGIN_FRAME_CSP}"><base href="${escapeAttribute(base.toString())}">`;
  const content = html.replace(/^\s*<!doctype[^>]*>/i, '');
  return `<!doctype html>${prefix}${content}`;
}

export function pluginAssetBaseUrl(
  protocol: 'attool-plugin' | 'attool-plugin-probe',
  authority: string,
  entry: string
) {
  const segments = entry.split('/');
  segments.pop();
  const directory = segments.map((segment) => encodeURIComponent(segment)).join('/');
  return `${protocol}://${authority}/${directory ? `${directory}/` : ''}`;
}

function escapeAttribute(value: string) {
  return value
    .replaceAll('&', '&amp;')
    .replaceAll('"', '&quot;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;');
}
