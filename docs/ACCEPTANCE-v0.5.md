# 桌面端 v0.5「会话优先」适配验收清单

> 适用版本：桌面端 v0.5.x ↔ 语析服务端（feat/pdf-evidence-v1 及之后）。
> 验收分三层：**A 网关自动断言**（可机器执行）、**B 真机五连**（人工，按序）、
> **C CI 回归**。三层全绿才算「完全适配」；本文是唯一验收依据。

## A. 网关自动断言（可直接执行）

前提：Yuxi 全栈已启动（`make up`），APISIX 配置改动后**必须重建容器**：

```bash
cd Yuxi
docker compose -f docker-compose.yml -f docker-compose.apisix.yml up -d --force-recreate apisix
```

### A1 契约测试（入库用例，宿主机执行）

```bash
cd Yuxi/backend
docker cp ../docker/apisix/apisix.yaml "$(docker compose ps -q api):/tmp/apisix.yaml"
MSYS_NO_PATHCONV=1 docker compose exec -T \
  -e APISIX_YAML_PATH=/tmp/apisix.yaml \
  -e E2E_GATEWAY_URL=http://yuxi-apisix:9080 \
  api uv run --no-sync --group test pytest test/e2e/test_apisix_gateway_contract.py -q
# 预期：6 passed
```

### A2 实链路探测（经 :9088，全部断言「非 404」）

```bash
# 附件链路：401=到达上游鉴权（正确）；404=网关未放行（失败）
curl -s -o /dev/null -w "%{http_code}\n" -X POST http://127.0.0.1:9088/api/chat/attachments/tmp
# 预期 401

# run-create 两条 anyOf 分支 + 附件 meta 字段（ASCII body；Git Bash 中文会被转码导致上游 400 假阳性）
for body in \
  '{"query":"rice","agent_slug":"default-chatbot","thread_id":"t1","meta":{"request_id":"probe-0123456789abcdef","client":"rice-endosperm-desktop"}}' \
  '{"agent_slug":"default-chatbot","thread_id":"t1","resume":"continue","created_by_run_id":"run-1","meta":{"request_id":"probe-0123456789abcdef","client":"rice-endosperm-desktop"}}' \
  '{"query":"rice","agent_slug":"default-chatbot","thread_id":"t1","meta":{"request_id":"probe-0123456789abcdef","client":"rice-endosperm-desktop","attachment_file_ids":["f1"]}}'
do
  curl -s -o /dev/null -w "%{http_code}\n" -X POST http://127.0.0.1:9088/api/agent/runs \
    -H "Content-Type: application/json" -d "$body"
done
# 预期三次都是 401
```

## B. 真机五连（人工按序执行，装 v0.5.x 桌面端）

| # | 步骤 | 预期 | 失败含义 |
|---|------|------|---------|
| 1 | **激活码登录**：管理员 `POST /api/admin/onboarding/invitations` 开户取 `yxact_` 码 → 桌面端「企业激活码」Tab 兑换 | 进入主界面；`startup.log` 出现 `session_established`；设置页显示「安全会话已配置」 | 兑换响应若含 api_key 会被客户端拒绑（契约违约防线生效） |
| 2 | **附件问答端到端**：聊天输入框添加 PDF → 解析 → 提问 | 全链路成功（网关 A2 已放行；旧版 v0.4.8 也应可用） | 400 = confirm schema 与服务端模型不一致，核对网关闭集 |
| 3 | **并发刷新**：访问令牌临近过期时快速连发多条提问 | 正常回答；服务端日志无 `reuse_detected` | 单飞锁失效，立即回滚该版本 |
| 4 | **远程下线回落**：另一处 `DELETE /api/auth/sessions/{family_id}` 下线本机 → 本机继续提问 | 报「登录会话已失效」并**自动回到连接设置页**（本地会话 blob 已清除，非持续报错循环） | 若卡在报错工作区，检查终态清 blob 逻辑 |
| 5 | **回退验证**：装 v0.5.x 后回装 v0.4.8 | 正常启动，旧历史可见（无编号迁移 = 无不可回退点） | 启动失败说明引入了回退不兼容，禁止发版 |

补充抽查（可选）：
- 设备码登录全流程：浏览器授权页核对确认码 → 批准 → 本机进入主界面；
  服务端确认过渡 Key 已被撤销（`GET /api/user/apikey` 列表，或兑换后 90 天 Key 不存在）。
- 升级认领：v0.4.8 有历史 → 升 v0.5 → 侧栏历史仍在（单账号机器自动认领）；
  多账号机器出现「把旧版本历史归入当前账号」按钮且默认不动。
- 用量与配额：设置页「用量与配额」能读出策略/配额/近 14 天用量。

## C. CI 回归（合并前必须）

1. GitHub Actions（windows runner，MSVC 工具链）跑绿：
   ```bash
   cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
   cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
   cargo test --manifest-path src-tauri/Cargo.toml
   ```
   重点用例（本次新增，本机开发环境因 MSVC 缺失无法运行、CI 必须跑绿）：
   - `commands::tests::concurrent_bearer_refresh_rotates_exactly_once`——并发 8 路
     `ensure_active_bearer` 断言刷新端点**只被调用一次**（单飞锁安全验收）；
   - `database::migration_tests::startup_claim_*`——legacy 认领的幂等、
     多账号跳过、无权威账号 no-op 三种判定。
2. 前端：`pnpm check`（tsc + vite build + vitest，当前 64 用例全绿）。

## 已知边界（验收时不算失败）

- 三要素存量用户仍走静态 Key（设计决策：不强制重登）；升级到会话需重新登录一次。
- 桌面端 resume 续跑仅支持文本批复；结构化审批载荷（LangGraph Command）暂不支持。
- 423 登录锁定透传服务端文案，未解析 `X-Lock-Remaining` 倒计时。
- OIDC 为间接支持（设备码浏览器侧 SSO），无原生桌面 SSO。
