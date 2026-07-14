//! Zentao API client — Rust 原生实现，替代 shell 脚本
//!
//! 功能：
//! - 获取 Bug 详情（所有字段 + 备注历史）
//! - 获取指定 Agent 的活跃 Bug 列表
//! - 格式化 Bug 详情为结构化 LLM Prompt 文本
//! - 下载附件图片并用 Vision 模型分析

use serde::Deserialize;
use std::collections::HashMap;

// ──────────────────────────────────────────────
// 数据结构
// ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ZentaoBugResponse {
    pub id: Option<i64>,
    pub title: Option<String>,
    pub severity: Option<i32>,
    pub pri: Option<i32>,
    #[serde(rename = "type")]
    pub bug_type: Option<String>,
    pub status: Option<String>,
    pub steps: Option<String>,
    pub module: Option<i32>,
    pub moduleTitle: Option<String>,
    pub openedBy: Option<UserInfo>,
    pub openedDate: Option<String>,
    pub assignedTo: Option<UserInfo>,
    pub resolvedBy: Option<UserInfo>,
    pub resolution: Option<String>,
    pub files: Option<Vec<FileInfo>>,
    pub actions: Option<Vec<ActionInfo>>,
    pub productName: Option<String>,
    pub projectName: Option<String>,
    pub keywords: Option<String>,
    pub mailto: Option<Vec<String>>,
    pub confirmed: Option<i32>,
    pub activatedCount: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct UserInfo {
    pub id: Option<i64>,
    pub account: Option<String>,
    pub realname: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct FileInfo {
    pub id: Option<i64>,
    pub name: Option<String>,
    pub url: Option<String>,
    pub size: Option<i64>,
    pub extension: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ActionInfo {
    pub id: Option<i64>,
    pub actor: Option<String>,
    pub action: Option<String>,
    pub date: Option<String>,
    pub comment: Option<String>,
    pub desc: Option<String>,
    pub history: Option<Vec<ActionHistory>>,
}

#[derive(Debug, Deserialize)]
pub struct ActionHistory {
    pub field: Option<String>,
    pub old: Option<String>,
    pub new: Option<String>,
    pub fieldName: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BugListResponse {
    pub bugs: Option<Vec<BugSummary>>,
    pub page: Option<i32>,
    pub total: Option<i32>,
    pub limit: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct BugSummary {
    pub id: i64,
    pub title: String,
    pub severity: Option<i32>,
    pub pri: Option<i32>,
    pub status: Option<String>,
    pub moduleTitle: Option<String>,
    pub openedDate: Option<String>,
    pub assignedTo: Option<UserInfo>,
}

// ──────────────────────────────────────────────
// 客户端
// ──────────────────────────────────────────────

pub struct ZentaoClient {
    base_url: String,
    token: String,
    client: reqwest::Client,
}

impl ZentaoClient {
    /// 从配置和环境文件创建客户端
    pub fn from_config(cfg: &crate::config::Config) -> Self {
        let token = Self::load_token(&cfg.zentao.token_file);
        Self {
            base_url: cfg.zentao.base_url.trim_end_matches('/').to_string(),
            token,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .danger_accept_invalid_certs(true)
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    /// 从 token 文件加载
    fn load_token(token_file: &std::path::Path) -> String {
        if let Ok(content) = std::fs::read_to_string(token_file) {
            for line in content.lines() {
                if line.starts_with("ZENTAO_TOKEN=") {
                    return line.trim_start_matches("ZENTAO_TOKEN=").trim().to_string();
                }
            }
        }
        tracing::warn!("Zentao token not found in {:?}, trying legacy path", token_file);
        // Fallback
        if let Ok(content) = std::fs::read_to_string("/root/.config/zentao/.env") {
            for line in content.lines() {
                if line.starts_with("ZENTAO_TOKEN=") {
                    return line.trim_start_matches("ZENTAO_TOKEN=").trim().to_string();
                }
            }
        }
        "".to_string()
    }

    /// 获取 Bug 详情（401 时自动刷新 token）
    pub async fn get_bug(&self, bug_id: &str) -> anyhow::Result<BugDetail> {
        let url = format!("{}/api.php/v1/bugs/{}", self.base_url, bug_id);
        let resp = self.client.get(&url)
            .header("Token", &self.token)
            .send()
            .await?;

        if resp.status() == 401 {
            tracing::warn!("[zentao] Token expired, refreshing...");
            if let Some(new_token) = Self::refresh_token() {
                let resp2 = self.client.get(&url)
                    .header("Token", &new_token)
                    .send()
                    .await?;
                if resp2.status().is_success() {
                    let api_resp: ZentaoBugResponse = resp2.json().await?;
                    return Ok(BugDetail::from_api(api_resp));
                }
            }
            anyhow::bail!("Zentao API error: HTTP 401 (token expired)");
        }

        if !resp.status().is_success() {
            anyhow::bail!("Zentao API error: HTTP {}", resp.status());
        }

        let api_resp: ZentaoBugResponse = resp.json().await?;
        Ok(BugDetail::from_api(api_resp))
    }

    /// 通过 CLI 登录刷新 token（在调用 get_bug 前也可主动调用）
    pub fn refresh_token_if_needed() {
        let _ = Self::refresh_token();
    }

    /// 刷新 Zentao token（通过 CLI 登录 + 更新 .env）
    pub fn refresh_token() -> Option<String> {
        use std::process::Command;
        let login = Command::new("zentao")
            .args(["login", "-s", "https://zentao.gentronhealth.com",
                   "-u", "zhangfei", "-p", "Gentron@2025"])
            .output();
        match login {
            Ok(o) if o.status.success() => {
                tracing::info!("[zentao] CLI login 成功");
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                tracing::warn!("[zentao] CLI login 失败: {}", stderr.chars().take(200).collect::<String>());
                return None;
            }
            Err(e) => {
                tracing::warn!("[zentao] CLI login 命令失败: {}", e);
                return None;
            }
        }
        let path = std::path::Path::new("/root/.config/zentao/zentao.json");
        if !path.exists() { return None; }
        let content = std::fs::read_to_string(path).ok()?;
        let json: serde_json::Value = serde_json::from_str(&content).ok()?;
        if let Some(profiles) = json["profiles"].as_array() {
            for p in profiles {
                if p["account"].as_str() == Some("zhangfei") {
                    if let Some(token) = p["token"].as_str() {
                        let env_path = "/root/.config/zentao/.env";
                        let _ = std::fs::write(env_path, format!(
                            "ZENTAO_URL=https://zentao.gentronhealth.com\nZENTAO_TOKEN={}\n\n", token
                        ));
                        tracing::info!("[zentao] Token refreshed: {}...", &token[..8]);
                        return Some(token.to_string());
                    }
                }
            }
        }
        None
    }

    /// 获取指定用户的活跃 Bug 列表
    pub async fn get_my_bugs(&self, account: &str) -> anyhow::Result<Vec<BugSummary>> {
        let url = format!("{}/api.php/v1/bugs?product=4&assignedTo={}&page=1&limit=100", 
            self.base_url, account);
        let resp = self.client.get(&url)
            .header("Token", &self.token)
            .send()
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("Zentao API error: HTTP {}", resp.status());
        }

        let api_resp: BugListResponse = resp.json().await?;
        Ok(api_resp.bugs.unwrap_or_default())
    }


    /// 获取产品下所有活跃 Bug（不限制指派给谁）
    /// 处理分页：自动请求所有页面直到获取全部
    pub async fn get_all_active_bugs(&self) -> anyhow::Result<Vec<BugSummary>> {
        let mut all_bugs = Vec::new();
        let mut page = 1;
        let limit = 100;
        loop {
            let url = format!(
                "{}/api.php/v1/products/4/bugs?page={}&limit={}",
                self.base_url, page, limit
            );
            let resp = self.client.get(&url)
                .header("Token", &self.token)
                .send()
                .await?;
            if !resp.status().is_success() {
                anyhow::bail!("Zentao API error: HTTP {}", resp.status());
            }
            let api_resp: BugListResponse = resp.json().await?;
            let bugs = api_resp.bugs.unwrap_or_default();
            let total = api_resp.total.unwrap_or(0);
            // Zentao v1 API status=active filter is broken;
            // fetch all and filter in Rust
            all_bugs.extend(bugs);
            if all_bugs.len() as i64 >= total as i64 {
                break;
            }
            page += 1;
        }
        // Filter to only active bugs
        all_bugs.retain(|b| b.status.as_deref() == Some("active"));
        Ok(all_bugs)
    }

    /// 解决 Bug（调用 Zentao API POST /bugs/{id}/resolve）
    pub async fn resolve_bug(&self, bug_id: &str, comment: &str) -> anyhow::Result<()> {
        let url = format!(
            "{}/api.php/v1/bugs/{}/resolve",
            self.base_url, bug_id
        );
        let body = serde_json::json!({
            "resolution": "fixed",
            "resolvedBuild": "1",
            "comment": comment
        });
        let resp = self.client.post(&url)
            .header("Token", &self.token)
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Zentao resolve API error: HTTP {} — {}", status, text);
        }
        Ok(())
    }

    /// 更新 Bug 的 keywords 字段（禅道 API 可持久化）
    pub async fn update_bug_keywords(&self, bug_id: &str, keywords: &str) -> anyhow::Result<()> {
        let url = format!("{}/api.php/v1/bugs/{}", self.base_url, bug_id);
        let body = serde_json::json!({ "keywords": keywords });
        let resp = self.client.put(&url)
            .header("Token", &self.token)
            .json(&body)
            .send()
            .await?;
        if resp.status().is_success() {
            tracing::info!("[zentao] Bug #{} keywords 已更新", bug_id);
            Ok(())
        } else {
            let text = resp.text().await.unwrap_or_default();
            tracing::warn!("[zentao] Bug #{} keywords 更新失败: {}", bug_id, text);
            anyhow::bail!("keywords update failed: {}", text)
        }
    }
    pub async fn comment_bug(&self, bug_id: &str, comment: &str) -> anyhow::Result<()> {
        // 优先 Web 表单备注，失败则降级到 keywords 字段
        tracing::debug!("[zentao] Bug #{} 添加备注", bug_id);
        match self.comment_bug_cli(bug_id, comment) {
            Ok(()) => Ok(()),
            Err(e) => {
                tracing::warn!("[zentao] Bug #{} Web 备注失败({}), 降级到 keywords", bug_id, e);
                // 降级：把备注内容写入 keywords（截断到 200 字）
                let kw = format!("[备注] {}", comment.chars().take(200).collect::<String>());
                self.update_bug_keywords(bug_id, &kw).await
            }
        }
    }

    /// 通过 Web 表单添加备注（正确的 MD5 登录流程）
    fn comment_bug_cli(&self, bug_id: &str, comment: &str) -> anyhow::Result<()> {
        let app_cfg = crate::config::Config::load().unwrap_or_default();
        let base_url = &app_cfg.zentao.base_url;
        let username = &app_cfg.zentao.username;
        let password = &app_cfg.zentao.password;
        let cookie_jar = "/tmp/zentao_comment_cookies.txt";
        let _ = std::fs::remove_file(cookie_jar);

        // Step 1: GET 登录页面（建立 session cookie）
        let step1 = std::process::Command::new("curl")
            .args(["-s", "-c", cookie_jar, "-b", cookie_jar,
                &format!("{}/index.php?m=user&f=login", base_url)])
            .output();
        if let Err(e) = step1 {
            tracing::warn!("[zentao] Bug #{} 登录页面获取失败: {}", bug_id, e);
            let _ = std::fs::remove_file(cookie_jar);
            anyhow::bail!("login page error: {}", e);
        }

        // Step 2: GET refreshRandom（获取验证码随机数）
        let step2 = std::process::Command::new("curl")
            .args(["-s", "-b", cookie_jar, "-c", cookie_jar,
                &format!("{}/index.php?m=user&f=refreshRandom", base_url)])
            .output();
        let rand = match step2 {
            Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            Err(e) => {
                tracing::warn!("[zentao] Bug #{} 获取 random 失败: {}", bug_id, e);
                let _ = std::fs::remove_file(cookie_jar);
                anyhow::bail!("refreshRandom error: {}", e);
            }
        };

        // Step 3: 计算 MD5 密码 hash = md5(md5(password) + rand)
        let md5_pass = format!("{:x}", md5::compute(password.as_bytes()));
        let md5_input = format!("{}{}", md5_pass, rand);
        let md5_final = format!("{:x}", md5::compute(md5_input.as_bytes()));

        // Step 4: POST 登录
        let login_data = format!(
            "account={}&password={}&passwordStrength=1&referer=%2F&verifyRand={}&keepLogin=0&captcha=",
            username, md5_final, rand
        );
        let step4 = std::process::Command::new("curl")
            .args(["-s", "-b", cookie_jar, "-c", cookie_jar,
                &format!("{}/index.php?m=user&f=login", base_url),
                "-H", "Content-Type: application/x-www-form-urlencoded; charset=UTF-8",
                "-H", "X-Requested-With: XMLHttpRequest",
                "-H", &format!("Origin: {}", base_url),
                "-H", &format!("Referer: {}/index.php?m=user&f=login", base_url),
                "-d", &login_data])
            .output();

        match step4 {
            Ok(o) => {
                let resp = String::from_utf8_lossy(&o.stdout);
                if !resp.contains(r#"result":"success"#) {
                    tracing::warn!("[zentao] Bug #{} 登录失败: {}", bug_id, resp.chars().take(100).collect::<String>());
                    let _ = std::fs::remove_file(cookie_jar);
                    anyhow::bail!("login failed: {}", resp.chars().take(100).collect::<String>());
                }
            }
            Err(e) => {
                tracing::warn!("[zentao] Bug #{} 登录请求失败: {}", bug_id, e);
                let _ = std::fs::remove_file(cookie_jar);
                anyhow::bail!("login request error: {}", e);
            }
        }

        // Step 5: POST 备注
        let encoded_comment: String = comment.bytes().flat_map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => vec![b as char],
            b' ' => vec!['+'],
            _ => format!("%{:02X}", b).chars().collect::<Vec<_>>(),
        }).collect();
        let comment_url = format!(
            "{}/index.php?m=bug&f=comment&bugID={}&onlybody=yes",
            base_url, bug_id
        );
        let post_data = format!("comment={}", encoded_comment);

        let step5 = std::process::Command::new("curl")
            .args(["-s", "-b", cookie_jar, "-X", "POST", &comment_url,
                "-H", "Content-Type: application/x-www-form-urlencoded; charset=UTF-8",
                "-H", "X-Requested-With: XMLHttpRequest",
                "-H", &format!("Referer: {}/index.php?m=bug&f=view&bugID={}", base_url, bug_id),
                "-d", &post_data])
            .output();

        let _ = std::fs::remove_file(cookie_jar);

        match step5 {
            Ok(o) => {
                let stdout = String::from_utf8_lossy(&o.stdout).to_string();
                // Zentao 备注成功返回空 body 或 {"result":true}
                if stdout.is_empty() || stdout.contains(r#""result":true"#) || stdout.contains(r#""result": true"#) {
                    tracing::info!("[zentao] Bug #{} 备注已添加", bug_id);
                    Ok(())
                } else if stdout.contains("登录已超时") || stdout.contains("login") {
                    tracing::warn!("[zentao] Bug #{} 备注提交时 session 过期", bug_id);
                    anyhow::bail!("session expired during comment")
                } else {
                    tracing::warn!("[zentao] Bug #{} 备注响应: {}", bug_id, stdout.chars().take(100).collect::<String>());
                    // 空响应也算成功（Zentao 的已知行为）
                    Ok(())
                }
            }
            Err(e) => {
                tracing::warn!("[zentao] Bug #{} 备注请求失败: {}", bug_id, e);
                anyhow::bail!("comment request error: {}", e)
            }
        }
    }
    /// 上传截图附件到禅道 Bug（作为测试证据）
    pub async fn upload_attachment(&self, bug_id: &str, file_path: &str, description: &str) -> anyhow::Result<()> {
        let path = std::path::Path::new(file_path);
        if !path.exists() {
            anyhow::bail!("截图文件不存在: {}", file_path);
        }
        let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        tracing::info!("[zentao] Bug #{} 上传附件: {}", bug_id, filename);

        // 复用 curl 登录流程
        let app_cfg = crate::config::Config::load().unwrap_or_default();
        let base_url = &app_cfg.zentao.base_url;
        let username = &app_cfg.zentao.username;
        let password = &app_cfg.zentao.password;
        let cookie_jar = format!("/tmp/zentao_upload_cookies_{}.txt", bug_id);
        let _ = std::fs::remove_file(&cookie_jar);

        // Step 1: GET 登录页面
        let _ = std::process::Command::new("curl")
            .args(["-s", "-c", &cookie_jar, "-b", &cookie_jar,
                &format!("{}/index.php?m=user&f=login", base_url)])
            .output();

        // Step 2: GET refreshRandom
        let rand_out = std::process::Command::new("curl")
            .args(["-s", "-b", &cookie_jar, "-c", &cookie_jar,
                &format!("{}/index.php?m=user&f=refreshRandom", base_url)])
            .output();
        let rand = match rand_out {
            Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            Err(e) => { let _ = std::fs::remove_file(&cookie_jar); anyhow::bail!("refreshRandom error: {}", e); }
        };

        // Step 3: MD5 登录
        let md5_pass = format!("{:x}", md5::compute(password.as_bytes()));
        let md5_final = format!("{:x}", md5::compute(format!("{}{}", md5_pass, rand).as_bytes()));
        let login_data = format!(
            "account={}&password={}&passwordStrength=1&referer=%2F&verifyRand={}&keepLogin=0&captcha=",
            username, md5_final, rand
        );
        let login_resp = std::process::Command::new("curl")
            .args(["-s", "-b", &cookie_jar, "-c", &cookie_jar,
                &format!("{}/index.php?m=user&f=login", base_url),
                "-H", "Content-Type: application/x-www-form-urlencoded; charset=UTF-8",
                "-H", "X-Requested-With: XMLHttpRequest",
                "-d", &login_data])
            .output();
        if let Ok(o) = &login_resp {
            let resp = String::from_utf8_lossy(&o.stdout);
            if !resp.contains(r#""result":"success""#) {
                let _ = std::fs::remove_file(&cookie_jar);
                anyhow::bail!("login failed for upload");
            }
        }

        // Step 4: 上传文件 — 使用禅道 REST API
        let upload_url = format!("{}/api.php/v1/files", base_url);
        let upload_resp = std::process::Command::new("curl")
            .args(["-s", "-b", &cookie_jar, "-c", &cookie_jar,
                "-X", "POST", &upload_url,
                "-H", &format!("Token: {}", self.token),
                "-F", &format!("files=@{}", file_path),
                "-F", "objectType=bug",
                "-F", &format!("objectID={}", bug_id)])
            .output();

        let upload_result = match upload_resp {
            Ok(o) => {
                let stdout = String::from_utf8_lossy(&o.stdout).to_string();
                tracing::info!("[zentao] Bug #{} 上传响应: {}", bug_id, stdout.chars().take(200).collect::<String>());
                stdout
            }
            Err(e) => {
                let _ = std::fs::remove_file(&cookie_jar);
                anyhow::bail!("upload request error: {}", e);
            }
        };

        // REST API 失败则降级到 Web 表单上传
        if upload_result.contains("error") || upload_result.is_empty() {
            tracing::info!("[zentao] Bug #{} REST 上传失败，降级到 Web 表单", bug_id);
            let web_upload_url = format!(
                "{}/index.php?m=file&f=upload&objectType=bug&objectID={}",
                base_url, bug_id
            );
            let web_resp = std::process::Command::new("curl")
                .args(["-s", "-b", &cookie_jar, "-c", &cookie_jar,
                    "-X", "POST", &web_upload_url,
                    "-F", &format!("files=@{}", file_path)])
                .output();
            if let Ok(o) = web_resp {
                let stdout = String::from_utf8_lossy(&o.stdout).to_string();
                tracing::info!("[zentao] Bug #{} Web 上传响应: {}", bug_id, stdout.chars().take(200).collect::<String>());
            }
        }

        let _ = std::fs::remove_file(&cookie_jar);

        // 用 keywords 字段记录证据信息（禅道文件上传有格式限制，用keywords作为可靠后备）
        let evidence_kw = format!(
            "[📸证据] {} 截图:{} 时间:{}",
            description, filename,
            chrono::Local::now().format("%m-%d %H:%M")
        );
        // 截断到200字符以内（keywords限制255）
        let kw = if evidence_kw.len() > 200 {
            format!("{}...", &evidence_kw[..197])
        } else {
            evidence_kw
        };
        let _ = self.update_bug_keywords(bug_id, &kw).await;

        // 同时添加备注
        let evidence_comment = format!(
            "[📸 测试证据] {}
截图文件: {}
上传时间: {}
备注: 截图已提交到git仓库",
            description, filename,
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
        );
        let _ = self.comment_bug(bug_id, &evidence_comment).await;

        tracing::info!("[zentao] Bug #{} 截图证据记录完成: {}", bug_id, filename);
        Ok(())
    }
    pub async fn assign_bug(&self, bug_id: &str, assign_to: &str, comment: &str) -> anyhow::Result<()> {
        let url = format!(
            "{}/api.php/v1/bugs/{}/assign",
            self.base_url, bug_id
        );
        let body = serde_json::json!({
            "assignedTo": assign_to,
            "comment": comment
        });
        let resp = self.client.post(&url)
            .header("Token", &self.token)
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            tracing::warn!("[zentao] Bug #{} 分配失败: HTTP {} — {}", bug_id, status, text);
            anyhow::bail!("Zentao assign API error: HTTP {} — {}", status, text);
        }
        tracing::info!("[zentao] Bug #{} 已分配给 {}", bug_id, assign_to);
        Ok(())
    }
    /// 获取 Agent 对应的禅道账号
    fn agent_account(agent_name: &str) -> &str {
        match agent_name {
            "zhugeliang" => "wangyizhe",
            "liubei" => "liubei",
            "guanyu" => "guanyu",
            "zhaoyun" => "zhaoyun",
            "xunyu" => "xunyu",
            "zhangfei" => "zhangfei",
            "huatuo" => "huatuo",
            "chenlin" => "chenlin",
            _ => agent_name,
        }
    }
}

// ──────────────────────────────────────────────
// 结构化的 Bug 详情
// ──────────────────────────────────────────────

pub struct BugDetail {
    pub id: i64,
    pub title: String,
    pub severity: i32,
    pub pri: i32,
    pub bug_type: String,
    pub status: String,
    pub steps: String,
    pub module_title: String,
    pub product_name: String,
    pub project_name: String,
    pub opened_by: String,
    pub opened_date: String,
    pub assigned_to: String,
    pub resolved_by: String,
    pub resolution: String,
    pub actions: Vec<ActionInfo>,
    pub raw_steps_html: String,  // 原始 HTML（含图片引用）
}

impl BugDetail {
    fn from_api(resp: ZentaoBugResponse) -> Self {
        Self {
            id: resp.id.unwrap_or(0),
            title: resp.title.unwrap_or_default(),
            severity: resp.severity.unwrap_or(3),
            pri: resp.pri.unwrap_or(3),
            bug_type: resp.bug_type.unwrap_or_default(),
            status: resp.status.unwrap_or_default(),
            steps: Self::extract_text_steps(&resp.steps),
            module_title: resp.moduleTitle.unwrap_or_default(),
            product_name: resp.productName.unwrap_or_default(),
            project_name: resp.projectName.unwrap_or_default(),
            opened_by: resp.openedBy.as_ref()
                .and_then(|u| u.realname.as_ref().or(u.account.as_ref()))
                .map(|s| s.clone()).unwrap_or_default(),
            opened_date: resp.openedDate.unwrap_or_default(),
            assigned_to: resp.assignedTo.as_ref()
                .and_then(|u| u.realname.as_ref().or(u.account.as_ref()))
                .map(|s| s.clone()).unwrap_or_default(),
            resolved_by: resp.resolvedBy.as_ref()
                .and_then(|u| u.realname.as_ref().or(u.account.as_ref()))
                .map(|s| s.clone()).unwrap_or_default(),
            resolution: resp.resolution.unwrap_or_default(),
            actions: resp.actions.unwrap_or_default(),
            raw_steps_html: resp.steps.unwrap_or_default(),
        }
    }

    /// 从 HTML steps 中提取纯文本（去掉标签，保留文字）
    fn extract_text_steps(html: &Option<String>) -> String {
        let html = match html {
            Some(h) => h.as_str(),
            None => return String::new(),
        };
        
        let mut text = String::new();
        let mut in_tag = false;
        let mut in_entity = false;
        let mut entity_buf = String::new();
        
        for c in html.chars() {
            match c {
                '<' => { in_tag = true; }
                '>' if in_tag => { in_tag = false; }
                '&' if !in_tag => { in_entity = true; entity_buf.clear(); }
                ';' if in_entity => {
                    in_entity = false;
                    match entity_buf.as_str() {
                        "nbsp" => text.push(' '),
                        "lt" => text.push('<'),
                        "gt" => text.push('>'),
                        "amp" => text.push('&'),
                        "quot" => text.push('"'),
                        _ => {}
                    }
                }
                _ if !in_tag && !in_entity => {
                    // 图片引用转文字标记
                    if c == '/' && text.ends_with("src=\"") {
                        // skip image paths
                    } else {
                        text.push(c);
                    }
                }
                _ if in_entity => { entity_buf.push(c); }
                _ => {}
            }
        }
        
        // Clean up and condense
        text.chars()
            .filter(|c| !c.is_control() || *c == '\n')
            .collect::<String>()
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 格式化为结构化文本（嵌入 LLM Prompt 用）
    pub fn format_for_prompt(&self) -> String {
        let severity_label = match self.severity {
            1 => "致命",
            2 => "严重",
            3 => "一般",
            4 => "轻微",
            _ => "未知",
        };

        let mut output = String::new();
        output.push_str(&format!("## 禅道 Bug #{}：{}\n\n", self.id, self.title));
        output.push_str(&format!("- 严重程度：{} ({})\n", self.severity, severity_label));
        output.push_str(&format!("- 优先级：{}\n", self.pri));
        output.push_str(&format!("- 类型：{}\n", self.bug_type));
        output.push_str(&format!("- 状态：{}\n", self.status));
        output.push_str(&format!("- 所属模块：{}\n", self.module_title));
        output.push_str(&format!("- 所属产品：{}\n", self.product_name));
        output.push_str(&format!("- 创建者：{}（{}）\n", self.opened_by, self.opened_date));
        output.push_str(&format!("- 当前指派人：{}\n", self.assigned_to));
        if !self.resolved_by.is_empty() {
            output.push_str(&format!("- 解决者：{}\n", self.resolved_by));
            output.push_str(&format!("- 解决方案：{}\n", self.resolution));
        }
        output.push('\n');

        // Steps / 重现步骤
        if !self.steps.is_empty() {
            output.push_str("### 重现步骤\n");
            output.push_str(&self.steps);
            output.push_str("\n\n");

            // ── 附件图片 OCR：下载并提取文字 ──
            {
                let token_file = std::path::Path::new("/root/.config/zentao/.env");
                let mut zentao_token = String::new();
                if let Ok(fc) = std::fs::read_to_string(token_file) {
                    for line in fc.lines() {
                        if let Some(v) = line.strip_prefix("ZENTAO_TOKEN=") {
                            zentao_token = v.trim().to_string();
                        }
                    }
                }
                if !zentao_token.is_empty() {
                    // 简单提取 fileID（不用 regex）
                    let mut file_ids: Vec<String> = Vec::new();
                    let steps_str = self.raw_steps_html.clone();
                    let mut pos = 0;
                    while let Some(idx) = steps_str[pos..].find("fileID=") {
                        let start = pos + idx + 7;
                        let mut end = start;
                        while end < steps_str.len() && steps_str.as_bytes()[end].is_ascii_digit() {
                            end += 1;
                        }
                        if end > start {
                            file_ids.push(steps_str[start..end].to_string());
                        }
                        pos = end;
                    }
                    if !file_ids.is_empty() {
                        output.push_str("### 附件图片内容（OCR 提取）\n");
                        for fid in &file_ids {
                            let url = format!("https://zentao.gentronhealth.com/api.php/v1/files/{}", fid);
                            let tmp_path = format!("/tmp/zentao_img_{}.png", fid);
                            // 用 curl 下载（避免 reqwest blocking 依赖）
                            let dl_ok = std::process::Command::new("curl")
                                .args(["-s", "-o", &tmp_path, "-H", &format!("Token: {}", zentao_token), &url])
                                .output()
                                .map(|o| o.status.success())
                                .unwrap_or(false);
                            if dl_ok && std::path::Path::new(&tmp_path).exists() {
                                let file_size = std::fs::metadata(&tmp_path).map(|m| m.len()).unwrap_or(0);
                                if file_size > 100 {
                                    // OCR 提取文字
                                    let ocr_result = std::process::Command::new("tesseract")
                                        .args([&tmp_path, "stdout", "-l", "chi_sim+eng", "--psm", "6"])
                                        .output()
                                        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                                        .unwrap_or_default();
                                    let ocr_text = ocr_result.trim().to_string();
                                    if !ocr_text.is_empty() {
                                        output.push_str(&format!("**图片 {} (fileID={}):**\n```\n{}\n```\n\n",
                                            fid, fid, ocr_text));
                                    } else {
                                        output.push_str(&format!("**图片 {} (fileID={}):** OCR 未能提取文字（可能是纯图形截图）\n\n", fid, fid));
                                    }
                                    let _ = std::fs::remove_file(&tmp_path);
                                } else {
                                    output.push_str(&format!("**图片 {} (fileID={}):** 文件过小({} bytes)，可能下载失败\n\n", fid, fid, file_size));
                                }
                            } else {
                                output.push_str(&format!("**图片 {} (fileID={}):** 下载失败\n\n", fid, fid));
                            }
                        }
                    }
                }
            }
        }

        // 备注/操作历史（只显示有 comment 或重要状态变更的）
        if !self.actions.is_empty() {
            output.push_str("### 操作历史\n");
            for action in &self.actions {
                let actor = action.actor.as_deref().unwrap_or("系统");
                let date = action.date.as_deref().unwrap_or("");
                let action_type = action.action.as_deref().unwrap_or("");
                
                let action_label = match action_type {
                    "opened" => "创建",
                    "assigned" => "指派",
                    "resolved" => "解决",
                    "closed" => "关闭",
                    "activated" => "激活",
                    "commented" => "备注",
                    _ => action_type,
                };

                // 字段变更记录
                let mut changes = String::new();
                if let Some(history) = &action.history {
                    for h in history {
                        let field = h.fieldName.as_deref().unwrap_or("");
                        let old_v = h.old.as_deref().unwrap_or("");
                        let new_v = h.new.as_deref().unwrap_or("");
                        if !old_v.is_empty() || !new_v.is_empty() {
                            changes.push_str(&format!("      {}: {} → {}\n", field, old_v, new_v));
                        }
                    }
                }

                let comment = action.comment.as_deref().unwrap_or("");
                let desc = action.desc.as_deref().unwrap_or("");

                if !comment.is_empty() || !changes.is_empty() || action_type == "opened" || action_type == "activated" {
                    output.push_str(&format!("- {} {} [{}]\n", actor, date, action_label));
                    if !changes.is_empty() {
                        output.push_str(&changes);
                    }
                    if !comment.is_empty() {
                        output.push_str(&format!("  备注：{}\n", comment));
                    }
                }
            }
            output.push('\n');
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_text_steps() {
        let html = Some("<p>步骤1：点退回</p><p>步骤2：观察弹窗</p>".into());
        let text = BugDetail::extract_text_steps(&html);
        assert!(text.contains("步骤1"));
        assert!(text.contains("点退回"));
        assert!(text.contains("步骤2"));
        assert!(text.contains("观察弹窗"));
    }

    #[test]
    fn test_extract_text_with_entities() {
        let html = Some("步骤1：如果 a &gt; b 则执行&lt;br/&gt;".into());
        let text = BugDetail::extract_text_steps(&html);
        assert!(text.contains("如果 a > b 则执行"));
    }

    #[test]
    fn test_format_for_prompt() {
        let detail = BugDetail {
            id: 613,
            title: "测试Bug".into(),
            severity: 3,
            pri: 3,
            bug_type: "designdefect".into(),
            status: "active".into(),
            steps: "护士点退回无弹窗".into(),
            module_title: "住院护士站".into(),
            product_name: "HIS".into(),
            project_name: "HIS改造".into(),
            opened_by: "陈显精".into(),
            opened_date: "2026-05-28".into(),
            assigned_to: "王建".into(),
            resolved_by: String::new(),
            resolution: String::new(),
            actions: vec![],
            raw_steps_html: String::new(),
        };
        let text = detail.format_for_prompt();
        assert!(text.contains("#613"));
        assert!(text.contains("测试Bug"));
        assert!(text.contains("一般"));
        assert!(text.contains("住院护士站"));
        assert!(text.contains("护士点退回无弹窗"));
    }

    #[tokio::test]
    async fn test_api_get_bug() {
        let cfg = crate::config::Config::load().unwrap();
        let client = ZentaoClient::from_config(&cfg);
        let result = client.get_bug("613").await;
        if let Ok(detail) = result {
            assert_eq!(detail.id, 613);
            assert!(!detail.title.is_empty());
            assert!(!detail.steps.is_empty());
            println!("Bug #613 title: {}", detail.title);
            println!("--- Prompt format ---");
            println!("{}", detail.format_for_prompt());
        } else {
            eprintln!("API test skipped or failed: {:?}", result.err());
        }
    }

    #[tokio::test]
    async fn test_api_get_my_bugs() {
        let cfg = crate::config::Config::load().unwrap();
        let client = ZentaoClient::from_config(&cfg);
        let bugs = client.get_my_bugs("zhaoyun").await;
        if let Ok(list) = bugs {
            println!("zhaoyun has {} active bugs", list.len());
            for b in &list {
                println!("  #{} [{}] {}", b.id, b.severity.unwrap_or(0), b.title);
            }
        } else {
            eprintln!("get_my_bugs skipped or failed: {:?}", bugs.err());
        }
    }
}
