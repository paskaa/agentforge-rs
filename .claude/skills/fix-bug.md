---
name: fix-bug
description: 修复单个 Bug 的标准流程（BDT 方法论）
---

# Bug 修复流程

1. 获取 Bug 详情: `curl -s https://zentao.gentronhealth.com/api.php/v1/bugs/{id} -H "Token: ..."`
2. 设计测试用例: 在 `tests/e2e/specs/bug-{id}.spec.ts` 创建 Playwright 测试
3. 基线测试: 运行测试确认 Bug 存在
4. 分析根因: 全链路 6 环追踪（前端→Controller→Service→Mapper→DB→关联模块）
5. 修复代码: 一次只修一个 Bug，只动必要文件
6. 编译验证: `cargo check`（Rust）/ `mvn compile`（Java）/ `vue-tsc`（前端）
7. 回归测试: 运行 Playwright 测试确认修复有效
8. 提交: `git commit -m "fix(#{id}): {描述}"`
9. 禅道备注: 写入根因分析 + 修复方案 + 验证结果
