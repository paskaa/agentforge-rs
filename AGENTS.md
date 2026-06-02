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


### 7. 状态值一致性（铁律 — 来自 Bug #574 教训）
- **修改任何状态值前，必须先列出完整的状态流转链路**
- **检查项**：
  1. 枚举定义（如 `SlotStatus`）的值是否正确
  2. Service 层设置的状态值是否与枚举一致
  3. 查询/列表接口的状态映射是否覆盖所有枚举值
  4. 前端 `STATUS_CLASS_MAP` 是否包含新状态
  5. 前端过滤条件（如 `v-if`）是否兼容新状态
  6. 池/统计表的聚合 SQL 是否包含新状态值
- **禁止**：只改一端不检查其他端

### 8. 禁止删除源文件（铁律 — 来自 Bug #574 教训）
- **绝对禁止**删除项目中已有的 Java/Vue/SQL 源文件
- 如果文件有编译错误 → 修复错误，不删除文件
- 如果文件是重复的 → 重构合并，不删除文件
- 如果文件是 AI 幻觉创建的 → 检查 git baseline 确认后再删除
- **唯一例外**：明确由人类确认删除的文件

### 9. 全链路验证（铁律 — 来自 Bug #574 教训）
- 修复涉及状态流转的 Bug 后，必须按以下顺序验证：
  ```
  ① 数据库：确认状态值已正确写入
  ② 后端接口：确认返回的状态映射正确
  ③ 前端显示：确认页面显示正确状态文本
  ④ 前端交互：确认按钮/操作基于正确状态启用/禁用
  ⑤ 统计数据：确认池/报表统计包含新状态
  ```
- **禁止**：只验证编译通过就认为修复完成

### 10. 禁止修改已有公开方法签名（铁律）
- 不能删除或重命名已有的 public 方法
- 不能修改已有方法的参数列表
- 需要新增功能时 → 添加新的重载方法
- 需要改行为时 → 修改方法内部实现，不改签名
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
- **Bug #574**：`checkInTicket()` 把状态设为 BOOKED(1) 而非 CHECKED_IN(3)，前端映射缺失，池统计漏计 → 必须走完整状态链路验证
- **Bug #574**：AI 智能体看到编译错误直接删文件，没有检查文件是否在 baseline 中 → 必须先确认文件来源再操作
- **Bug #574**：多次"fallback修复"改的是 OrderServiceImpl，完全没触及 TicketServiceImpl 中的签到逻辑 → 必须用 rg 搜索所有相关代码路径
