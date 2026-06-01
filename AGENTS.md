# AgentForge-RS — 智能体 Bug 修复框架

> 模型决定上限，Harness 决定底线。

## 架构概览

```
刘备(协调) → 诸葛亮(分析) → {关羽|赵云}(修复) → 荀彧(DB审查) → 张飞(测试) → 华佗(验收) → 陈琳(归档)
```

## 智能体定义

每个智能体有独立的 YAML 定义文件：
- `agents/liubei.md` — 总协调者
- `agents/zhugeliang.md` — 分析师
- `agents/guanyu.md` — 后端修复师
- `agents/zhaoyun.md` — 前端修复师
- `agents/xunyu.md` — DB审查师
- `agents/zhangfei.md` — 测试师
- `agents/huatuo.md` — 验收师
- `agents/chenlin.md` — 归档师

## 技能系统

每个阶段有独立的技能文件：
- `skills/fix/SKILL.md` — 修复技能
- `skills/test/SKILL.md` — 测试技能
- `skills/verify/SKILL.md` — 验收技能
- `skills/archive/SKILL.md` — 归档技能
- `skills/db-review/SKILL.md` — DB审查技能
- `skills/analyze/SKILL.md` — 分析技能

## 铁律（不可违反）

### 1. Bug 状态管理
- 人类提的 Bug：只加备注，不改状态，不改分配
- 智能体提的 Bug：可以改分配和加备注
- 已关闭/已解决的 Bug 不再处理

### 2. 修复流程
- 一次只修一个 Bug，不扩大范围
- 修复前必须读 AGENTS.md
- 修复后必须验证编译
- 涉及 SQL 必须先查真实数据库

### 3. 测试流程
- Playwright 必须 `--workers=1`
- 超时 120 秒
- 最多重试 3 次
- 测试结果写入禅道备注

### 4. 归档流程
- 三重归档：Git + SQLite + Redis
- SQLite 必须使用完整字段
- 禅道备注格式：[📝 陈琳归档] Bug #xxx

### 5. 禅道交互
- 备注使用 resolve+activate workaround
- 不直接调用 comment API（会 404）
- 图片附件必须 OCR 读取

### 6. 质量门禁
- L1: 编译通过
- L2: 测试通过
- L3: DB审查通过
- L4: 验收通过
- L5: 归档完成

## 全链路 6 环分析

涉及数据库字段的 Bug 必须走完整链路：
```
前端/页面 → Controller → Service → Mapper → DB/SQL → 关联模块
 ①录入      ②验证      ③业务     ④持久化    ⑤存储     ⑥联动
```

## 工具链

| 工具 | 用途 |
|------|------|
| `db-query` | 查询真实数据库 |
| `zentao` CLI | 禅道 API 交互 |
| `rg` | 代码搜索 |
| `git blame` | 历史追溯 |
| `playwright` | E2E 测试 |
| `mvn compile` | Java 编译验证 |
| `vue-tsc` | TypeScript 类型检查 |

## 过往教训

- `bug_reports` 表缺少列会导致 INSERT 静默失败
- 禅道 comment API 不存在，必须用 resolve+activate
- SQLite WAL 模式下多进程并发写需要 checkpoint
- UTF-8 多字节字符不能用 byte index 切片
