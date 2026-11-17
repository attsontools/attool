import { existsSync, readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const manifestPath = 'src-tauri/runtime-manifest.json';

describe('bundled runtime manifest', () => {
  it('pins every host-owned engine and OCR language source', () => {
    expect(existsSync(manifestPath)).toBe(true);
    const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));

    expect(manifest.tesseract.version).toBe('5.5.3');
    expect(manifest.tesseract.vcpkgCommit).toMatch(/^[0-9a-f]{40}$/);
    expect(manifest.tessdata.commit).toMatch(/^[0-9a-f]{40}$/);
    expect(manifest.tessdata.languages).toEqual(['eng', 'chi_sim', 'chi_tra', 'jpn']);
    expect(manifest.aria2.version).toBe('1.37.0');
    expect(manifest.aria2.commit).toMatch(/^[0-9a-f]{40}$/);
    expect(manifest).not.toHaveProperty('ytDlp');
    expect(manifest).not.toHaveProperty('quickJs');
  });

  it('runs runtime preparation before every release build', () => {
    const packageJson = JSON.parse(readFileSync('package.json', 'utf8'));
    const tauriConfig = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8'));
    const prepareRuntime = readFileSync('.github/scripts/prepare-runtime.mjs', 'utf8');

    expect(packageJson.scripts['build:runtime']).toBeTruthy();
    expect(tauriConfig.build.beforeBuildCommand).toContain('pnpm build:runtime');
    expect(prepareRuntime).toContain("process.argv.slice(2).find((argument) => argument !== '--')");
  });

  it('embeds prepared runtime files into the release executable', () => {
    const rustBuild = readFileSync('src-tauri/build.rs', 'utf8');
    const runtimeModule = readFileSync('src-tauri/src/bundled_tools.rs', 'utf8');

    expect(rustBuild).toContain('bundled_tools.rs');
    expect(rustBuild).toContain('EmbeddedFile');
    expect(runtimeModule).toContain('include!(concat!(env!("OUT_DIR"), "/bundled_tools.rs"))');
  });

  it('does not tell users to install bundled engines', () => {
    const ocrPane = readFileSync('src/components/image/OcrPane.vue', 'utf8');
    const ocrBackend = readFileSync('src-tauri/src/imaging/ocr.rs', 'utf8');
    const downloadBackend = readFileSync('src-tauri/src/lib.rs', 'utf8');

    expect(ocrPane).not.toContain('brew install tesseract');
    expect(ocrBackend).not.toContain('apt install tesseract');
    expect(downloadBackend).not.toContain('brew install aria2');
  });

  it('declares the Linux GStreamer ABI and stages every recording element', () => {
    const tauriConfig = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8'));
    const prepareRuntime = readFileSync('.github/scripts/prepare-runtime.mjs', 'utf8');
    const workflow = readFileSync('.github/workflows/build.yml', 'utf8');
    const dependencies = tauriConfig.bundle.linux.deb.depends;

    expect(dependencies).toContain('libgstreamer1.0-0');
    expect(dependencies).toContain('gstreamer1.0-plugins-base');
    expect(dependencies).toContain('gstreamer1.0-plugins-good');
    expect(workflow).toContain('gstreamer1.0-tools');
    for (const element of ['appsrc', 'videoconvert', 'videocrop', 'videoscale', 'queue', 'vp8enc', 'webmmux', 'filesink']) {
      expect(prepareRuntime).toContain(`'${element}'`);
    }
  });

  it('builds native engines from pinned sources on every target', () => {
    const vcpkg = JSON.parse(readFileSync('src-tauri/vcpkg.json', 'utf8'));
    const workflow = readFileSync('.github/workflows/build.yml', 'utf8');
    const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));

    expect(vcpkg['builtin-baseline']).toBe(manifest.tesseract.vcpkgCommit);
    expect(vcpkg.dependencies).toContain('tesseract');
    expect(vcpkg.dependencies).not.toContain('aria2');
    expect(workflow).toContain(`vcpkgGitCommitId: ${manifest.tesseract.vcpkgCommit}`);
    expect(workflow).toContain('build-aria2.mjs');
    expect(manifest.aria2.sourceSha256).toMatch(/^[0-9a-f]{64}$/);
    expect(manifest.aria2.windowsAssetSha256).toMatch(/^[0-9a-f]{64}$/);
    expect(workflow.match(/vcpkg: /g)).toHaveLength(5);
  });

  it('reuses native runtime builds until their pinned inputs change', () => {
    const workflow = readFileSync('.github/workflows/build.yml', 'utf8');

    expect(workflow).toContain('id: native-runtime-cache');
    expect(workflow).toContain('uses: actions/cache/restore@v4');
    expect(workflow).toContain('uses: actions/cache/save@v4');
    expect(workflow).toContain("hashFiles('src-tauri/vcpkg.json', 'src-tauri/runtime-manifest.json', '.github/scripts/build-aria2.mjs')");
    expect(workflow).toContain("steps.native-runtime-cache.outputs.cache-hit != 'true'");
    expect(workflow).toContain('${{ runner.temp }}/attool-vcpkg');
    expect(workflow).toContain('${{ runner.temp }}/attool-native');
  });

  it('keeps the application-owned video runtime out of the host build', () => {
    const workflow = readFileSync('.github/workflows/build.yml', 'utf8');
    const notices = readFileSync('src-tauri/THIRD_PARTY_NOTICES.md', 'utf8');
    const prepareRuntime = readFileSync('.github/scripts/prepare-runtime.mjs', 'utf8');
    const smokeRuntime = readFileSync('.github/scripts/smoke-runtime.mjs', 'utf8');
    const rustBuild = readFileSync('src-tauri/build.rs', 'utf8');
    const runtimeModule = readFileSync('src-tauri/src/bundled_tools.rs', 'utf8');

    expect(existsSync('.github/scripts/install-yt-dlp.mjs')).toBe(false);
    expect(workflow).not.toContain('install-yt-dlp.mjs');
    expect(prepareRuntime).not.toContain('quickjs');
    expect(prepareRuntime).not.toContain('QuickJS');
    expect(smokeRuntime).not.toContain('quickjs');
    expect(smokeRuntime).not.toContain('QuickJS');
    expect(rustBuild).not.toContain('QUICKJS');
    expect(runtimeModule).not.toContain('quickjs()');
    expect(notices).not.toContain('yt-dlp');
    expect(notices).not.toContain('QuickJS');
    expect(existsSync('src-tauri/licenses/yt-dlp-UNLICENSE.txt')).toBe(false);
    expect(existsSync('src-tauri/licenses/QuickJS-ng-MIT.txt')).toBe(false);
  });

  it('allows smoke checks that only require a successful exit status', () => {
    const smokeRuntime = readFileSync('.github/scripts/smoke-runtime.mjs', 'utf8');

    expect(smokeRuntime).toContain('expected !== undefined && !output.includes(expected)');
  });

  it('ships third-party notices, corresponding aria2 source, and size budgets', () => {
    const workflow = readFileSync('.github/workflows/build.yml', 'utf8');
    const prepareRuntime = readFileSync('.github/scripts/prepare-runtime.mjs', 'utf8');
    const sizeCheck = readFileSync('.github/scripts/check-package-size.mjs', 'utf8');

    expect(prepareRuntime).toContain('THIRD_PARTY_NOTICES.md');
    expect(workflow).toContain('aria2-1.37.0-source.tar.gz');
    expect(workflow).toContain('aria2.sourceSha256');
    expect(sizeCheck).toContain("macos: 55");
    expect(sizeCheck).toContain("windows: 55");
    expect(sizeCheck).toContain("linux: 75");
  });
});
