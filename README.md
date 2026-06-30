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

## 配置

支持通过 JSON 文件配置（若不配置，默认随机端口即可开箱使用）：

- **`$HOME/.config/lantype/config.json`** — 全局配置，对所有实例生效
- **`./config.json`** — 本地配置，与全局配置浅合并，本地键值覆盖全局

支持字段：

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `port` | `"auto"` 或 `1–65535` | `"auto"` | 监听端口；`"auto"` 使用随机端口，指定数字则固定端口（绑定失败时自动回退随机端口） |
| `nickname` | `string` 或 `null` | `null` | 自定义设备名；不设置时自动生成随机中文名称并持久化到全局配置 |
| `blocklist` | `[{ip, device_name}]` | `[]` | 已屏蔽设备列表，每个条目包含 IP 和设备名；通过桌面端界面管理 |

示例：

```json
{
  "port": 9876,
  "nickname": "我的桌面"
}
```

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
