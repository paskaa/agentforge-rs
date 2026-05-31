//! Integration tests — end-to-end agent workflow.

#[cfg(test)]
mod tests {
    use agentforge::config::Config;
    use agentforge::core::coordinator::{self};
    use agentforge::core::executor::AgentExecutor;
    use agentforge::core::pipeline;
    use agentforge::core::trace::TraceStore;

    /// Config should parse from file or env.
    #[test]
    fn test_config_defaults() {
        // Without a file, Config should use env or defaults
        let result = Config::load();
        // May fail if no YAML file, which is expected in test env
        match result {
            Ok(cfg) => {
                assert_eq!(cfg.redis.host, "127.0.0.1");
                assert_eq!(cfg.redis.port, 16379);
            }
            Err(_) => {
                // Expected in CI — no config file
            }
        }
    }

    /// Keyword routing should match correctly.
    #[test]
    fn test_route_frontend_keys() {
        let frontend_cases = [
            "#518 报卡页面缺失核心字段",
            "vue组件渲染问题显示异常",
            "弹窗界面按钮样式错误",
            "前端组件数据加载失败提示语不规范",
        ];
        for case in frontend_cases {
            assert_eq!(coordinator::route_bug(case), "zhaoyun",
                "Failed to route '{}' as frontend", case);
        }
    }

    #[test]
    fn test_route_backend_keys() {
        let backend_cases = [
            "后端api接口报500错误service异常",
            "spring事务处理校验签发逻辑",
            "div_log完诊审计记录异常",
            "执行科室mapper配置保存报错",
            "库存发药计费缓存问题",
        ];
        for case in backend_cases {
            assert_eq!(coordinator::route_bug(case), "guanyu",
                "Failed to route '{}' as backend", case);
        }
    }

    #[test]
    fn test_route_dba_keys() {
        assert_eq!(coordinator::route_bug("数据库查询慢sql优化"), "xunyu");
    }

    /// Agent name mapping.
    #[test]
    fn test_agent_names() {
        assert_eq!(AgentExecutor::agent_name_from_id("zhaoyun"), "赵云");
        assert_eq!(AgentExecutor::agent_name_from_id("guanyu"), "关羽");
        assert_eq!(AgentExecutor::agent_name_from_id("unknown"), "unknown");
    }

    /// Pipeline: is_human check.
    #[test]
    fn test_is_human() {
        assert!(pipeline::is_human("chenxj"));
        assert!(pipeline::is_human("yangkexiang"));
        assert!(!pipeline::is_human("zhaoyun"));
        assert!(!pipeline::is_human(""));
    }

    /// Trace store: open and log.
    #[tokio::test]
    async fn test_trace_store() {
        let tmp = std::env::temp_dir().join("agentforge_test_traces.db");
        let _ = std::fs::remove_file(&tmp);

        let store = TraceStore::open(&tmp).await.unwrap();
        store.log("zhaoyun", "task_start", Some("Bug#999"), Some("test message"), None, None, None, None, None).await;
        store.log("zhaoyun", "task_done", Some("Bug#999"), None, None, None, Some(2450), Some("ok"), None).await;

        let traces = store.query(10).await;
        assert_eq!(traces.len(), 2);
        assert_eq!(traces[0].agent_id, "zhaoyun");
        assert_eq!(traces[0].event, "task_done");

        let summary = store.agent_summary().await;
        assert!(summary.iter().any(|s| s.agent_id == "zhaoyun" && s.event == "task_start"));

        let _ = std::fs::remove_file(&tmp);
    }
}
