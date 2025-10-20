use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream};
use hound::{WavSpec, WavWriter};
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, error, info};

pub struct AudioInput {
    device: Device,
    config: cpal::StreamConfig,
    stream: Option<Stream>,
    wav_writer: Arc<Mutex<Option<WavWriter<std::io::BufWriter<std::fs::File>>>>>,
}

impl AudioInput {
    pub fn new() -> Result<Self> {
        let host = cpal::default_host();

        // Get the default input device
        let device = host
            .default_input_device()
            .context("Failed to get default input device")?;

        debug!("Using input device: {}", device.name()?);

        // Get the default config for the input device
        let config = device
            .default_input_config()
            .context("Failed to get default input config")?
            .config();

        debug!(
            "Input config: {} channels, {} Hz sample rate",
            config.channels, config.sample_rate.0
        );

        Ok(Self {
            device,
            config,
            stream: None,
            wav_writer: Arc::new(Mutex::new(None)),
        })
    }

    #[cfg(false)]
    #[allow(dead_code)]
    pub fn new_with_device_name(device_name: &str) -> Result<Self> {
        let host = cpal::default_host();

        // Find device by name
        let devices = host.input_devices()?;
        let device = devices
            .filter_map(|d| {
                if let Ok(name) = d.name() {
                    if name == device_name {
                        Some(d)
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .next()
            .context(format!("Device '{device_name}' not found"))?;

        debug!("Using input device: {}", device.name()?);

        // Get the default config for the input device
        let config = device
            .default_input_config()
            .context("Failed to get default input config")?
            .config();

        debug!(
            "Input config: {} channels, {} Hz sample rate",
            config.channels, config.sample_rate.0
        );

        Ok(Self {
            device,
            config,
            stream: None,
            wav_writer: Arc::new(Mutex::new(None)),
        })
    }

    pub fn list_available_devices() -> Result<Vec<String>> {
        let host = cpal::default_host();
        let devices = host.input_devices()?;

        let mut device_names = Vec::new();
        for device in devices {
            if let Ok(name) = device.name() {
                device_names.push(name);
            }
        }

        Ok(device_names)
    }

    pub fn start_recording<F>(&mut self, mut callback: F) -> Result<()>
    where
        F: FnMut(&[f32]) + Send + 'static,
    {
        let err_fn = |err| error!("An error occurred on the audio stream: {}", err);
        let wav_writer_clone = self.wav_writer.clone();

        let stream = match self.device.default_input_config()?.sample_format() {
            SampleFormat::F32 => {
                self.device.build_input_stream(
                    &self.config,
                    move |data: &[f32], _: &_| {
                        // Write to WAV file if active
                        if let Some(ref mut writer) = *wav_writer_clone.lock() {
                            for &sample in data {
                                let _ = writer.write_sample(sample);
                            }
                        }
                        callback(data);
                    },
                    err_fn,
                    None,
                )?
            }
            SampleFormat::I16 => {
                self.device.build_input_stream(
                    &self.config,
                    move |data: &[i16], _: &_| {
                        // Convert i16 samples to f32
                        let float_data: Vec<f32> =
                            data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                        
                        // Write to WAV file if active
                        if let Some(ref mut writer) = *wav_writer_clone.lock() {
                            for &sample in &float_data {
                                let _ = writer.write_sample(sample);
                            }
                        }
                        callback(&float_data);
                    },
                    err_fn,
                    None,
                )?
            }
            SampleFormat::U16 => {
                self.device.build_input_stream(
                    &self.config,
                    move |data: &[u16], _: &_| {
                        // Convert u16 samples to f32
                        let float_data: Vec<f32> = data
                            .iter()
                            .map(|&s| ((s as f32 / u16::MAX as f32) * 2.0) - 1.0)
                            .collect();
                        
                        // Write to WAV file if active
                        if let Some(ref mut writer) = *wav_writer_clone.lock() {
                            for &sample in &float_data {
                                let _ = writer.write_sample(sample);
                            }
                        }
                        callback(&float_data);
                    },
                    err_fn,
                    None,
                )?
            }
            _ => return Err(anyhow::anyhow!("Unsupported sample format")),
        };

        stream.play()?;
        self.stream = Some(stream);

        Ok(())
    }

    #[allow(dead_code)]
    pub fn stop_recording(&mut self) {
        self.stream = None;
    }

    pub fn get_sample_rate(&self) -> u32 {
        self.config.sample_rate.0
    }

    pub fn get_channels(&self) -> u16 {
        self.config.channels
    }

    /// Start saving audio to a WAV file
    pub fn start_saving_to_file<P: Into<PathBuf>>(&self, path: P) -> Result<()> {
        let path = path.into();
        
        let spec = WavSpec {
            channels: self.config.channels,
            sample_rate: self.config.sample_rate.0,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };

        let writer = WavWriter::create(&path, spec)
            .context(format!("Failed to create WAV file at {:?}", path))?;

        *self.wav_writer.lock() = Some(writer);
        
        info!("Started saving audio to file: {:?}", path);
        Ok(())
    }

    /// Stop saving audio and finalize the WAV file
    pub fn stop_saving_to_file(&self) -> Result<()> {
        if let Some(writer) = self.wav_writer.lock().take() {
            writer.finalize().context("Failed to finalize WAV file")?;
            info!("Stopped saving audio and finalized WAV file");
        }
        Ok(())
    }
}
