# 发布指南

## 版本一致性

发布前确保 `package.json`、`src-tauri/Cargo.toml` 和 `src-tauri/tauri.conf.json` 使用同一语义化版本，完成本地质量检查并更新 Release Notes。

## 更新签名

Tauri 更新签名密钥与 Windows Authenticode 代码签名是两套独立机制：

- `TAURI_SIGNING_PRIVATE_KEY` 与 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 用于应用内自动更新完整性验证。私钥只存 GitHub Actions Secrets，公钥写入 `tauri.conf.json`。
- Authenticode 用于 Windows“已验证的发布者”和 SmartScreen 信任。正式生产发布应购买 OV/EV 证书，并在 CI 中通过短期凭证或硬件/云签名服务完成签名。

不得轮换 Tauri 更新密钥而不提供迁移策略；已安装客户端只信任内置公钥。

## 发布

1. 合并并确认 `main` 的 CI 通过。
2. 创建带说明的 `vX.Y.Z` 标签。
3. 推送标签，等待 `release.yml` 完成。
4. 检查 Release 同时包含以 `Daoxin-Zhixi_` 开头的 NSIS、MSI、更新签名文件、`latest.json`、`SHA256SUMS.txt` 和 GitHub 构建来源证明。安装后的产品名称仍为“稻芯智析”，ASCII 文件名只用于确保 GitHub 和更新器稳定处理下载资产。
5. 在干净 Windows 虚拟机上验证安装、首次连接、对话、取消、重启恢复、检查更新和卸载。
6. 公布 SHA-256；未启用 Authenticode 时明确披露“未知发布者”提示。

`publish-release-metadata.ps1` 会核对标签与应用版本、安装包数量、签名和已上传资产，并显式生成 Tauri 2 `windows-x86_64` 更新清单。任何一项缺失都会使发布工作流失败，避免产生没有自动更新能力的“绿色”Release。

不要手工替换已经发布的同版本安装包。需要修复时递增补丁版本并发布新标签，以保持更新签名和可追溯性。
