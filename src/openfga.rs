//! Minimal OpenFGA relationship client used by the structured authorization UI.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct OpenFgaClient {
    http: reqwest::Client,
    url: String,
    store: String,
    token: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct TupleKey {
    pub user: String,
    pub relation: String,
    pub object: String,
}

impl OpenFgaClient {
    pub fn new(url: String, store: String, token: String) -> Result<Self, String> {
        if url.trim().is_empty() || store.trim().is_empty() {
            return Err("OpenFGA API URL and store ID are required".into());
        }
        Ok(Self {
            http: reqwest::Client::new(),
            url: url.trim_end_matches('/').to_owned(),
            store,
            token,
        })
    }

    fn request(&self, method: reqwest::Method, suffix: &str) -> reqwest::RequestBuilder {
        let request = self.http.request(
            method,
            format!("{}/stores/{}/{}", self.url, self.store, suffix),
        );
        if self.token.is_empty() {
            request
        } else {
            request.bearer_auth(&self.token)
        }
    }

    pub async fn read(&self) -> Result<Vec<TupleKey>, String> {
        let response = self
            .request(reqwest::Method::POST, "read")
            .json(&serde_json::json!({"tuple_key":{}}))
            .send()
            .await
            .map_err(|error| error.to_string())?;
        let value = decode(response).await?;
        Ok(value
            .get("tuples")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|tuple| serde_json::from_value(tuple.get("key")?.clone()).ok())
            .collect())
    }

    pub async fn write(&self, tuple: &TupleKey, delete: bool) -> Result<(), String> {
        let section = if delete { "deletes" } else { "writes" };
        let body = serde_json::json!({(section):{"tuple_keys":[tuple]}});
        decode(
            self.request(reqwest::Method::POST, "write")
                .json(&body)
                .send()
                .await
                .map_err(|error| error.to_string())?,
        )
        .await?;
        Ok(())
    }

    pub async fn check(&self, tuple: &TupleKey) -> Result<bool, String> {
        let value = decode(
            self.request(reqwest::Method::POST, "check")
                .json(&serde_json::json!({"tuple_key":tuple}))
                .send()
                .await
                .map_err(|error| error.to_string())?,
        )
        .await?;
        value
            .get("allowed")
            .and_then(serde_json::Value::as_bool)
            .ok_or_else(|| "OpenFGA check response omitted allowed".into())
    }
}

async fn decode(response: reqwest::Response) -> Result<serde_json::Value, String> {
    let status = response.status();
    let value: serde_json::Value = response.json().await.map_err(|error| error.to_string())?;
    if status.is_success() {
        Ok(value)
    } else {
        Err(value
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("OpenFGA request failed")
            .to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::TupleKey;

    #[test]
    fn tuple_serialization_matches_openfga_api() {
        let tuple = TupleKey {
            user: "user:anne".into(),
            relation: "viewer".into(),
            object: "project:ops".into(),
        };
        assert_eq!(
            serde_json::to_value(tuple).unwrap(),
            serde_json::json!({"user":"user:anne","relation":"viewer","object":"project:ops"})
        );
    }
}
