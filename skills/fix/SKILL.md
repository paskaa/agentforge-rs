---
name: bug-fix
description: Bug修复技能 — 全链路修复流程
when_to_use: 修复Bug时自动激活
paths:
  - "*.java"
  - "*.vue"
  - "*.ts"
---

# Bug 修复技能

## 修复流程

### 1. 分析阶段
- 读取禅道 Bug 完整信息（标题、描述、附件图片）
- 使用 OCR 读取图片中的错误信息
- 分析 6 环链路：前端→Controller→Service→Mapper→DB→关联模块

### 2. 定位阶段
- 使用 `rg` 搜索相关代码
- 使用 `git blame` 追溯历史
- 确认根因

### 3. 修复阶段
- 一次只修一个 Bug
- 修改最小范围代码
- 遵守项目编码规范

### 4. 验证阶段
- 后端：`mvn compile`
- 前端：`vue-tsc --noEmit`
- 数据库：`db-query` 验证 SQL

### 5. 提交阶段
- `git add` + `git commit`
- commit message 格式：`fix(#bug_id): 简要描述`
- 推送到 develop 分支
