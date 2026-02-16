//! Voice transcription provider (Groq Whisper endpoint).

#[derive(Clone)]
pub struct GroqTranscriber {
    api_key: String,
    api_base: String,
    client: reqwest::Client,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct TranscriptionResponse {
    pub text: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub language: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub duration: Option<f64>,
}

impl GroqTranscriber {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            api_base: "https://api.groq.com/openai/v1".to_string(),
            client: reqwest::Client::new(),
        }
    }

    pub fn is_available(&self) -> bool {
        !self.api_key.trim().is_empty()
    }

    pub async fn transcribe(
        &self,
        audio_file_path: &std::path::Path,
    ) -> anyhow::Result<TranscriptionResponse> {
        let form = reqwest::multipart::Form::new()
            .text("model", "whisper-large-v3")
            .text("response_format", "json")
            .file("file", audio_file_path)
            .await?;

        let resp = self
            .client
            .post(format!("{}/audio/transcriptions", self.api_base))
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("transcription API error: {}", body));
        }
        let parsed = resp.json::<TranscriptionResponse>().await?;
        Ok(parsed)
    }
}
