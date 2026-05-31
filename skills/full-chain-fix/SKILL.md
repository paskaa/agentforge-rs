---
name: full-chain-fix
description: Full-chain verification methodology for bugfixes and feature development. When fixing a bug or implementing a requirement, trace the complete data flow (input → save → query → edit → delete → related modules) instead of fixing locally.
metadata:
  short-description: Full-chain data flow verification for bugfixes
---

# 全链路修复原则 ⚠️

> 修 Bug / 做需求时，**不得"就事论事"**，必须走通完整的**数据流全链路**。

## 检查清单（每一环都确认）
1. **录入** → 前端有无输入入口？（弹窗、表格行编辑、表单…）
2. **保存** → 前端 → API → 后端 Controller → Service → Entity → DB，**每一个保存入口**都传了该字段吗？（注意多个 Service 实现类可能走不同入口）
3. **查询** → DB → Mapper XML（注意 UNION ALL 子查询要统一加）→ DTO → 前端展示，列和数据绑定都完整吗？
4. **修改** → 已有数据编辑回显 → 修改再保存 → 数据能正确更新吗？
5. **删除/停止/撤回** → 相关状态变更会丢失该字段数据吗？
6. **关联模块** → 上游（如医嘱录入后护士站要看到备注）和下游（如打印、计费、报表）是否也需要同步修改？

## 常见陷阱
- ❌ 只修了「主入口」的保存逻辑，忘了「批量保存」「签发保存」等其他入口
- ❌ 前端加了输入框，后端 Service 没传字段（不同模块可能走不同 Service 实现类）
- ❌ Mapper XML 是 UNION ALL 查询，只改了其中一个子查询，导致列数不匹配或漏加
- ❌ DTO 层级继承关系没检查（如 `RegAdviceSaveDto extends AdviceSaveDto`，父类改了对不对）
- ❌ 只测了新增，没测编辑已有数据的回显和修改再保存

## 执行细则
- 每个字段的新增/修改，先在脑中画出完整的数据流向图再动手
- 提交前逐个环节检查一遍，确保没有断链
- 编译通过不等同于功能正确，必要时做端到端验证
