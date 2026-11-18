import { existsSync, readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

describe('retired host video implementation', () => {
  it('removes the built-in video UI and process-local Aria2 handoff', () => {
    for (const path of [
      'src/components/douyin/DouyinTool.vue',
      'src/composables/useAria2Handoff.ts',
      'src/composables/useAria2Handoff.test.ts',
      'src/types/douyin.ts',
      'src/utils/douyinLink.ts',
      'src/utils/douyinLink.test.ts',
      'src/utils/platformDetect.ts',
      'src/utils/platformDetect.test.ts',
    ]) {
      expect(existsSync(path), path).toBe(false);
    }

    const app = readFileSync('src/App.vue', 'utf8');
    expect(app).not.toContain('DouyinTool');
    expect(app).not.toContain('useAria2Handoff');
    expect(app).not.toContain("selectedTool.id === 'video-link'");
  });

  it('removes the in-process platform parsers and Tauri commands', () => {
    for (const path of [
      'src-tauri/src/douyin.rs',
      'src-tauri/src/xhs.rs',
      'src-tauri/src/bilibili.rs',
      'src-tauri/src/youtube.rs',
    ]) {
      expect(existsSync(path), path).toBe(false);
    }

    const host = readFileSync('src-tauri/src/lib.rs', 'utf8');
    for (const command of [
      'resolve_douyin_url',
      'extract_douyin_video',
      'extract_xhs_note',
      'extract_bilibili_video',
      'extract_youtube_video',
    ]) {
      expect(host, command).not.toContain(command);
    }
  });
});
