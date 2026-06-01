---
name: playwright-test
description: Playwright回归测试技能
when_to_use: 测试Bug修复时自动激活
---

# Playwright 回归测试技能

## 测试流程

### 1. 环境准备
- 确保前端 dev server 运行在 81 端口
- 使用 `--workers=1` 单线程运行

### 2. 执行测试
```bash
cd /root/.openclaw/workspace/his-repo/openhis-ui-vue3
npx playwright test --grep @bug{ID} --reporter=line --workers=1
```

### 3. 结果判定
- 测试通过 → 通知华佗验收
- 测试失败 → 增加重试计数，退回修复智能体
- 超过 3 次 → 通知人工介入

### 4. 结果记录
- 写入禅道备注
- 保存测试文档到 Redis
