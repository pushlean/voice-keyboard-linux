use anyhow::{Context, Result};
use reqwest::multipart;
use serde::{Deserialize, Serialize};
use std::env;
use tracing::{debug, info};

pub const WHISPER_API_URL: &str = "https://api.openai.com/v1/audio/transcriptions";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhisperResponse {
    pub text: String,
}

pub struct WhisperClient {
    api_url: String,
    api_key: Option<String>,
}

impl WhisperClient {
    pub fn new(api_url: Option<&str>) -> Self {
        let api_key = env::var("OPENAI_API_KEY").ok();
        
        if api_key.is_none() {
            debug!("OPENAI_API_KEY not set; API calls may fail");
        }

        Self {
            api_url: api_url.unwrap_or(WHISPER_API_URL).to_string(),
            api_key,
        }
    }

    /// Transcribe audio data using OpenAI Whisper API
    /// audio_data: PCM 16-bit audio data
    /// sample_rate: Sample rate of the audio
    pub async fn transcribe(&self, audio_data: &[u8], sample_rate: u32) -> Result<String> {
        debug!("Preparing to send {} bytes of audio data to Whisper API", audio_data.len());

        // Convert PCM data to WAV format
        let wav_data = Self::pcm_to_wav(audio_data, sample_rate)?;
        
        debug!("Converted to WAV format: {} bytes", wav_data.len());

        // Get API key
        let api_key = self.api_key.as_ref()
            .context("OPENAI_API_KEY environment variable is not set")?;

        // Build multipart form
        let part = multipart::Part::bytes(wav_data)
            .file_name("audio.wav")
            .mime_str("audio/wav")?;

        let form = multipart::Form::new()
            .part("file", part)
            .text("model", "whisper-1");

        // Send request
        info!("Sending audio to OpenAI Whisper API...");
        let client = reqwest::Client::new();
        let response = client
            .post(&self.api_url)
            .header("Authorization", format!("Bearer {}", api_key))
            .multipart(form)
            .send()
            .await
            .context("Failed to send request to Whisper API")?;

        // Check for errors
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "<no body>".to_string());
            return Err(anyhow::anyhow!(
                "Whisper API request failed with status {}: {}",
                status,
                error_text
            ));
        }

        // Parse response
        let whisper_response: WhisperResponse = response.json().await
            .context("Failed to parse Whisper API response")?;

        // Trim whitespace from the transcription
        let trimmed_text = whisper_response.text.trim().to_string();
        
        info!("Received transcription from Whisper API: {}", trimmed_text);
        Ok(trimmed_text)
    }

    /// Convert PCM 16-bit audio data to WAV format
    fn pcm_to_wav(pcm_data: &[u8], sample_rate: u32) -> Result<Vec<u8>> {
        let mut wav_data = Vec::new();
        
        // WAV header
        let num_samples = pcm_data.len() / 2; // 16-bit = 2 bytes per sample
        let byte_rate = sample_rate * 2; // 16-bit mono
        let data_size = pcm_data.len() as u32;
        let file_size = 36 + data_size;

        // RIFF header
        wav_data.extend_from_slice(b"RIFF");
        wav_data.extend_from_slice(&file_size.to_le_bytes());
        wav_data.extend_from_slice(b"WAVE");

        // fmt chunk
        wav_data.extend_from_slice(b"fmt ");
        wav_data.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
        wav_data.extend_from_slice(&1u16.to_le_bytes()); // audio format (1 = PCM)
        wav_data.extend_from_slice(&1u16.to_le_bytes()); // num channels (1 = mono)
        wav_data.extend_from_slice(&sample_rate.to_le_bytes()); // sample rate
        wav_data.extend_from_slice(&byte_rate.to_le_bytes()); // byte rate
        wav_data.extend_from_slice(&2u16.to_le_bytes()); // block align
        wav_data.extend_from_slice(&16u16.to_le_bytes()); // bits per sample

        // data chunk
        wav_data.extend_from_slice(b"data");
        wav_data.extend_from_slice(&data_size.to_le_bytes());
        wav_data.extend_from_slice(pcm_data);

        debug!("Created WAV file: {} samples, {} Hz, {} bytes", num_samples, sample_rate, wav_data.len());
        Ok(wav_data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pcm_to_wav() {
        // Create simple PCM data (1 second of silence at 16kHz)
        let sample_rate = 16000;
        let pcm_data = vec![0u8; sample_rate as usize * 2]; // 16-bit = 2 bytes per sample
        
        let wav_data = WhisperClient::pcm_to_wav(&pcm_data, sample_rate).unwrap();
        
        // WAV header should be 44 bytes
        assert!(wav_data.len() >= 44);
        
        // Check RIFF header
        assert_eq!(&wav_data[0..4], b"RIFF");
        assert_eq!(&wav_data[8..12], b"WAVE");
        
        // Check fmt chunk
        assert_eq!(&wav_data[12..16], b"fmt ");
        
        // Check data chunk
        assert_eq!(&wav_data[36..40], b"data");
    }
}

