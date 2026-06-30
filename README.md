# LanType

局域网跨端输入工具。手机浏览器作为输入端，通过扫码或 mDNS 发现匹配桌面端，文字实时同步到桌面当前光标位置。

## 用法

1. 在桌面端运行 `lantype`，窗口显示二维码和设备名
2. 手机浏览器打开 `http://<桌面IP>:<端口>`，扫码或选择设备
3. 在手机输入框打字，文字实时注入桌面

## 技术栈

- **桌面端:** Rust + Tauri v2
- **键盘注入:** enigo
- **通信:** WebSocket (tokio-tungstenite)
- **设备发现:** mDNS (`_lantype._tcp`)
- **二维码:** qrcode + image
- **手机端:** 单 HTML 文件

## 构建

```bash
make release          # 当前平台
make release-macos    # macOS universal .app
make release-windows  # Windows GNU .exe
make release-all      # macOS + Windows
```

交叉编译依赖 `zig` 和 `cargo-zigbuild`：

```bash
cargo install cargo-zigbuild
rustup target add x86_64-apple-darwin aarch64-apple-darwin x86_64-pc-windows-gnu
```

产物路径：

- macOS: `target/universal2-apple-darwin/release/LanType.app`
- Windows: `target/x86_64-pc-windows-gnu/release/lantype.exe`
- 当前平台: `target/release/lantype`、`target/release/lantype.exe` 或 `target/release/LanType.app`
