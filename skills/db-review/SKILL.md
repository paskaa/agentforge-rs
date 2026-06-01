---
name: db-review
description: DB审查技能 — 数据库变更验证
when_to_use: 修复涉及SQL/数据库时自动激活
paths:
  - "*.sql"
  - "*mapper*"
  - "*Mapper*"
---

# DB 审查技能

## 审查流程

### 1. 检查变更范围
- 使用 `git diff --name-only` 查看变更文件
- 识别 SQL DDL 变更（.sql、migration）
- 区分 DDL 变更和代码引用（mapper XML）

### 2. 验证 SQL
- 用 db-query 查询真实表结构
- 检查 NOT NULL 约束
- 检查外键约束
- 用 EXPLAIN 验证查询计划

### 3. 审查结论
- 无 DDL 变更 → 直接通过
- 有 DDL 变更 → 检查迁移脚本
- 审查失败 → 退回修复智能体并附原因
