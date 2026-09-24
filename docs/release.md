# 发布与打包

PC 端不要求用户本地构建，直接用 GitHub Actions 产物。当前第一版仍以文本同步为主，后续可扩展为更广义的数据互通。

## 自动打包

`.github/workflows/release.yml` 会构建：

- `span-macos-arm64.dmg` / `span-macos-x64.dmg`：标准 macOS 安装镜像，可将 `Span.app` 拖入 Applications
- `span-macos-arm64.tar.gz` / `span-macos-x64.tar.gz`：便携版，只包含 `Span.app`
- `span-windows-x64-setup.exe`：Windows 标准安装器，自动安装后台同步、开始菜单快捷方式，并放行局域网发现与文本同步端口
- `span-windows-x64.zip`：便携版，只包含 `span.exe` 和 `span-gui.exe`
- `span-linux-x64.tar.gz`：只包含 `span` 和 `span-gui`；Linux 当前 GUI 会提示暂不支持
- `span-android-release.apk`：使用固定 Android 发布密钥签名，可覆盖升级

触发方式：

```sh
git tag v0.1.0
git push origin v0.1.0
```

也可以在 GitHub Actions 页面手动点 `workflow_dispatch`。

### Android 固定签名

Android 覆盖升级要求每个版本使用同一把签名密钥。密钥只保存在 GitHub Actions Secrets，不能提交到仓库。首次发布前生成并离线备份 keystore，然后配置：

```text
ANDROID_KEYSTORE_BASE64
ANDROID_KEYSTORE_PASSWORD
ANDROID_KEY_ALIAS
ANDROID_KEY_PASSWORD
ANDROID_SIGNING_CERT_SHA256
```

其中 `ANDROID_KEYSTORE_BASE64` 是 keystore 文件的单行 Base64 内容，`ANDROID_SIGNING_CERT_SHA256` 是发布证书的 64 位十六进制 SHA-256 指纹。Release workflow 会先跑 Android 模拟器双向剪贴板测试，再构建并核对最终 APK 的证书；缺少任一项、指纹不匹配或测试失败时，整个 GitHub Release 和 npm 发布都会停止。tag 会写入 `versionName`，workflow run number 会生成单调递增的 `versionCode`。

`v0.1.2-test.34` 及更早 Android 测试包使用了 CI 临时 debug 证书。首次迁移到固定签名版时必须卸载旧 APK（旧配对和设置会被清除）并重新安装、配对；此后只要 keystore 和证书指纹保持不变，就可以直接覆盖升级。

## 包内容

包内不再塞 README、协议文档等杂项，只保留可运行内容：

- macOS：`Span.app`，双击打开 GUI；内部带 `span` daemon/CLI 与 `span-gui`
- Windows：普通用户优先运行 `span-windows-x64-setup.exe`；安装后从开始菜单打开 Span。zip 仅作为免安装便携版
- Linux：`span` CLI/daemon 与 `span-gui` 占位 GUI

Windows/Linux 仍保留两个二进制，是为了让 Windows GUI 使用无控制台子系统，同时 daemon/CLI 保持标准终端行为；这是当前最小且最稳的拆分。

Windows 安装器会添加两条仅限本地子网的入站规则：`46792/UDP` 用于设备发现，`46793/TCP` 用于加密文本传输；卸载时会自动移除。便携版不会修改系统防火墙，首次运行时需要在 Windows 安全提示中允许专用网络访问。

## 本地验证

```sh
cargo test --workspace
cargo build --release -p span --bins
./target/release/span-gui
./target/release/span --help
```

## 体积目标

当前 release 二进制仍保持在几百 KB 量级。

保持体积小的原则：

- PC 端不使用 Flutter/Electron
- 不内置 WebView
- 不引入数据库
- 不引入异步 runtime，除非确实需要
- GUI 只使用平台原生控件；daemon 保持无窗口，CLI 仅保留调试/脚本入口

## 开源前需要确认

公开仓库前建议补：

- `LICENSE`：建议 MIT 或 Apache-2.0
- `SECURITY.md`：说明当前是否加密、如何报告漏洞
- `CONTRIBUTING.md`：说明如何跑测试和构建
- Release notes：说明当前已加密，但仍建议仅在可信局域网使用

## npm 桌面端安装包

目录：`npm/span-desktop`。

npm 包的职责只有两件事：

1. 安装时识别 `macOS arm64/x64`、`Windows x64` 或 `Linux x64`。
2. 下载对应 GitHub Release 压缩包，并从 `Span.app` 或最小二进制目录中提取 `span` 与 `span-gui`。`span` 命令无参数打开 GUI；`span install/start/stop/...` 调用 CLI。

因此 npm 层不会引入 Electron，实际常驻进程仍然是 Rust daemon。

### 本地打包测试

```sh
cargo build --release -p span --bins
cd npm/span-desktop
npm pack
SPAN_LOCAL_BINARY="$PWD/../../target/release/span" \
SPAN_LOCAL_GUI_BINARY="$PWD/../../target/release/span-gui" \
  npm install --prefix /tmp/span-npm-prefix ./span-desktop-*.tgz
/tmp/span-npm-prefix/node_modules/.bin/span status
```

### npm 发布

首次发布前确认 npm 包名未被占用，并在本地登录。由于 npm 的可写 Granular Access Token 目前最多 90 天，建议只用它完成首次初始化发布，随后切换到 Trusted Publishing（GitHub Actions OIDC）：

```sh
npm login
npm whoami
cd npm/span-desktop
npm version 0.1.2-test.5 --no-git-tag-version
npm publish --access public --tag test
```

发布后用户可以直接：

```sh
npm install -g span-desktop
span install
```

### GitHub Actions 自动发布（Trusted Publishing）

首次发布成功后，在 npm 包 `span-desktop` 的设置中添加 Trusted Publisher：

```text
Provider: GitHub Actions
Owner: mrone0
Repository: rs
Workflow: release.yml
Environment: 留空
```

然后在 GitHub Actions workflow 的 npm 发布 job 中启用：

```yaml
permissions:
  contents: read
  id-token: write
```

当前 `release.yml` 已配置 OIDC，不再需要保存 `NPM_TOKEN`。预发布 tag（例如 `v0.1.2-test.5`）进入 npm `test` 通道；正式 tag（例如 `v0.1.2`）进入 `latest`。

如果暂时不用 Trusted Publishing，也可以使用短期 Token。GitHub 仓库需要添加：

```text
NPM_TOKEN
```

同一版本不能重复发布；例如 `v0.1.2` 对应 `span-desktop@0.1.2`。测试版本对应关系为：`v0.1.2-test.5` → `span-desktop@0.1.2-test.5`。
