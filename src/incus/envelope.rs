use serde::Deserialize;

/// The common response envelope returned by Incus REST endpoints.
#[derive(Debug, Deserialize)]
pub struct ApiResponse<T> {
    #[serde(rename = "type")]
    pub response_type: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub status_code: u16,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub error_code: i64,
    pub metadata: Option<T>,
    pub operation: Option<String>,
}

impl<T> ApiResponse<T> {
    /// Reports whether Incus classified this response as a successful request.
    pub fn is_success(&self) -> bool {
        self.error.is_empty() && self.status_code < 400
    }
}

#[cfg(test)]
mod tests {
    use super::ApiResponse;

    #[test]
    fn decodes_an_incus_sync_envelope() {
        let response: ApiResponse<Vec<String>> = serde_json::from_str(
            r#"{
                "type": "sync",
                "status": "Success",
                "status_code": 200,
                "operation": "",
                "error": "",
                "error_code": 0,
                "metadata": ["/1.0/instances/demo"]
            }"#,
        )
        .expect("valid Incus envelope");

        assert!(response.is_success());
        assert_eq!(
            response.metadata.as_deref(),
            Some(["/1.0/instances/demo".to_owned()].as_slice())
        );
    }
}
