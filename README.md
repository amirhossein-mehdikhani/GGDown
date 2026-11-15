# GGDown - Google Drive Download Manager

A modern, high-performance, cross-platform (Windows, macOS, Linux) Download Manager centered around **Google Drive**, featuring full **Resumable Transfers** (Upload & Download that seamlessly continue after network interruptions).

---

## 🚀 Key Features

1. **Google Drive Cloud File Explorer:**
   * Browse folders and files with breadcrumb navigation.
   * Create new folders, rename, and delete files.
   * Distinct file-type icons (Videos, Audio, Archives, Images, Documents).

2. **Resumable Upload Engine (Google Drive Resumable Protocol):**
   * Implements Google Drive's official `uploadType=resumable` specification.
   * Saves active session URIs. If your connection drops or the app is closed, it queries Google with `Content-Range: bytes */TOTAL` (`308 Resume Incomplete`) to find the exact byte offset received, resuming instantly without re-uploading previous data!
   * 2 MB Rust-powered network chunking with retry backoff for maximum stability and minimal memory usage.

3. **Resumable Download Engine:**
   * Utilizes HTTP `Range: bytes=OFFSET-` headers.
   * Downloads stream to temporary `.part` files and rename to final filenames upon completion.
   * Automatic or manual resume from where it stopped.

4. **Network Watcher & Auto-Reconnect:**
   * Actively monitors operating system connectivity. Automatically resumes interrupted tasks once network connection is restored.

5. **Desktop OAuth 2.0 PKCE Flow:**
   * Integrated local loopback HTTP server for seamless one-click browser authorization and token auto-refresh.

---

## 🛠️ Linux (Ubuntu) Setup & Run

### Prerequisites:
```bash
sudo apt update
sudo apt install -y pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

### Run in Development Mode:
```bash
npm run tauri dev
```

### Build Production Binary (deb / AppImage / exe):
```bash
npm run tauri build
```
Production packages will be generated under `src-tauri/target/release/bundle/`.

