# Span Android

Android 端第一版只做**纯文本双向流转**，目标是用最小的系统开销把手机和同一局域网内已信任的 Span PC 连接起来。

## 已支持

- Android 10+（`minSdk 29`）
- 当前剪贴板一键发送
- 系统分享菜单发送选中的文本 / URL
- Quick Settings Tile 一键发送当前剪贴板；启用可靠后台后无需打开 Span 界面
- PC → Android：前台接收服务监听 TCP 46793，Span 界面不在前台时也会解密并写入系统剪贴板
- 可选“可靠后台”系统托管服务：用于被厂商系统清理后恢复接收器，并在切换到目标 App 时重试待写入的剪贴板；不读取界面、不模拟点击
- 开机、快速开机及 APK 覆盖升级后恢复接收服务（用户关闭接收后不会恢复）
- 局域网 UDP 自动发现
- 设备信任 / 撤销
- `X25519 + HKDF-SHA256 + ChaCha20-Poly1305`，与 Rust PC 端协议兼容
- 原生 Android View，无 Compose、无第三方运行时
- 空闲时使用阻塞 socket，不持有 CPU WakeLock 或 Wi‑Fi 高性能锁

## 当前边界

- Android 端现在支持双向文本链路：Android → PC 主动发送，PC → Android 由接收服务写入剪贴板。
- Android 10 以后系统限制后台读取剪贴板。Span 不做绕过系统限制的常驻读取：
  - 打开 Span 后可读取当前剪贴板；
  - 启用可靠后台后，无障碍快捷按钮、Quick Settings Tile 和常驻通知的发送按钮会在用户点击时通过透明的系统托管窗口读取一次并发送，不打开 Span 界面；
  - 未启用可靠后台，或厂商系统不支持透明系统托管窗口时，快捷入口会短暂拉起 Span 再读取并发送；
  - 系统分享菜单是最可靠的选中文本入口。
- PC → Android 接收是独立的前台服务，只接收已信任设备的加密 TCP 文本，不读取手机当前剪贴板。首次设置完成后，不需要先进入 Span；可直接打开微信、浏览器等目标 App 粘贴。若厂商系统拒绝普通后台服务写剪贴板，Span 会立即转交给已启用的系统托管服务重试。
- 华为、小米、三星等厂商可能清理普通前台服务。首次配对后点击 **开启可靠后台**：
  1. 在系统“无障碍”设置中启用 **Span reliable background receiver**；
  2. 返回 Span，再允许忽略电池优化。
  该系统托管服务只订阅前台窗口切换事件，不能读取窗口内容、不会执行手势；用途是保证局域网接收器存活，并在厂商系统延迟剪贴板写入时重试。授权会在重启后保留。华为还建议在“应用启动管理”中允许 Span 自启动和后台活动。

### 各厂商后台设置

不同系统的菜单名称和后台策略差异较大，请按手机品牌查看独立说明：

- [设置总览与验证方法](docs/background-setup/README.md)
- [小米 / Redmi（HyperOS、MIUI）](docs/background-setup/xiaomi-redmi.md)
- [OPPO / 一加 / realme（ColorOS 系）](docs/background-setup/oppo-oneplus-realme.md)
- [华为（HarmonyOS、EMUI）](docs/background-setup/huawei.md)
- [荣耀（MagicOS）](docs/background-setup/honor.md)
- [vivo / iQOO（OriginOS、Funtouch OS）](docs/background-setup/vivo-iqoo.md)
- [三星（One UI）](docs/background-setup/samsung.md)

## 本机编译

需要 Android SDK、JDK 21 和 Android 36 平台：

```sh
cd apps/android
JAVA_HOME=$(/usr/libexec/java_home -v 21) ./gradlew :app:testDebugUnitTest :app:assembleDebug
```

APK：

```text
apps/android/app/build/outputs/apk/debug/app-debug.apk
apps/android/app/build/outputs/apk/release/app-release.apk
```

本机实测体积：debug 约 67K，R8 + 资源压缩后的本地 release 约 40K。本机构建在没有提供发布密钥时使用当前电脑的 Android debug key，仅用于开发验证，不能作为可升级的正式安装包。

GitHub tag 发布使用仓库 Secrets 中的固定 Android keystore，产物名为 `span-android-release.apk`；`versionName` 来自 tag，`versionCode` 随 Release workflow 递增。固定签名是覆盖升级的前提，不能把 keystore 提交到仓库。

`v0.1.2-test.34` 及更早测试包使用的是 CI 临时 debug 证书，因此首次切换到固定签名版不能直接覆盖安装。请先卸载旧 APK（Android 会同时清除旧配对和设置），再安装新的 `span-android-release.apk` 并重新配对；之后的固定签名版本可以正常覆盖升级。

`local.properties` 只用于本机 Android SDK 路径，已加入根目录 `.gitignore`。

## 与 PC 配对

PC 端先启动后台守护进程：

```sh
span start
```

Android 打开 Span 后会自动发现局域网设备。PC 端也可以执行：

```sh
span discover
```

发现 Android 后，PC 端可直接信任已发现设备：

```sh
span trust <android_device_id>
```

Android 端在 Devices 列表中点击 PC 的 **Trust**。手动配对时需要填写：

- Device ID
- Host / IP
- 32 字节 X25519 公钥十六进制字符串

信任关系保存在本地；未信任设备不会收到文本广播。

## GitHub Actions

`.github/workflows/android-apk.yml` 会在 Android 代码变更或手动触发时：

1. 安装 JDK 21 和 Android 36 SDK；
2. 运行 JVM 单元测试；
3. 构建 debug 和 release APK；
4. 上传 `span-android-apks` artifact，其中包含仅供 CI 验证的 debug APK 和本地 debug-key 签名的压缩 APK。

tag Release 的正式 APK 由 `.github/workflows/release.yml` 单独构建；若稳定签名 Secrets 缺失、证书指纹不匹配或 Android 模拟器集成测试失败，整个 Release 都会停止，不再创建缺少 APK 或无法覆盖升级的版本。公开 Release 只附加正式签名 APK，debug APK 仅保留在普通 Android CI artifact 中。

`.github/workflows/android-test.yml` 会分别在 Android 10（API 29）、Android 15（API 35）和 Android 16（API 36）模拟器上运行集成测试。除普通 instrumentation 外，它还会启用真实无障碍服务，在另一个测试 App 保持前台时验证 PC → Android 写入及 Android → PC 发送，并确认整个过程不会打开 Span 界面。
