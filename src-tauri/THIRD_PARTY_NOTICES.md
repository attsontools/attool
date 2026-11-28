# AT Tool bundled runtime notices

AT Tool distributes the following independent runtime components. Their licenses apply to those components; AT Tool's own license is unchanged.

| Component | Version/source | License |
|---|---|---|
| Tesseract OCR | 5.5.3 | Apache License 2.0 |
| tessdata_fast models | commit `65727574dfcd264acbb0c3e07860e4e9e9b22185` | Apache License 2.0 |
| yt-dlp | 2026.10.21 | Unlicense |
| yt-dlp-ejs | 0.8.0 | Unlicense; bundled JavaScript retains its ISC/MIT notices |
| QuickJS-ng | 0.17.0 | MIT |
| GStreamer core and plugins | 1.24 ABI, Linux only | GNU LGPL v2.1 or later |
| libvpx | system package used by the Linux VP8 plugin | BSD 3-Clause |

The native libraries used by Tesseract retain their upstream licenses. Their vcpkg copyright files are included in the runtime license directory.

The exact license texts and package copyright files are shipped beside this notice in AT Tool's materialized runtime directory.

Project sources:

- https://github.com/tesseract-ocr/tesseract
- https://github.com/tesseract-ocr/tessdata_fast
- https://github.com/yt-dlp/yt-dlp
- https://github.com/yt-dlp/ejs
- https://github.com/quickjs-ng/quickjs
- https://gitlab.freedesktop.org/gstreamer/gstreamer
- https://chromium.googlesource.com/webm/libvpx
