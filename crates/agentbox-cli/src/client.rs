use anyhow::{bail, Context, Result};
use reqwest::Client;

pub struct AgentBoxClient {
    base_url: String,
    http: Client,
}

impl AgentBoxClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http: Client::new(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    async fn check_response(resp: reqwest::Response) -> Result<reqwest::Response> {
        if resp.status().is_success() {
            return Ok(resp);
        }
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
            if let Some(msg) = json.get("error").and_then(|v| v.as_str()) {
                bail!("Server error ({status}): {msg}");
            }
        }
        bail!("Server error ({status}): {body}");
    }

    pub async fn post_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &impl serde::Serialize,
    ) -> Result<T> {
        let resp = self
            .http
            .post(self.url(path))
            .json(body)
            .send()
            .await
            .context("Failed to connect to daemon")?;
        let resp = Self::check_response(resp).await?;
        resp.json().await.context("Failed to parse response")
    }

    pub async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let resp = self
            .http
            .get(self.url(path))
            .send()
            .await
            .context("Failed to connect to daemon")?;
        let resp = Self::check_response(resp).await?;
        resp.json().await.context("Failed to parse response")
    }

    pub async fn get_json_with_query<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<T> {
        let resp = self
            .http
            .get(self.url(path))
            .query(query)
            .send()
            .await
            .context("Failed to connect to daemon")?;
        let resp = Self::check_response(resp).await?;
        resp.json().await.context("Failed to parse response")
    }

    pub async fn get_bytes(&self, path: &str, query: &[(&str, &str)]) -> Result<Vec<u8>> {
        let resp = self
            .http
            .get(self.url(path))
            .query(query)
            .send()
            .await
            .context("Failed to connect to daemon")?;
        let resp = Self::check_response(resp).await?;
        Ok(resp.bytes().await?.to_vec())
    }

    pub async fn delete_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let resp = self
            .http
            .delete(self.url(path))
            .send()
            .await
            .context("Failed to connect to daemon")?;
        let resp = Self::check_response(resp).await?;
        resp.json().await.context("Failed to parse response")
    }

    pub async fn post_multipart<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        form: reqwest::multipart::Form,
    ) -> Result<T> {
        let resp = self
            .http
            .post(self.url(path))
            .multipart(form)
            .send()
            .await
            .context("Failed to connect to daemon")?;
        let resp = Self::check_response(resp).await?;
        resp.json().await.context("Failed to parse response")
    }
}
