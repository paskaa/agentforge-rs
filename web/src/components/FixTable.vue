<template>
  <div>
    <el-table :data="fixes" stripe style="width:100%" max-height="60vh">
      <el-table-column prop="bug" label="Bug" width="100">
        <template #default="{ row }"><span style="font-family:monospace;color:#60a5fa">#{{ row.bug }}</span></template>
      </el-table-column>
      <el-table-column prop="agent" label="智能体" width="120" />
      <el-table-column prop="ok" label="状态" width="100">
        <template #default="{ row }">
          <el-tag :type="row.ok ? 'success' : 'danger'" size="small">{{ row.ok ? '✅ 成功' : '❌ 失败' }}</el-tag>
        </template>
      </el-table-column>
      <el-table-column prop="dur" label="耗时" width="100" />
      <el-table-column prop="ts" label="时间">
        <template #default="{ row }"><span style="color:#64748b;font-size:12px">{{ row.ts }}</span></template>
      </el-table-column>
    </el-table>
    <el-empty v-if="fixes.length === 0" description="今日暂无修复记录" :image-size="60" />
  </div>
</template>

<script setup>
defineProps({ fixes: { type: Array, default: () => [] } })
</script>
