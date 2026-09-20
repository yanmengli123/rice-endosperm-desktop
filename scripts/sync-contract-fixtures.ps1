# 从服务端权威语料同步 AgentRun 契约 fixtures 到桌面端回放测试目录。
#
# 服务端真源：../Yuxi/backend/test/fixtures/agent_run_contract
#   （服务端测试 test_agent_run_contract_fixtures.py 保证语料与服务端序列化
#     逐字节一致；manifest.json 记录每个文件的 sha256）
# 桌面端副本：src-tauri/tests/fixtures/agent_run_contract
#   （cargo 回放测试消费；manifest 哈希校验保证副本未与服务端漂移）
#
# 两端任何一侧单独改动 fixtures，都会在各自的 manifest 校验测试里红——
# 「协议改动必须两端同步」由测试机器强制，不靠自觉。

param(
    [string]$Source = (Join-Path $PSScriptRoot "..\..\Yuxi\backend\test\fixtures\agent_run_contract")
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path (Join-Path $Source "manifest.json"))) {
    Write-Error "服务端契约语料不存在或缺 manifest.json：$Source（先在 Yuxi 仓库生成/更新语料）"
}

$destination = Join-Path $PSScriptRoot "..\src-tauri\tests\fixtures\agent_run_contract"
New-Item -ItemType Directory -Force -Path $destination | Out-Null

# 先清空副本再复制，避免残留服务端已删除的文件造成"幽灵契约"。
Get-ChildItem $destination -File | Remove-Item -Force
Copy-Item -Path (Join-Path $Source "*") -Destination $destination -Force

$count = (Get-ChildItem $destination -File).Count
Write-Host "已同步 $count 个契约文件到 $destination"
Write-Host "提交前请确认两端（Yuxi 与本仓库）的 fixtures 改动在同一批提交中互相引用。"
